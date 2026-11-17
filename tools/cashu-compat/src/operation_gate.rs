//! Write-ahead guard around the real CDK connector. This never issues credit.
use crate::{
    journal::Journal,
    transport::serialize_body,
    writer_lock::{Authority, Operation, WriterLock},
};
mod split;
use anyhow::{Result, ensure};
use async_trait::async_trait;
use cdk::nuts::AuthToken;
use cdk_http_client::{
    HttpError, RawResponse, Transport,
    ws::{WsError, WsReceiver, WsSender},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fmt,
    sync::{Arc, Mutex},
};
use url::Url;
use uuid::Uuid;

fn refused() -> HttpError {
    HttpError::Other("wallet operation requires reconciliation".into())
}

#[derive(Clone)]
pub struct OperationGate<T: Transport> {
    inner: T,
    journal: Arc<Mutex<Journal>>,
    active: crate::writer_lock::ActiveOperation,
    limit: usize,
    database:
        Option<Arc<dyn cdk::cdk_database::WalletDatabase<cdk::cdk_database::Error> + Send + Sync>>,
    key_policy: Option<(String, Arc<crate::intake::PinnedKeys>)>,
    treasury: Option<Arc<TreasuryPolicy>>,
    split_policy: Option<Arc<crate::journal::treasury_split::AdmissionPolicy>>,
    _writer: Arc<WriterLock>,
}
/// Private per-wallet scope. This is never serialized or logged.
pub struct TreasuryPolicy {
    pub binding: crate::treasury::TreasuryBinding,
    pub mint: crate::mint_policy::MintPolicy,
    pub seed: [u8; 64],
}

impl<T: Transport> fmt::Debug for OperationGate<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationGate").finish_non_exhaustive()
    }
}

/// Dropping a canceled task removes process authority, never its durable intent.
pub struct OperationLease {
    active: crate::writer_lock::ActiveOperation,
    id: Uuid,
}
impl Drop for OperationLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock()
            && active
                .as_ref()
                .is_some_and(|operation| operation.id == self.id)
        {
            *active = None;
        }
    }
}
impl<T: Transport> OperationGate<T> {
    pub fn new(
        inner: T,
        journal: Arc<Mutex<Journal>>,
        writer: Arc<WriterLock>,
        limit: usize,
    ) -> Result<Self> {
        ensure!(
            limit > 0 && limit <= 1_048_576,
            "invalid operation message limit"
        );
        Ok(Self {
            inner,
            journal,
            active: writer.coordinator(),
            limit,
            database: None,
            key_policy: None,
            treasury: None,
            split_policy: None,
            _writer: writer,
        })
    }
    pub fn with_key_policy(
        mut self,
        mint_id: String,
        keys: Arc<crate::intake::PinnedKeys>,
    ) -> Self {
        self.key_policy = Some((mint_id, keys));
        self
    }

    pub fn with_wallet_database(
        mut self,
        database: Arc<
            dyn cdk::cdk_database::WalletDatabase<cdk::cdk_database::Error> + Send + Sync,
        >,
    ) -> Self {
        self.database = Some(database);
        self
    }

    pub fn with_treasury_policy(mut self, policy: Arc<TreasuryPolicy>) -> Self {
        self.treasury = Some(policy);
        self
    }

    /// Explicit execution authority for a bound treasury intent.
    /// The journal separately gates quote creation and prepared melt submission.
    /// Shares the acquisition writer lease; cannot run alongside another swap.
    pub fn arm_treasury(&self, id: Uuid) -> Result<OperationLease> {
        let policy = self
            .treasury
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("treasury disabled"))?;
        ensure!(
            self.database.is_some() && self.key_policy.is_some(),
            "treasury dependencies missing"
        );
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
        ensure!(active.is_none(), "another wallet operation is active");
        let journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        let issued = journal.treasury_instruction(&policy.binding, id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        policy.binding.validate_new_invoice(&issued, now)?;
        *active = Some(Operation {
            id,
            authority: Authority::Treasury,
        });
        Ok(OperationLease {
            active: self.active.clone(),
            id,
        })
    }

    async fn create_treasury_quote<P: Serialize + Send + Sync, R: DeserializeOwned>(
        &self,
        url: Url,
        payload: &P,
    ) -> Result<R, HttpError> {
        let policy = self.treasury.as_ref().ok_or_else(refused)?;
        let operation = self.active_operation()?;
        if operation.authority != Authority::Treasury {
            return Err(refused());
        }
        let id = operation.id;
        let bytes = serialize_body(payload, self.limit)?;
        let request: cdk::nuts::MeltQuoteBolt11Request =
            serde_json::from_slice(&bytes).map_err(|_| refused())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| refused())?
            .as_secs();
        self.journal
            .lock()
            .map_err(|_| refused())?
            .claim_treasury_quote_request(&policy.binding, &policy.mint, id, &url, &request, now)
            .map_err(|_| refused())?;
        let response: serde_json::Value = self.inner.http_post(url, None, payload).await?;
        let reply = serialize_body(&response, self.limit)?;
        self.journal
            .lock()
            .map_err(|_| refused())?
            .record_treasury_quote_reply(id, &reply)
            .map_err(|_| refused())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| refused())?
            .as_secs();
        self.journal
            .lock()
            .map_err(|_| refused())?
            .adopt_retained_treasury_quote(&policy.binding, &policy.mint, id, now)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        serde_json::from_value(response).map_err(|_| refused())
    }

    async fn freeze_and_claim_melt(&self, url: &Url, bytes: &[u8]) -> Result<Uuid, HttpError> {
        let policy = self.treasury.as_ref().ok_or_else(refused)?;
        let operation = self.active_operation()?;
        if operation.authority != Authority::Treasury {
            return Err(refused());
        }
        let id = operation.id;
        let database = self.database.as_ref().ok_or_else(refused)?;
        let request: cdk::nuts::MeltRequest<String> =
            serde_json::from_slice(bytes).map_err(|_| refused())?;
        let sagas = database
            .get_incomplete_sagas()
            .await
            .map_err(|_| refused())?;
        if sagas.len() > 1024 {
            return Err(refused());
        }
        let mut matching = sagas
            .iter()
            .filter(|s| s.quote_id.as_ref() == Some(request.quote_id()));
        let saga = matching.next().ok_or_else(refused)?;
        if matching.next().is_some()
            || saga
                .mint_url
                .join("v1/melt/bolt11")
                .map_err(|_| refused())?
                != *url
        {
            return Err(refused());
        }
        let (mint_id, keys) = self.key_policy.as_ref().ok_or_else(refused)?;
        if *mint_id != policy.binding.mint_id {
            return Err(refused());
        }
        let ys = request
            .inputs()
            .iter()
            .map(|p| p.y())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        let owned = database
            .get_proofs_by_ys(ys.clone())
            .await
            .map_err(|_| refused())?;
        let fresh: cdk::nuts::CheckStateResponse = self
            .inner
            .http_post(
                saga.mint_url.join("v1/checkstate").map_err(|_| refused())?,
                None,
                &cdk::nuts::CheckStateRequest { ys },
            )
            .await?;
        crate::melt_derivation::verify_melt_inputs(
            &request,
            mint_id,
            &saga.mint_url,
            &owned,
            keys,
            &fresh,
        )
        .map_err(|_| refused())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| refused())?
            .as_secs();
        let mut journal = self.journal.lock().map_err(|_| refused())?;
        journal
            .freeze_treasury_melt_request(&policy.binding, id, saga, &request, &policy.seed, now)
            .map_err(|_| refused())?;
        journal
            .claim_treasury_melt_submission(&policy.binding, id, url, bytes, now)
            .map_err(|_| refused())?;
        Ok(id)
    }

    async fn capture_derivation(
        &self,
        id: Uuid,
        url: &Url,
        request: &[u8],
    ) -> Result<(), HttpError> {
        let Some(database) = &self.database else {
            // Unit boundary fixtures have no CDK database and cannot seal ownership.
            #[cfg(test)]
            return Ok(());
            #[cfg(not(test))]
            return Err(refused());
        };
        let swap: cdk::nuts::SwapRequest =
            serde_json::from_slice(request).map_err(|_| refused())?;
        let sagas = database
            .get_incomplete_sagas()
            .await
            .map_err(|_| refused())?;
        if sagas.len() > 1024 {
            return Err(refused());
        }
        let mut binding = None;
        for saga in sagas {
            if saga.mint_url.join("v1/swap").map_err(|_| refused())? != *url {
                continue;
            }
            if let Ok(candidate) = crate::ownership::SwapDerivation::capture(&saga, &swap) {
                if binding.is_some() {
                    return Err(refused());
                }
                binding = Some(candidate);
            }
        }
        let binding = binding.ok_or_else(refused)?;
        let (mint_id, keys) = self.key_policy.as_ref().ok_or_else(refused)?;
        if self
            .journal
            .lock()
            .map_err(|_| refused())?
            .mint_id(id)
            .map_err(|_| refused())?
            != *mint_id
        {
            return Err(refused());
        }
        let stored = database
            .get_keys(&binding.keyset_id)
            .await
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        for output in &binding.outputs {
            let qualified = keys
                .key(mint_id, output.keyset_id, u64::from(output.amount))
                .map_err(|_| refused())?;
            if stored.amount_key(output.amount) != Some(qualified) {
                return Err(refused());
            }
        }

        self.journal
            .lock()
            .map_err(|_| refused())?
            .record_derivation(id, &binding)
            .map_err(|_| refused())
    }

    pub fn arm(
        &self,
        id: Uuid,
        recover_original: bool,
        maximum_receive_fee_sats: u64,
    ) -> Result<OperationLease> {
        self.arm_with_recovery_fence(id, recover_original, maximum_receive_fee_sats, false)
    }

    /// CDK may have compensated a rejected receive while our original recovery
    /// record remains. New, disjoint inputs may proceed only after CDK cleanup
    /// and with every retained output counter range durably reserved.
    pub async fn arm_new_acquisition(
        &self,
        id: Uuid,
        maximum_receive_fee_sats: u64,
    ) -> Result<OperationLease> {
        let database = self.database.as_ref().ok_or_else(refused)?;
        let (mint_id, _) = self.key_policy.as_ref().ok_or_else(refused)?;
        let lease = self.arm_with_recovery_fence(id, false, maximum_receive_fee_sats, true)?;
        let bindings = {
            let journal = self.journal.lock().map_err(|_| refused())?;
            ensure!(
                journal.mint_id(id)? == *mint_id,
                "acquisition mint mismatch"
            );
            journal.unresolved_derivations_except(id)?
        };
        let sagas = database.get_incomplete_sagas().await?;
        // Stores are mint-scoped. Any incomplete CDK work requires its original
        // recovery path; no success/failure inference comes from a mint error.
        ensure!(sagas.is_empty(), "CDK wallet work remains unresolved");
        for binding in bindings {
            let counter = database
                .increment_keyset_counter(&binding.keyset_id, 0)
                .await?;
            ensure!(
                counter >= binding.counter_end,
                "retained output counter range is not reserved"
            );
        }
        Ok(lease)
    }

    fn arm_with_recovery_fence(
        &self,
        id: Uuid,
        recover_original: bool,
        maximum_receive_fee_sats: u64,
        retained_recovery_checked: bool,
    ) -> Result<OperationLease> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
        ensure!(active.is_none(), "another wallet operation is active");
        let journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        let state = journal.state(id)?;
        if recover_original {
            ensure!(state == "acquiring", "original operation is not unresolved");
        } else {
            ensure!(
                state == "validated" || state == "acquisition_prepared",
                "new operation is not admitted"
            );
            ensure!(
                retained_recovery_checked || !journal.mint_has_other_unresolved_operation(id)?,
                "mint has unresolved financial work"
            );
        }
        ensure!(
            maximum_receive_fee_sats <= i64::MAX as u64,
            "invalid receive fee authority"
        );
        *active = Some(Operation {
            id,
            authority: Authority::Acquisition(maximum_receive_fee_sats),
        });
        Ok(OperationLease {
            active: self.active.clone(),
            id,
        })
    }
    fn active_operation(&self) -> Result<Operation, HttpError> {
        self.active
            .lock()
            .map_err(|_| refused())?
            .ok_or_else(refused)
    }
    fn freeze(&self, request: &[u8]) -> Result<Uuid, HttpError> {
        let operation = self.active_operation()?;
        let Authority::Acquisition(maximum_receive_fee_sats) = operation.authority else {
            return Err(refused());
        };
        let id = operation.id;
        let mut journal = self.journal.lock().map_err(|_| refused())?;
        let swap: cdk::nuts::SwapRequest =
            serde_json::from_slice(request).map_err(|_| refused())?;
        let fee = journal
            .verify_swap_inputs(id, &swap)
            .map_err(|_| refused())?;
        if fee > maximum_receive_fee_sats {
            return Err(refused());
        }
        journal.prepare(id, request).map_err(|_| refused())?;
        match journal.state(id).map_err(|_| refused())?.as_str() {
            "acquisition_prepared" => journal.start_acquisition(id).map_err(|_| refused())?,
            "acquiring" => {}
            _ => return Err(refused()),
        }
        Ok(id)
    }
}

#[async_trait]
impl<T: Transport + 'static> Transport for OperationGate<T> {
    fn with_proxy(
        &mut self,
        _url: Url,
        _matcher: Option<&str>,
        _invalid_certs: bool,
    ) -> Result<(), HttpError> {
        Err(refused())
    }
    async fn ws_connect(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
    ) -> Result<(WsSender, WsReceiver), WsError> {
        Err(WsError::NotSupported("wallet WebSockets disabled".into()))
    }
    async fn http_get<R: DeserializeOwned>(
        &self,
        url: Url,
        auth: Option<AuthToken>,
    ) -> Result<R, HttpError> {
        self.inner.http_get(url, auth).await
    }
    async fn http_get_raw(
        &self,
        url: Url,
        auth: Option<AuthToken>,
    ) -> Result<RawResponse, HttpError> {
        self.inner.http_get_raw(url, auth).await
    }
    async fn http_post<P: Serialize + Send + Sync, R: DeserializeOwned>(
        &self,
        url: Url,
        auth: Option<AuthToken>,
        payload: &P,
    ) -> Result<R, HttpError> {
        if auth.is_some() {
            return Err(refused());
        }
        if url.path().ends_with("/v1/checkstate") || url.path().ends_with("/v1/restore") {
            return self.inner.http_post(url, None, payload).await;
        }
        if url.path().ends_with("/v1/melt/quote/bolt11") {
            return self.create_treasury_quote(url, payload).await;
        }
        if url.path().ends_with("/v1/melt/bolt11") {
            let request = serialize_body(payload, self.limit)?;
            let id = self.freeze_and_claim_melt(&url, &request).await?;
            let response: serde_json::Value = self.inner.http_post(url, None, payload).await?;
            let reply = serialize_body(&response, self.limit)?;
            self.journal
                .lock()
                .map_err(|_| refused())?
                .record_treasury_melt_reply(id, &reply)
                .map_err(|_| refused())?;
            return serde_json::from_value(response).map_err(|_| refused());
        }
        if !url.path().ends_with("/v1/swap") {
            return Err(refused());
        }
        let request = serialize_body(payload, self.limit)?;
        if self.active_operation()?.authority == Authority::Split {
            return self.submit_split(url, payload, &request).await;
        }
        let id = self.freeze(&request)?;
        self.capture_derivation(id, &url, &request).await?;
        let response: serde_json::Value = self.inner.http_post(url, None, payload).await?;
        let reply = serialize_body(&response, self.limit)?;
        self.journal
            .lock()
            .map_err(|_| refused())?
            .record_swap_reply(id, &reply)
            .map_err(|_| refused())?;
        serde_json::from_value(response).map_err(|_| refused())
    }
    async fn http_post_form_raw<P: Serialize + Send + Sync>(
        &self,
        _url: Url,
        _auth: Option<AuthToken>,
        _payload: &P,
    ) -> Result<RawResponse, HttpError> {
        Err(refused())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Admission, Transport as Rail, ValidatedIntake};
    use cdk::{
        Amount,
        nuts::{Id, Proof, PublicKey},
        secret::Secret,
    };
    use std::{
        str::FromStr,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[derive(Clone)]
    struct Fixture {
        journal: Arc<Mutex<Journal>>,
        id: Uuid,
        spent: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
        executions: Arc<AtomicUsize>,
        lose_first: bool,
    }
    impl fmt::Debug for Fixture {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Fixture")
        }
    }
    #[async_trait]
    impl Transport for Fixture {
        fn with_proxy(
            &mut self,
            _url: Url,
            _matcher: Option<&str>,
            _invalid: bool,
        ) -> Result<(), HttpError> {
            Err(refused())
        }
        async fn http_get<R: DeserializeOwned>(
            &self,
            _url: Url,
            _auth: Option<AuthToken>,
        ) -> Result<R, HttpError> {
            Err(refused())
        }
        async fn http_get_raw(
            &self,
            _url: Url,
            _auth: Option<AuthToken>,
        ) -> Result<RawResponse, HttpError> {
            Err(refused())
        }
        async fn http_post<P: Serialize + Send + Sync, R: DeserializeOwned>(
            &self,
            _url: Url,
            _auth: Option<AuthToken>,
            payload: &P,
        ) -> Result<R, HttpError> {
            // The fixture's irreversible boundary observes the committed journal,
            // not process memory or a claimed intent from the caller.
            {
                let journal = self.journal.lock().unwrap();
                assert_eq!(journal.state(self.id).unwrap(), "acquiring");
                assert_eq!(
                    journal.prepared_request(self.id).unwrap().unwrap(),
                    serialize_body(payload, 4096).unwrap()
                );
            }
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.spent.swap(true, Ordering::SeqCst) {
                self.executions.fetch_add(1, Ordering::SeqCst);
            }
            if call == 0 && self.lose_first {
                return Err(HttpError::Timeout);
            }
            serde_json::from_value(serde_json::json!({"signatures":[]})).map_err(|_| refused())
        }
        async fn http_post_form_raw<P: Serialize + Send + Sync>(
            &self,
            _url: Url,
            _auth: Option<AuthToken>,
            _payload: &P,
        ) -> Result<RawResponse, HttpError> {
            Err(refused())
        }
    }
    fn proof(secret: &str) -> Proof {
        Proof::new(
            Amount::from(8),
            Id::from_str("00deadbeef123456").unwrap(),
            Secret::new(secret),
            PublicKey::from_hex(
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )
            .unwrap(),
        )
    }

    fn admit(journal: &mut Journal, secret: &str, mint: &str) -> Uuid {
        let proof = proof(secret);
        match journal
            .admit(&ValidatedIntake {
                mint_id: mint,
                policy_version: "fixture",
                transport: Rail::Cashu,
                source_ref: secret,
                source_author: None,
                proofs: &[proof],
            })
            .unwrap()
        {
            Admission::New(id) => id,
            _ => panic!("new fixture operation expected"),
        }
    }
    fn request(secret: &str) -> serde_json::Value {
        let input = proof(secret);
        let output = cdk::nuts::BlindedMessage::new(input.amount, input.keyset_id, input.c);
        serde_json::to_value(cdk::nuts::SwapRequest::new(vec![input], vec![output])).unwrap()
    }

    fn fixture(journal: Arc<Mutex<Journal>>, id: Uuid, lose_first: bool) -> Fixture {
        Fixture {
            journal,
            id,
            lose_first,
            spent: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
            executions: Arc::new(AtomicUsize::new(0)),
        }
    }
    #[tokio::test]
    async fn lost_reply_recovers_exact_request_without_new_execution_or_credit() -> Result<()> {
        let dir = crate::test_directory()?;
        let writer = Arc::new(WriterLock::acquire(dir.path())?);
        let journal = Arc::new(Mutex::new(Journal::open(
            &dir.path().join("journal.sqlite"),
            Uuid::new_v4(),
            8,
        )?));
        let id = admit(&mut journal.lock().unwrap(), "first", "mint-a");
        let inner = fixture(journal.clone(), id, true);
        let gate = OperationGate::new(inner.clone(), journal.clone(), writer, 4096)?;
        let url = Url::parse("https://mint.example/v1/swap")?;
        let request = request("first");
        assert!(
            gate.http_post::<_, serde_json::Value>(url.clone(), None, &request)
                .await
                .is_err()
        );
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
        let lease = gate.arm(id, false, 0)?;
        assert!(
            gate.http_post::<_, serde_json::Value>(url.clone(), None, &request)
                .await
                .is_err()
        );
        assert_eq!(journal.lock().unwrap().state(id)?, "acquiring");
        drop(lease); // cancellation/restart removes authority, never the durable request.
        assert!(gate.arm(id, false, 0).is_err());
        let _recover = gate.arm(id, true, 0)?;
        let mut wrong = request.clone();
        wrong["outputs"][0]["amount"] = 2.into();
        assert!(
            gate.http_post::<_, serde_json::Value>(url.clone(), None, &wrong)
                .await
                .is_err()
        );
        let _: serde_json::Value = gate.http_post(url, None, &request).await?;
        assert_eq!(inner.executions.load(Ordering::SeqCst), 1);
        assert_eq!(inner.calls.load(Ordering::SeqCst), 2);
        assert!(journal.lock().unwrap().swap_reply(id)?.is_some());
        assert!(journal.lock().unwrap().export_receipt(id).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn same_mint_uncertainty_blocks_new_swap_but_not_other_mint_admission() -> Result<()> {
        let dir = crate::test_directory()?;
        let writer = Arc::new(WriterLock::acquire(dir.path())?);
        let journal = Arc::new(Mutex::new(Journal::open(
            &dir.path().join("journal.sqlite"),
            Uuid::new_v4(),
            8,
        )?));
        let first = admit(&mut journal.lock().unwrap(), "first", "mint-a");
        let second = admit(&mut journal.lock().unwrap(), "second", "mint-a");
        let other = admit(&mut journal.lock().unwrap(), "other", "mint-b");
        let gate = OperationGate::new(
            fixture(journal.clone(), first, false),
            journal.clone(),
            writer.clone(),
            4096,
        )?;
        let separate_connector = OperationGate::new(
            fixture(journal.clone(), other, false),
            journal.clone(),
            writer,
            4096,
        )?;
        {
            let _first = gate.arm(first, false, 0)?;
            gate.freeze(&serde_json::to_vec(&request("first"))?)?;
            assert!(gate.arm(other, false, 0).is_err());
            assert!(separate_connector.arm(other, false, 0).is_err());
        }
        assert!(gate.arm(second, false, 0).is_err());
        let _other = gate.arm(other, false, 0)?;
        assert!(
            gate.http_post::<_, serde_json::Value>(
                Url::parse("https://mint.example/v1/melt/bolt11")?,
                None,
                &serde_json::json!({})
            )
            .await
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn receive_fee_requires_explicit_authority_before_any_submission() -> Result<()> {
        let dir = crate::test_directory()?;
        let writer = Arc::new(WriterLock::acquire(dir.path())?);
        let journal = Arc::new(Mutex::new(Journal::open(
            &dir.path().join("journal.sqlite"),
            Uuid::new_v4(),
            8,
        )?));
        let id = admit(&mut journal.lock().unwrap(), "fee-input", "mint-a");
        let gate = OperationGate::new(
            fixture(journal.clone(), id, false),
            journal.clone(),
            writer,
            4096,
        )?;
        let mut fee_request = request("fee-input");
        fee_request["outputs"][0]["amount"] = 4.into();
        let bytes = serde_json::to_vec(&fee_request)?;
        {
            let _lease = gate.arm(id, false, 0)?;
            assert!(gate.freeze(&bytes).is_err());
            assert!(
                gate.freeze(&serde_json::to_vec(&request("wrong-input"))?)
                    .is_err()
            );
        }
        assert_eq!(journal.lock().unwrap().state(id)?, "validated");
        assert!(journal.lock().unwrap().prepared_request(id)?.is_none());
        let _lease = gate.arm(id, false, 4)?;
        gate.freeze(&bytes)?;
        assert_eq!(journal.lock().unwrap().state(id)?, "acquiring");
        assert!(journal.lock().unwrap().export_receipt(id).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn configured_gate_requires_a_matching_durable_cdk_saga() -> Result<()> {
        let dir = crate::test_directory()?;
        let writer = Arc::new(WriterLock::acquire(dir.path())?);
        let journal = Arc::new(Mutex::new(Journal::open(
            &dir.path().join("journal.sqlite"),
            Uuid::new_v4(),
            8,
        )?));
        let id = admit(&mut journal.lock().unwrap(), "missing-saga", "mint-a");
        let inner = fixture(journal.clone(), id, false);
        let storage = Arc::new(cdk_sqlite::wallet::memory::empty().await?);
        let gate = OperationGate::new(inner.clone(), journal.clone(), writer, 4096)?
            .with_wallet_database(storage);
        let _lease = gate.arm(id, false, 0)?;
        assert!(
            gate.http_post::<_, serde_json::Value>(
                Url::parse("https://mint.example/v1/swap")?,
                None,
                &request("missing-saga")
            )
            .await
            .is_err()
        );
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
        assert!(journal.lock().unwrap().swap_reply(id)?.is_none());
        assert!(journal.lock().unwrap().export_receipt(id).is_err());
        Ok(())
    }
}
