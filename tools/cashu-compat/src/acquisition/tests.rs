//! Protocol fixture only: synthetic keys/ecash, real CDK wallet and crypto.
use super::*;
use crate::intake::{Limits, nutzap_json, ordinary_json};
use crate::mint_policy::MintRegistry;
use anyhow::bail;
use async_trait::async_trait;
use cdk::{
    Amount, dhke,
    nuts::{
        AuthToken, BlindSignature, CheckStateResponse, CurrencyUnit, Id, KeySet, Keys, MintInfo,
        Nuts, Proof, ProofDleq, RestoreRequest, RestoreResponse, State, SwapRequest, SwapResponse,
    },
    secret::Secret,
    wallet::{CustomHttpClient, WalletBuilder},
};
use cdk_http_client::{
    HttpError, RawResponse,
    ws::{WsError, WsReceiver, WsSender},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use url::Url;

fn refused() -> HttpError {
    HttpError::Other("protocol fixture rejected request".into())
}
fn decode<R: DeserializeOwned>(value: Value) -> Result<R, HttpError> {
    serde_json::from_value(value).map_err(|_| refused())
}

#[derive(Default)]
struct IssuerState {
    spent: BTreeSet<[u8; 33]>,
    signatures: BTreeMap<[u8; 33], BlindSignature>,
    replies: BTreeMap<Vec<u8>, SwapResponse>,
}
#[derive(Clone)]
struct FixtureMint {
    private_keys: Arc<BTreeMap<Amount, SecretKey>>,
    keyset: KeySet,
    state: Arc<Mutex<IssuerState>>,
    lose_reply: Arc<AtomicBool>,
    fail_check_once: Arc<AtomicBool>,
    incomplete_check_once: Arc<AtomicBool>,
    swap_attempts: Arc<AtomicUsize>,
    executions: Arc<AtomicUsize>,
    melt_attempts: Arc<AtomicUsize>,
    melt_paid_reply: Arc<Mutex<Option<Value>>>,
    melt_settled: Arc<AtomicBool>,
    melt_status_unavailable: Arc<AtomicBool>,
    lose_melt_reply: Arc<AtomicBool>,
    return_melt_change: Arc<AtomicBool>,
    consume_melt_reserve: Arc<AtomicBool>,
    quote_attempts: Arc<AtomicUsize>,
    quote_reply: Arc<Mutex<Option<Value>>>,
    // Simulate a bounded/expired response cache while retaining NUT-09 material.
    cache_enabled: Arc<AtomicBool>,
}
impl fmt::Debug for FixtureMint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SyntheticProtocolMint")
    }
}
impl FixtureMint {
    fn new(fee: u64) -> Self {
        let private_keys = Arc::new(
            (0..11)
                .map(|bit| {
                    (
                        Amount::from(1_u64 << bit),
                        SecretKey::from_slice(&[7 + bit as u8; 32]).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        );
        let keys = Keys::new(
            private_keys
                .iter()
                .map(|(amount, key)| (*amount, key.public_key()))
                .collect(),
        );
        let keyset = KeySet {
            id: Id::v1_from_keys(&keys),
            keys,
            unit: CurrencyUnit::Sat,
            active: Some(true),
            input_fee_ppk: fee,
            final_expiry: None,
        };
        Self {
            private_keys,
            keyset,
            state: Arc::new(Mutex::new(IssuerState::default())),
            lose_reply: Arc::new(AtomicBool::new(false)),
            fail_check_once: Arc::new(AtomicBool::new(false)),
            incomplete_check_once: Arc::new(AtomicBool::new(false)),
            swap_attempts: Arc::new(AtomicUsize::new(0)),
            executions: Arc::new(AtomicUsize::new(0)),
            melt_attempts: Arc::new(AtomicUsize::new(0)),
            melt_paid_reply: Default::default(),
            melt_settled: Default::default(),
            melt_status_unavailable: Default::default(),
            lose_melt_reply: Default::default(),
            return_melt_change: Default::default(),
            consume_melt_reserve: Default::default(),
            quote_attempts: Arc::new(AtomicUsize::new(0)),
            quote_reply: Arc::new(Mutex::new(None)),
            cache_enabled: Arc::new(AtomicBool::new(true)),
        }
    }
    fn settle_melt(&self, request: cdk::nuts::MeltRequest<String>) -> Result<Value, HttpError> {
        let mut paid = self
            .melt_paid_reply
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| HttpError::Connection("synthetic melt reply lost".into()))?;
        let original = self
            .quote_reply
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(refused)?;
        if paid["quote"].as_str() != Some(request.quote_id().as_str())
            || paid["quote"] != original["quote"]
            || paid["amount"] != original["amount"]
            || request.inputs().is_empty()
            || request.inputs().len() > 64
            || !self.return_melt_change.load(Ordering::SeqCst)
                && !self.consume_melt_reserve.load(Ordering::SeqCst)
                && request
                    .outputs()
                    .as_ref()
                    .is_some_and(|outputs| !outputs.is_empty())
        {
            return Err(refused());
        }
        let mut state = self.state.lock().unwrap();
        let mut ys = BTreeSet::new();
        let mut total = 0u64;
        for proof in request.inputs() {
            let y = proof.y().map_err(|_| refused())?;
            let key = self.private_keys.get(&proof.amount).ok_or_else(refused)?;
            if proof.keyset_id != self.keyset.id
                || !ys.insert(y.to_bytes())
                || state.spent.contains(&y.to_bytes())
                || dhke::sign_message(key, &y).map_err(|_| refused())? != proof.c
            {
                return Err(refused());
            }
            total = total
                .checked_add(u64::from(proof.amount))
                .ok_or_else(refused)?;
        }
        let fee = (u64::try_from(request.inputs().len()).map_err(|_| refused())?
            * self.keyset.input_fee_ppk)
            .div_ceil(1000);
        let consumed = paid["amount"]
            .as_u64()
            .and_then(|amount| amount.checked_add(fee))
            .ok_or_else(refused)?;
        let consumed = if self.consume_melt_reserve.load(Ordering::SeqCst) {
            if self.return_melt_change.load(Ordering::SeqCst)
                || original["fee_reserve"].as_u64() != Some(1)
                || request.outputs().as_ref().is_none_or(|outputs| {
                    outputs.len() != 1
                        || outputs[0].keyset_id != self.keyset.id
                        || outputs[0].amount != Amount::ZERO
                })
            {
                return Err(refused());
            }
            consumed.checked_add(1).ok_or_else(refused)?
        } else {
            consumed
        };
        if self.return_melt_change.load(Ordering::SeqCst) {
            // This financial fixture returns one sat of unused routing reserve.
            // Sign the actual original blank output and retain NUT-09 material.
            if total.checked_sub(consumed) != Some(1) || original["fee_reserve"].as_u64() != Some(1)
            {
                return Err(refused());
            }
            let outputs = request.outputs().as_ref().ok_or_else(refused)?;
            if outputs.len() != 1 {
                return Err(refused());
            }
            let output = &outputs[0];
            if output.keyset_id != self.keyset.id
                || output.amount != Amount::ZERO
                || state
                    .signatures
                    .contains_key(&output.blinded_secret.to_bytes())
            {
                return Err(refused());
            }
            let key = self
                .private_keys
                .get(&Amount::from(1u64))
                .ok_or_else(refused)?;
            let blinded = dhke::sign_message(key, &output.blinded_secret).map_err(|_| refused())?;
            let signature = BlindSignature::new(
                Amount::from(1u64),
                blinded,
                self.keyset.id,
                &output.blinded_secret,
                key,
            )
            .map_err(|_| refused())?;
            paid["change"] =
                serde_json::to_value(vec![signature.clone()]).map_err(|_| refused())?;
            state
                .signatures
                .insert(output.blinded_secret.to_bytes(), signature);
            *self.melt_paid_reply.lock().unwrap() = Some(paid.clone());
        } else if consumed != total {
            return Err(refused());
        }
        state.spent.extend(ys);
        self.melt_settled.store(true, Ordering::SeqCst);
        if self.lose_melt_reply.load(Ordering::SeqCst) {
            return Err(HttpError::Connection(
                "synthetic paid melt reply lost".into(),
            ));
        }
        Ok(paid)
    }

    fn issue(&self, secret: String) -> Proof {
        self.issue_amount(secret, 8)
    }
    fn issue_amount(&self, secret: String, amount: u64) -> Proof {
        let key = &self.private_keys[&Amount::from(amount)];
        let (b, r) = dhke::blind_message(
            secret.as_bytes(),
            Some(SecretKey::from_slice(&[8; 32]).unwrap()),
        )
        .unwrap();
        let c = dhke::sign_message(key, &b).unwrap();
        let signature =
            BlindSignature::new(Amount::from(amount), c, self.keyset.id, &b, key).unwrap();
        let mut proof = Proof::new(
            Amount::from(amount),
            self.keyset.id,
            Secret::new(secret),
            dhke::unblind_message(&c, &r, &key.public_key()).unwrap(),
        );
        let dleq = signature.dleq.unwrap();
        proof.dleq = Some(ProofDleq::new(dleq.e, dleq.s, r));
        proof
    }
    fn swap(&self, request: SwapRequest) -> Result<Value, HttpError> {
        let bytes = serde_json::to_vec(&request).map_err(|_| refused())?;
        let mut state = self.state.lock().unwrap();
        if let Some(reply) = state.replies.get(&bytes)
            && self.cache_enabled.load(Ordering::SeqCst)
        {
            return serde_json::to_value(reply).map_err(|_| refused());
        }
        let mut ys = BTreeSet::new();
        for proof in request.inputs() {
            let y = proof.y().map_err(|_| refused())?;
            if proof.keyset_id != self.keyset.id
                || !ys.insert(y.to_bytes())
                || state.spent.contains(&y.to_bytes())
            {
                return Err(refused());
            }
            let key = self.private_keys.get(&proof.amount).ok_or_else(refused)?;
            if dhke::sign_message(key, &y).map_err(|_| refused())? != proof.c {
                return Err(HttpError::Status {
                    status: 400,
                    message: "synthetic invalid proof".into(),
                });
            }
            // The fixture also enforces the receiver's actual P2PK witness.
            if proof.secret.as_bytes().first() == Some(&b'[') {
                proof.verify_p2pk().map_err(|_| refused())?;
            }
        }
        let fee = (u64::try_from(request.inputs().len()).map_err(|_| refused())?
            * self.keyset.input_fee_ppk)
            .div_ceil(1000);
        if u64::from(request.input_amount().map_err(|_| refused())?)
            != u64::from(request.output_amount().map_err(|_| refused())?) + fee
        {
            return Err(refused());
        }
        let mut signatures = Vec::new();
        for output in request.outputs() {
            if output.keyset_id != self.keyset.id
                || self.keyset.keys.amount_key(output.amount).is_none()
            {
                return Err(refused());
            }
            let key = self.private_keys.get(&output.amount).ok_or_else(refused)?;
            let c = dhke::sign_message(key, &output.blinded_secret).map_err(|_| refused())?;
            signatures.push(
                BlindSignature::new(
                    output.amount,
                    c,
                    output.keyset_id,
                    &output.blinded_secret,
                    key,
                )
                .map_err(|_| refused())?,
            );
        }
        // Validate the whole request before the synthetic mint's atomic boundary.
        for y in ys {
            state.spent.insert(y);
        }
        for (output, signature) in request.outputs().iter().zip(&signatures) {
            state
                .signatures
                .insert(output.blinded_secret.to_bytes(), signature.clone());
        }
        let reply = SwapResponse { signatures };
        state.replies.insert(bytes, reply.clone());
        self.executions.fetch_add(1, Ordering::SeqCst);
        if self.lose_reply.swap(false, Ordering::SeqCst) {
            return Err(HttpError::Connection(
                "synthetic reply lost after execution".into(),
            ));
        }
        serde_json::to_value(reply).map_err(|_| refused())
    }
}
#[async_trait]
impl Transport for FixtureMint {
    fn with_proxy(
        &mut self,
        _url: Url,
        _matcher: Option<&str>,
        _invalid: bool,
    ) -> Result<(), HttpError> {
        Err(refused())
    }
    async fn ws_connect(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
    ) -> Result<(WsSender, WsReceiver), WsError> {
        Err(WsError::NotSupported("fixture has no network".into()))
    }
    async fn http_get<R: DeserializeOwned>(
        &self,
        url: Url,
        _auth: Option<AuthToken>,
    ) -> Result<R, HttpError> {
        let value = match url.path() {
            "/v1/info" => serde_json::to_value(
                MintInfo::new()
                    .name("synthetic fixture only")
                    .nuts(Nuts::new().nut07(true).nut09(true).nut11(true).nut12(true)),
            )
            .map_err(|_| refused())?,
            "/v1/keysets" => {
                json!({"keysets":[{"id":self.keyset.id,"unit":"sat","active":true,"input_fee_ppk":self.keyset.input_fee_ppk}]})
            }
            path if path.starts_with("/v1/melt/quote/bolt11/") => {
                let paid = self
                    .melt_paid_reply
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or_else(refused)?;
                if self.melt_status_unavailable.load(Ordering::SeqCst)
                    || !self.melt_settled.load(Ordering::SeqCst)
                    || path.rsplit('/').next() != paid["quote"].as_str()
                {
                    return Err(refused());
                }
                paid
            }
            path if path == "/v1/keys" || path.starts_with("/v1/keys/") => {
                json!({"keysets":[self.keyset]})
            }
            _ => return Err(refused()),
        };
        decode(value)
    }
    async fn http_get_raw(
        &self,
        url: Url,
        auth: Option<AuthToken>,
    ) -> Result<RawResponse, HttpError> {
        let value: Value = self.http_get(url, auth).await?;
        Ok(RawResponse::new(
            200,
            serde_json::to_vec(&value).map_err(|_| refused())?,
        ))
    }
    async fn http_post<P: Serialize + Send + Sync, R: DeserializeOwned>(
        &self,
        url: Url,
        _auth: Option<AuthToken>,
        payload: &P,
    ) -> Result<R, HttpError> {
        let payload = serde_json::to_value(payload).map_err(|_| refused())?;
        let value = match url.path() {
            "/v1/swap" => {
                self.swap_attempts.fetch_add(1, Ordering::SeqCst);
                self.swap(decode(payload)?)?
            }
            "/v1/checkstate" => {
                if self.fail_check_once.swap(false, Ordering::SeqCst) {
                    return Err(HttpError::Connection(
                        "synthetic state read unavailable".into(),
                    ));
                }
                let mut request: CheckStateRequest = decode(payload)?;
                if self.incomplete_check_once.swap(false, Ordering::SeqCst) {
                    request.ys.pop();
                }
                let state = self.state.lock().unwrap();
                serde_json::to_value(CheckStateResponse {
                    states: request
                        .ys
                        .into_iter()
                        .map(|y| {
                            let status = if state.spent.contains(&y.to_bytes()) {
                                State::Spent
                            } else {
                                State::Unspent
                            };
                            (y, status).into()
                        })
                        .collect(),
                })
                .map_err(|_| refused())?
            }
            "/v1/melt/quote/bolt11" => {
                self.quote_attempts.fetch_add(1, Ordering::SeqCst);
                self.quote_reply
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or_else(refused)?
            }
            "/v1/melt/bolt11" => {
                self.melt_attempts.fetch_add(1, Ordering::SeqCst);
                self.settle_melt(decode(payload)?)?
            }
            "/v1/restore" => {
                let request: RestoreRequest = decode(payload)?;
                let state = self.state.lock().unwrap();
                let mut outputs = Vec::new();
                let mut signatures = Vec::new();
                for mut output in request.outputs {
                    if let Some(signature) = state.signatures.get(&output.blinded_secret.to_bytes())
                    {
                        output.amount = signature.amount;
                        outputs.push(output);
                        signatures.push(signature.clone());
                    }
                }
                serde_json::to_value(RestoreResponse {
                    outputs,
                    signatures,
                })
                .map_err(|_| refused())?
            }
            _ => return Err(refused()),
        };
        decode(value)
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

struct Harness {
    id: String,
    directory: tempfile::TempDir,
    mint: FixtureMint,
    keys: Arc<PinnedKeys>,
    epoch: Uuid,
    seed: [u8; 64],
    receiver: SecretKey,
}
impl Harness {
    fn new(fee: u64) -> Result<Self> {
        let mint = FixtureMint::new(fee);
        let mut keys = PinnedKeys::default();
        for (amount, key) in mint.keyset.keys.iter() {
            keys.insert("fixture", mint.keyset.id, u64::from(*amount), *key)?;
        }
        Ok(Self {
            id: "fixture".into(),
            directory: crate::test_directory()?,
            mint,
            keys: Arc::new(keys),
            epoch: Uuid::new_v4(),
            seed: [42; 64],
            receiver: SecretKey::from_slice(&[1; 32])?,
        })
    }
    fn policy(&self) -> MintPolicy {
        MintPolicy {
            id: self.id.clone(),
            advertised_url: format!("https://{}.example", self.id),
            policy_version: "fixture-v1".into(),
            receive_cashu: true,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        }
    }
    async fn open(&self) -> Result<Coordinator<'_, FixtureMint>> {
        self.open_with_writer(Arc::new(WriterLock::acquire(self.directory.path())?))
            .await
    }
    async fn open_with_writer(
        &self,
        writer: Arc<WriterLock>,
    ) -> Result<Coordinator<'_, FixtureMint>> {
        let database = crate::wallet::open_wallet_storage(self.directory.path()).await?;
        let journal = Arc::new(Mutex::new(Journal::open(
            &self.directory.path().join("journal.sqlite"),
            self.epoch,
            8,
        )?));
        fixture_coordinator(
            &self.seed,
            self.policy().advertised_url.parse()?,
            self.mint.clone(),
            database,
            journal,
            writer,
            self.keys.clone(),
        )
    }
    fn ordinary(&self, proof: Proof, source: &str) -> Result<ValidatedContribution> {
        let body = serde_json::to_vec(
            &json!({"id":"reusable","mint":self.policy().advertised_url,"unit":"sat","proofs":[proof]}),
        )?;
        ordinary_json(
            &body,
            source,
            &["reusable".into()],
            &MintRegistry::new(vec![self.policy()])?,
            &self.keys,
            &Limits {
                body_bytes: 65536,
                proof_count: 8,
                receipt_sats: 10000,
            },
            self.receiver.public_key(),
        )
    }
    fn nutzap_body(&self, proof: Proof) -> Result<(String, String)> {
        let recipient = SecretKey::from_slice(&[3; 32])?
            .public_key()
            .x_only_public_key()
            .to_string();
        Ok((self.nutzap_body_for(proof, &recipient)?, recipient))
    }
    fn nutzap_body_for(&self, proof: Proof, recipient: &str) -> Result<String> {
        use cdk::secp256k1::{Keypair, Message, Secp256k1};
        use lightning_goats_nostr_event::{SignedNostrEvent, canonical_bytes};
        use sha2::{Digest, Sha256};
        let secp = Secp256k1::new();
        let sender =
            Keypair::from_secret_key(&secp, &cdk::secp256k1::SecretKey::from_slice(&[2; 32])?);
        assert_ne!(
            recipient,
            self.receiver.public_key().x_only_public_key().to_string()
        );
        let mut event = SignedNostrEvent {
            id: String::new(),
            pubkey: sender.x_only_public_key().0.to_string(),
            created_at: 1,
            kind: 9321,
            tags: vec![
                vec!["p".into(), recipient.to_owned()],
                vec!["u".into(), self.policy().advertised_url],
                vec!["unit".into(), "sat".into()],
                vec!["proof".into(), serde_json::to_string(&proof)?],
            ],
            content: String::new(),
            sig: String::new(),
        };
        let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event)?).into();
        event.id = cdk::util::hex::encode(hash);
        event.sig = secp
            .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &sender)
            .to_string();
        Ok(serde_json::to_string(&event)?)
    }
    fn locked_proof(&self) -> Proof {
        let key = self
            .receiver
            .public_key()
            .x_only_public_key()
            .public_key(cdk::secp256k1::Parity::Even);
        self.mint
            .issue(json!(["P2PK",{"nonce":"fixture-nonce","data":key.to_string()}]).to_string())
    }
}

fn fixture_coordinator(
    seed: &[u8; 64],
    mint_url: MintUrl,
    mint: FixtureMint,
    database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
    journal: Arc<Mutex<Journal>>,
    writer: Arc<WriterLock>,
    keys: Arc<PinnedKeys>,
) -> Result<Coordinator<'_, FixtureMint>> {
    let gate = OperationGate::new(mint, journal.clone(), writer, 65536)?
        .with_wallet_database(database.clone())
        .with_key_policy("fixture".into(), keys.clone());
    let client = CustomHttpClient::with_transport(mint_url.clone(), gate.clone(), None);
    let wallet = WalletBuilder::new()
        .mint_url(mint_url.clone())
        .unit(CurrencyUnit::Sat)
        .localstore(database.clone())
        .seed(*seed)
        .client(client)
        .without_rate_limiting()
        .use_http_subscription()
        .build()?;
    Ok(Coordinator {
        wallet,
        database,
        gate,
        journal,
        keys,
        seed,
        mint_id: "fixture".into(),
        mint_url,
        permissions: Permissions {
            admission: true,
            acquisition: true,
            receipt_export: true,
            original_swap_replay: true,
        },
        maximum_receive_fee_sats: 1,
    })
}
fn new_id(admission: Admission) -> Uuid {
    match admission {
        Admission::New(id) => id,
        _ => panic!("new acquisition expected"),
    }
}
fn receipt(progress: Progress) -> Box<Receipt> {
    match progress {
        Progress::Receipt(receipt) => receipt,
        _ => panic!("verified acquired receipt expected"),
    }
}

#[tokio::test]
async fn rejected_bearer_proof_does_not_block_later_owned_receives_after_restart() -> Result<()> {
    let harness = Harness::new(0)?;
    let mut forged = harness.mint.issue("original-fixture-secret".into());
    forged.secret = Secret::new("unissued-forged-secret");
    forged.dleq = None;
    let bad = harness.ordinary(forged, "bad-delivery")?;
    let bad_id = {
        let mut coordinator = harness.open().await?;
        let id = new_id(coordinator.admit(&bad)?);
        assert!(matches!(
            coordinator.acquire_new(id, &harness.receiver).await?,
            Progress::Pending(_)
        ));
        assert!(
            coordinator
                .database
                .get_incomplete_sagas()
                .await?
                .is_empty()
        );
        assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 0);
        id
    };
    let mut coordinator = harness.open().await?;
    let old = coordinator.journal.lock().unwrap().derivation(bad_id)?;
    let valid = harness.ordinary(
        harness.mint.issue("later-valid-secret".into()),
        "good-delivery",
    )?;
    let good_id = new_id(coordinator.admit(&valid)?);
    assert!(coordinator.gate.arm(good_id, false, 1).is_err());
    // Simulate a wallet-only rollback behind the retained application journal.
    let counter = coordinator
        .database
        .increment_keyset_counter(&old.keyset_id, 0)
        .await?;
    let database = rusqlite::Connection::open(harness.directory.path().join("wallet.sqlite"))?;
    assert_eq!(
        database.execute(
            "UPDATE keyset_counter SET counter=0 WHERE keyset_id=?",
            [old.keyset_id.to_string()]
        )?,
        1
    );
    assert!(
        coordinator
            .gate
            .arm_new_acquisition(good_id, 1)
            .await
            .is_err()
    );
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 0);
    database.execute(
        "UPDATE keyset_counter SET counter=? WHERE keyset_id=?",
        rusqlite::params![counter, old.keyset_id.to_string()],
    )?;
    drop(database);
    let received = receipt(coordinator.acquire_new(good_id, &harness.receiver).await?);
    assert_eq!(received.net_sats, 8);
    let mut journal = coordinator.journal.lock().unwrap();
    assert_eq!(journal.state(bad_id)?, "acquiring");
    assert!(journal.derivation(bad_id)? == old);
    assert!(journal.export_receipt(bad_id).is_err());
    assert!(journal.derivation(good_id)?.counter_start >= old.counter_end);
    assert!(
        journal
            .record_derivation(good_id, &old)
            .unwrap_err()
            .to_string()
            .contains("overlaps retained recovery")
    );
    drop(journal);
    assert_eq!(coordinator.admit(&bad)?, Admission::Duplicate(bad_id));
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn missing_recovery_material_still_fences_new_mint_work() -> Result<()> {
    let harness = Harness::new(0)?;
    let coordinator = harness.open().await?;
    let first =
        new_id(coordinator.admit(
            &harness.ordinary(harness.mint.issue("missing-first".into()), "missing-first")?,
        )?);
    let second = new_id(coordinator.admit(&harness.ordinary(
        harness.mint.issue("missing-second".into()),
        "missing-second",
    )?)?);
    {
        let mut journal = coordinator.journal.lock().unwrap();
        journal.prepare(first, b"retained original")?;
        journal.start_acquisition(first)?;
    }
    assert!(
        coordinator
            .gate
            .arm_new_acquisition(second, 1)
            .await
            .is_err()
    );
    // A refused arm drops its lease rather than wedging the process coordinator.
    let _original = coordinator.gate.arm(first, true, 1)?;
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn ordinary_and_nutzap_use_real_cdk_and_one_reusable_descriptor() -> Result<()> {
    let harness = Harness::new(0)?;
    let mut coordinator = harness.open().await?;
    let plain = harness.ordinary(
        harness.mint.issue("first-private-proof".into()),
        "http-first",
    )?;
    let first = new_id(coordinator.admit(&plain)?);
    assert_eq!(
        receipt(coordinator.acquire_new(first, &harness.receiver).await?).net_sats,
        8
    );
    let config = crate::relay_intake::tests::config(harness.directory.path())?;
    let intent = crate::publication::Intent::announcement(&config, 1)?;
    let raw = crate::publication::tests::sign(intent.unsigned(None)?)?;
    let info = intent.verify_signed(None, &raw)?;
    let key = info.singleton_tag("pubkey", true)?.unwrap();
    // Quartz 2.13.0 copies the advertised key into the proof secret verbatim.
    let locked = harness.mint.issue(
        json!(["P2PK",{"nonce":"00000000000000000000000000000000","data":key,"tags":[]}])
            .to_string(),
    );
    let sent = harness.nutzap_body_for(locked.clone(), &info.event().pubkey)?;
    let zap = nutzap_json(
        &sent,
        &info.event().pubkey,
        &MintRegistry::new(vec![harness.policy()])?,
        &harness.keys,
        &Limits {
            body_bytes: 65536,
            proof_count: 8,
            receipt_sats: 10000,
        },
        harness.receiver.public_key(),
    )?;
    let second = new_id(coordinator.admit(&zap)?);
    let zap_receipt = receipt(coordinator.acquire_new(second, &harness.receiver).await?);
    assert_eq!(zap_receipt.net_sats, 8);
    assert_eq!(zap_receipt.source_author.as_deref(), zap.author());
    let replay = harness.ordinary(locked, "http-rewrapped")?;
    assert_eq!(coordinator.admit(&replay)?, Admission::Duplicate(second));
    assert_eq!(
        receipt(coordinator.acquire_new(second, &harness.receiver).await?).acquisition_id,
        second
    );
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn rotated_receivers_redeem_original_queued_and_in_window_proofs() -> Result<()> {
    use crate::{
        descriptor::{Descriptor, Encoding, RetiredDescriptor, RetiredMint},
        intake::{DescriptorAcceptance, NutzapReceivers, nutzap_json_at, ordinary_json_at},
        service_config::{NutzapWindow, RetainedReceiver},
        service_keys::Keys,
    };
    use std::os::unix::fs::PermissionsExt;
    let harness = Harness::new(0)?;
    let old = harness.receiver.public_key();
    let current = SecretKey::from_slice(&[8; 32])?.public_key();
    for (name, bytes) in [
        ("wallet-seed.bin".into(), harness.seed.to_vec()),
        ("receiving-key.bin".into(), vec![1; 32]),
        (
            format!("receiving-key-{}.bin", current.to_hex()),
            vec![8; 32],
        ),
    ] {
        let path = harness.directory.path().join(name);
        std::fs::write(&path, bytes)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let incoming = |key: PublicKey, nonce| {
        let even = key
            .x_only_public_key()
            .public_key(cdk::secp256k1::Parity::Even);
        harness
            .mint
            .issue(json!(["P2PK",{"nonce":nonce,"data":even.to_string()}]).to_string())
    };
    let queued = {
        let first = harness.open().await?;
        new_id(first.admit(&harness.ordinary(incoming(old, "queued"), "queued-before-rotation")?)?)
    };
    let keys = Keys::load_configured(harness.directory.path(), current, old, &[old])?;
    let mut reopened = harness.open().await?;
    let mint = harness.policy();
    let registry = MintRegistry::new(vec![mint.clone()])?;
    let endpoint = "https://lightning-goats.example/api/v1/cashu";
    let old_descriptor = Descriptor::prepare(&registry, endpoint, old, Encoding::CreqA)?;
    let retired = [RetiredDescriptor {
        id: old_descriptor.id().into(),
        endpoint: endpoint.into(),
        receiving_key: Some(old.to_hex()),
        retired_at: 100,
        accept_before: 200,
        mints: vec![RetiredMint {
            id: mint.id.clone(),
            advertised_url: mint.advertised_url.clone(),
            policy_version: mint.policy_version.clone(),
        }],
    }];
    let retained = [RetainedReceiver {
        public_key: old.to_hex(),
        nutzap: Some(NutzapWindow {
            starts_at: 100,
            accept_before: 200,
            mint_ids: BTreeSet::from([mint.id.clone()]),
        }),
    }];
    let limits = Limits {
        body_bytes: 65536,
        proof_count: 8,
        receipt_sats: 10000,
    };
    let (zap, recipient) = harness.nutzap_body(incoming(old, "delayed-zap"))?;
    let decode_zap = |now| {
        nutzap_json_at(
            &zap,
            &recipient,
            &registry,
            &harness.keys,
            &limits,
            NutzapReceivers {
                current,
                retained: &retained,
                now,
            },
        )
    };
    let payload = serde_json::to_vec(&json!({
        "id":old_descriptor.id(),"mint":mint.advertised_url,"unit":"sat","proofs":[incoming(old,"old-qr")]
    }))?;
    let decode_qr = |now| {
        ordinary_json_at(
            &payload,
            "old-qr-source",
            DescriptorAcceptance {
                active: &[],
                retired: &retired,
                now,
            },
            &registry,
            &harness.keys,
            &limits,
            current,
        )
    };
    assert!(decode_zap(200).is_err() && decode_qr(200).is_err());
    let late_zap = new_id(reopened.admit(&decode_zap(199)?)?);
    let old_qr = new_id(reopened.admit(&decode_qr(199)?)?);
    for id in [queued, late_zap, old_qr] {
        let verified = receipt(
            reopened
                .acquire_with_receivers(id, |lock| keys.signing_key(lock))
                .await?,
        );
        assert_eq!(verified.acquisition_id, id);
        assert_eq!(verified.net_sats, 8);
    }
    assert_eq!(harness.mint.swap_attempts.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn lost_mint_reply_restarts_and_restores_without_a_second_execution() -> Result<()> {
    let harness = Harness::new(0)?;
    harness.mint.lose_reply.store(true, Ordering::SeqCst);
    let id = {
        let mut coordinator = harness.open().await?;
        let id = new_id(
            coordinator
                .admit(&harness.ordinary(harness.mint.issue("lost-reply".into()), "http-lost")?)?,
        );
        assert!(matches!(
            coordinator.acquire_new(id, &harness.receiver).await?,
            Progress::Pending(_)
        ));
        id
    };
    harness.mint.cache_enabled.store(false, Ordering::SeqCst);
    let mut coordinator = harness.open().await?;
    let waiting = new_id(coordinator.admit(&harness.ordinary(
        harness.mint.issue("waiting-for-ambiguous".into()),
        "waiting",
    )?)?);
    assert!(
        coordinator
            .gate
            .arm_new_acquisition(waiting, 1)
            .await
            .is_err()
    );
    assert!(matches!(
        coordinator.acquire_new(waiting, &harness.receiver).await?,
        Progress::Pending(_)
    ));
    let recovered = receipt(coordinator.recover_original(id).await?);
    assert_eq!(recovered.net_sats, 8);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    assert_eq!(
        receipt(coordinator.recover_original(id).await?).acquisition_id,
        id
    );
    assert_eq!(
        receipt(coordinator.acquire_new(waiting, &harness.receiver).await?).net_sats,
        8
    );
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn receive_fees_are_conserved_and_export_toggle_preserves_acquired_funds() -> Result<()> {
    let harness = Harness::new(125)?;
    let mut coordinator = harness.open().await?;
    coordinator.permissions.receipt_export = false;
    let id = new_id(
        coordinator
            .admit(&harness.ordinary(harness.mint.issue("fee-bearing".into()), "http-fee")?)?,
    );
    assert!(matches!(
        coordinator.acquire_new(id, &harness.receiver).await?,
        Progress::Acquired(_)
    ));
    coordinator.permissions.receipt_export = true;
    let acquired = receipt(coordinator.reconcile_owned(id).await?);
    assert_eq!(acquired.gross_sats, 8);
    assert_eq!(acquired.net_sats, 7);
    assert_eq!(acquired.receive_fee_sats, 1);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn completed_transaction_without_owned_outputs_never_exports_credit() -> Result<()> {
    let harness = Harness::new(0)?;
    harness.mint.fail_check_once.store(true, Ordering::SeqCst);
    let mut coordinator = harness.open().await?;
    let id =
        new_id(coordinator.admit(
            &harness.ordinary(harness.mint.issue("missing-owned".into()), "http-missing")?,
        )?);
    assert!(matches!(
        coordinator.acquire_new(id, &harness.receiver).await?,
        Progress::Pending(_)
    ));
    // Fault injection into disposable wallet state, never a production repair action.
    let owned = coordinator
        .database
        .get_proofs(
            Some(coordinator.mint_url.clone()),
            Some(CurrencyUnit::Sat),
            None,
            None,
        )
        .await?;
    assert!(!owned.is_empty());
    coordinator
        .database
        .update_proofs(vec![], owned.into_iter().map(|proof| proof.y).collect())
        .await?;
    assert!(matches!(
        coordinator.recover_original(id).await?,
        Progress::Pending(_)
    ));
    assert!(
        coordinator
            .journal
            .lock()
            .unwrap()
            .export_receipt(id)
            .is_err()
    );
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn reviewed_hold_rechecks_original_outputs_without_resubmission_or_export() -> Result<()> {
    let harness = Harness::new(125)?;
    let (id, request) = {
        let mut coordinator = harness.open().await?;
        let id = new_id(coordinator.admit(&harness.ordinary(
            harness.mint.issue("held-ownership".into()),
            "http-hold-recheck",
        )?)?);
        harness
            .mint
            .incomplete_check_once
            .store(true, Ordering::SeqCst);
        assert!(matches!(
            coordinator.acquire_new(id, &harness.receiver).await?,
            Progress::Held(held) if held == id
        ));
        let request = coordinator
            .journal
            .lock()
            .unwrap()
            .prepared_request(id)?
            .unwrap();
        (id, request)
    };
    let mut coordinator = harness.open().await?;
    assert!(matches!(coordinator.recover_original(id).await?, Progress::Held(held) if held == id));
    harness.mint.fail_check_once.store(true, Ordering::SeqCst);
    assert!(
        matches!(coordinator.recheck_held_ownership(id).await?, Progress::Held(held) if held == id)
    );
    harness
        .mint
        .incomplete_check_once
        .store(true, Ordering::SeqCst);
    assert!(
        matches!(coordinator.recheck_held_ownership(id).await?, Progress::Held(held) if held == id)
    );
    {
        let stored = coordinator.journal.lock().unwrap();
        assert_eq!(stored.state(id)?, "held_for_reconciliation");
        assert!(stored.has_recheckable_ownership_hold(id)?);
    }

    for _ in 0..2 {
        assert!(
            matches!(coordinator.recheck_held_ownership(id).await?, Progress::Acquired(acquired) if acquired == id)
        );
    }
    {
        let stored = coordinator.journal.lock().unwrap();
        assert_eq!(stored.state(id)?, "acquired");
        assert!(stored.has_recheckable_ownership_hold(id)?);
        assert_eq!(stored.prepared_request(id)?, Some(request));
    }
    assert_eq!(harness.mint.swap_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
    let exported = receipt(coordinator.reconcile_owned(id).await?);
    assert_eq!(exported.acquisition_id, id);
    assert_eq!(exported.net_sats, 7);
    assert_eq!(exported.receive_fee_sats, 1);
    assert!(coordinator.recheck_held_ownership(id).await.is_err());
    Ok(())
}

#[tokio::test]
async fn prepared_treasury_melt_binds_real_acquired_proofs_without_submission() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::PreparedOnly).await
}

#[tokio::test]
async fn real_cdk_melt_transport_commits_once_before_a_lost_reply() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::Submit).await
}

#[tokio::test]
async fn lost_treasury_quote_reply_retains_one_attempt_across_restart() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::LostQuoteReply).await
}

#[tokio::test]
async fn invalid_treasury_quote_terms_are_retained_without_melt_authority() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::InvalidQuoteTerms).await
}

#[tokio::test]
async fn retained_treasury_quote_recovers_after_binding_failure_without_network() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::RetainedQuote).await
}

#[tokio::test]
async fn treasury_executor_preserves_one_preparation_and_submission_across_restart() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::Executor).await
}

#[tokio::test]
async fn treasury_executor_missing_proofs_remains_held_without_another_preparation() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::ExecutorMissingProof).await
}

#[tokio::test]
async fn treasury_executor_resumes_only_original_unsubmitted_preparation() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::ExecutorPrepared).await
}

#[tokio::test]
async fn treasury_executor_refuses_proofs_owned_by_another_operation() -> Result<()> {
    prepared_treasury_case(TreasuryScenario::ExecutorClaimedProof).await
}

#[tokio::test]
async fn treasury_executor_recovers_original_cdk_preparation_after_journal_bind_failure()
-> Result<()> {
    prepared_treasury_case(TreasuryScenario::ExecutorUnbound).await
}

#[tokio::test]
async fn treasury_worker_progresses_acquired_funds_and_never_resubmits_after_timeout() -> Result<()>
{
    prepared_treasury_case(TreasuryScenario::Scheduled).await
}

#[derive(Clone, Copy)]
enum TreasuryScenario {
    PreparedOnly,
    Submit,
    LostQuoteReply,
    InvalidQuoteTerms,
    RetainedQuote,
    Executor,
    ExecutorMissingProof,
    ExecutorClaimedProof,
    ExecutorPrepared,
    ExecutorUnbound,
    Scheduled,
}

async fn prepared_treasury_case(scenario: TreasuryScenario) -> Result<()> {
    let real_transport = !matches!(scenario, TreasuryScenario::PreparedOnly);
    let harness = Harness::new(0)?;
    let mut coordinator = harness.open().await?;
    let contribution = harness.ordinary(
        harness.mint.issue("treasury-owned-source".into()),
        "treasury-source",
    )?;
    let acquisition = new_id(coordinator.admit(&contribution)?);
    let acquired = receipt(
        coordinator
            .acquire_new(acquisition, &harness.receiver)
            .await?,
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let (mut binding, mut issued) = crate::treasury::tests::fixture();
    binding.wallet_epoch = harness.epoch;
    binding.mint_id = harness.policy().id;
    issued.intent.wallet_epoch = binding.wallet_epoch;
    issued.intent.mint_id = binding.mint_id.clone();
    issued.intent.input_sats = 8;
    issued.intent.deposit_sats = 7;
    issued.intent.sources[0].acquisition_id = acquisition;
    issued.intent.sources[0].input_sats = 8;
    issued.intent.sources[0].receipt_digest = lightning_goats_cashu_receipt::treasury::hex_digest(
        crate::receipt_wire::from_private(binding.wallet_instance, &acquired)?.digest()?,
    );
    issued.invoice = crate::treasury::tests::signed_invoice_hash(
        cdk::lightning_invoice::Currency::Bitcoin,
        7000,
        now,
        300,
        "ab",
    );
    let proofs = coordinator.wallet.get_unspent_proofs().await?;
    let mut ys = proofs
        .iter()
        .map(|p| p.y().map(|y| y.to_hex()))
        .collect::<Result<Vec<_>, _>>()?;
    ys.sort();
    let quote = cdk::wallet::MeltQuote {
        id: "prepared-original-quote".into(),
        mint_url: Some(coordinator.mint_url.clone()),
        unit: CurrencyUnit::Sat,
        amount: 7u64.into(),
        request: issued.invoice.clone(),
        fee_reserve: 1u64.into(),
        state: cdk::nuts::MeltQuoteState::Unpaid,
        expiry: now + 250,
        payment_proof: None,
        estimated_blocks: None,
        fee_index: None,
        payment_method: cdk::nuts::PaymentMethod::BOLT11,
        used_by_operation: None,
        version: 0,
    };
    {
        let mut journal = coordinator.journal.lock().unwrap();
        journal.bind_treasury_instruction(&binding, &issued, now)?;
        if !matches!(scenario, TreasuryScenario::Scheduled) {
            journal.reserve_treasury_sources(&binding, issued.intent.id, now)?;
            let selected = journal.select_treasury_proofs(&binding, issued.intent.id, now)?;
            assert_eq!(selected, BTreeMap::from([(acquisition, ys)]));
        }
        if !real_transport {
            journal.bind_treasury_quote(
                &binding,
                &harness.policy(),
                issued.intent.id,
                &quote,
                now,
            )?;
        }
    }
    if !real_transport {
        coordinator.database.add_melt_quote(quote.clone()).await?;
    }
    let treasury_gate = coordinator.gate.clone().with_treasury_policy(Arc::new(
        crate::operation_gate::TreasuryPolicy {
            binding: binding.clone(),
            mint: harness.policy(),
            seed: harness.seed,
        },
    ));
    if matches!(
        scenario,
        TreasuryScenario::Executor
            | TreasuryScenario::ExecutorMissingProof
            | TreasuryScenario::ExecutorClaimedProof
            | TreasuryScenario::ExecutorPrepared
            | TreasuryScenario::ExecutorUnbound
            | TreasuryScenario::Scheduled
    ) {
        *harness.mint.quote_reply.lock().unwrap() = Some(json!({
            "quote": quote.id, "amount": 7, "fee_reserve": 1,
            "state": "UNPAID", "expiry": quote.expiry,
            "payment_preimage": null, "change": null,
            "request": issued.invoice, "unit": "sat", "method": "bolt11"
        }));
        drop(treasury_gate);
        drop(coordinator);
        let path = harness.directory.path().join("journal.sqlite");
        let writer = Arc::new(WriterLock::acquire(harness.directory.path())?);
        let database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
        let journal = Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?));
        let policy = Arc::new(crate::operation_gate::TreasuryPolicy {
            binding: binding.clone(),
            mint: harness.policy(),
            seed: harness.seed,
        });
        let executor = crate::treasury_execution::Executor::new(
            harness.mint.clone(),
            policy.clone(),
            database.clone(),
            journal,
            writer.clone(),
            harness.keys.clone(),
            65536,
        )?;
        if matches!(scenario, TreasuryScenario::Scheduled) {
            use crate::treasury_scheduler::{Execution, Tick, Worker};
            drop(executor);
            let make_worker = |execution| {
                Worker::from_parts(
                    Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?)),
                    database.clone(),
                    writer.clone(),
                    harness.keys.clone(),
                    vec![(policy.clone(), harness.mint.clone())],
                    execution,
                    std::time::Duration::from_secs(2),
                    65536,
                )
            };
            let execution = Execution {
                split: Default::default(),
                execute: true,
                reconcile: true,
                maximum_attempts: 2,
                maximum_input_sats: 8,
                maximum_fee_sats: 1,
                operations_per_24h: 1,
                input_sats_per_24h: 8,
                maximum_in_flight_sats: 8,
            };
            let worker = make_worker(execution.clone())?;
            for offset in 0..3 {
                assert_eq!(worker.tick(now as i64 + offset).await?, Tick::Progress);
            }
            assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
            drop(worker);
            let mut paused = execution.clone();
            paused.execute = false;
            let worker = make_worker(paused.clone())?;
            assert_eq!(worker.tick(now as i64 + 3).await?, Tick::Idle);
            drop(worker);
            let worker = make_worker(execution)?;
            assert_eq!(worker.tick(now as i64 + 4).await?, Tick::RetryRetained);
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
            drop(worker);
            let worker = make_worker(paused)?;
            assert_eq!(worker.tick(now as i64 + 5).await?, Tick::RetryRetained);
            assert_eq!(worker.tick(now as i64 + 6).await?, Tick::Idle);
            assert_eq!(worker.tick(now as i64 + 12).await?, Tick::RetryRetained);
            assert_eq!(worker.tick(now as i64 + 100).await?, Tick::Idle);
            assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
            let db = rusqlite::Connection::open(&path)?;
            let (state, admissions, charged): (String,u64,u64) = db.query_row(
                "SELECT (SELECT state FROM treasury_instructions WHERE id=?),COUNT(*),COALESCE(SUM(input_sats),0) FROM treasury_execution_admissions",
                [issued.intent.id.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            assert_eq!(state, "submitted");
            assert_eq!((admissions, charged), (1, 8));
            return Ok(());
        }
        let actual = executor.quote_reserved(issued.intent.id).await?;
        assert_eq!(actual.id, quote.id);
        let missing = matches!(scenario, TreasuryScenario::ExecutorMissingProof);
        if missing {
            // Isolated fault: custody journal remains intact but CDK lost its outputs.
            database
                .update_proofs(
                    vec![],
                    proofs
                        .iter()
                        .map(|p| p.y())
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .await?;
        }
        let claimed = matches!(scenario, TreasuryScenario::ExecutorClaimedProof);
        if claimed {
            let ys = proofs
                .iter()
                .map(|p| p.y())
                .collect::<Result<Vec<_>, _>>()?;
            let mut rows = database.get_proofs_by_ys(ys).await?;
            assert!(!rows.is_empty());
            // Inconsistent custody metadata must fail before CDK adopts it.
            rows[0].used_by_operation = Some(Uuid::from_u128(999));
            database.update_proofs(rows, vec![]).await?;
        }
        let unavailable = missing || claimed;
        let expected_melts = usize::from(!unavailable);
        let unbound = matches!(scenario, TreasuryScenario::ExecutorUnbound);
        let prepared_operation = if unbound {
            assert!(
                executor
                    .recover_preparation(issued.intent.id)
                    .await
                    .is_err()
            );
            let db = rusqlite::Connection::open(&path)?;
            db.execute_batch("CREATE TRIGGER refuse_preparation BEFORE INSERT ON treasury_prepared BEGIN SELECT RAISE(ABORT, 'fixture interrupted binding'); END;")?;
            assert!(executor.prepare_reserved(issued.intent.id).await.is_err());
            db.execute_batch("DROP TRIGGER refuse_preparation;")?;
            let count: i64 =
                db.query_row("SELECT COUNT(*) FROM treasury_prepared", [], |r| r.get(0))?;
            assert_eq!(count, 0);
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
            let retained = database.get_melt_quote(&quote.id).await?.unwrap();
            Some(retained.used_by_operation.unwrap().parse::<Uuid>()?)
        } else if matches!(scenario, TreasuryScenario::ExecutorPrepared) {
            let operation = executor.prepare_reserved(issued.intent.id).await?;
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
            Some(operation)
        } else {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                executor.submit_reserved(issued.intent.id),
            )
            .await;
            assert!(!matches!(result, Ok(Ok(()))));
            None
        };
        assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            harness.mint.melt_attempts.load(Ordering::SeqCst),
            if prepared_operation.is_some() {
                0
            } else {
                expected_melts
            }
        );
        drop(executor);
        drop(database);
        // Worker/store restart within one process keeps its process-level lock.
        // CDK may retain connector references while canceled work unwinds.
        let database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
        let journal = Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?));
        let executor = crate::treasury_execution::Executor::new(
            harness.mint.clone(),
            policy,
            database.clone(),
            journal,
            writer,
            harness.keys.clone(),
            65536,
        )?;
        if let Some(operation) = prepared_operation {
            if unbound {
                use cdk::wallet::types::{MeltSagaState, WalletSagaState};
                // A later-stage saga is not evidence of an unsubmitted preparation.
                let mut saga = database.get_saga(&operation).await?.unwrap();
                saga.update_state(WalletSagaState::Melt(MeltSagaState::MeltRequested));
                assert!(database.update_saga(saga).await?);
                assert!(
                    executor
                        .recover_preparation(issued.intent.id)
                        .await
                        .is_err()
                );
                let mut saga = database.get_saga(&operation).await?.unwrap();
                saga.update_state(WalletSagaState::Melt(MeltSagaState::ProofsReserved));
                assert!(database.update_saga(saga).await?);
                let mut changed = database.get_melt_quote(&quote.id).await?.unwrap();
                changed.used_by_operation = Some(Uuid::from_u128(777).to_string());
                database.add_melt_quote(changed).await?;
                assert!(
                    executor
                        .recover_preparation(issued.intent.id)
                        .await
                        .is_err()
                );
                let mut restored = database.get_melt_quote(&quote.id).await?.unwrap();
                restored.used_by_operation = Some(operation.to_string());
                database.add_melt_quote(restored).await?;
                // Crash after reserving the quote and proofs, before saga creation.
                database.delete_saga(&operation).await?;
                // Completed history does not impose a lifetime recovery limit.
                for index in 0..1025 {
                    let mut historical = database.get_melt_quote(&quote.id).await?.unwrap();
                    historical.id = format!("historical-{index}");
                    historical.version = 0;
                    historical.used_by_operation = None;
                    historical.state = cdk::nuts::MeltQuoteState::Paid;
                    database.add_melt_quote(historical).await?;
                }
                let mut unrelated = database.get_melt_quote(&quote.id).await?.unwrap();
                unrelated.id = "unrelated-reservation".into();
                unrelated.version = 0;
                database.add_melt_quote(unrelated).await?;
                assert!(
                    executor
                        .recover_preparation(issued.intent.id)
                        .await
                        .is_err()
                );
                assert!(database.get_saga(&operation).await?.is_none());
                database.remove_melt_quote("unrelated-reservation").await?;
                let original_rows = database.get_reserved_proofs(&operation).await?;
                let mut changed_rows = original_rows.clone();
                changed_rows[0].proof.amount = 99u64.into();
                database.update_proofs(changed_rows, vec![]).await?;
                assert!(
                    executor
                        .recover_preparation(issued.intent.id)
                        .await
                        .is_err()
                );
                assert!(database.get_saga(&operation).await?.is_none());
                database.update_proofs(original_rows, vec![]).await?;
                let fault = rusqlite::Connection::open(&path)?;
                fault.execute_batch("CREATE TRIGGER refuse_recovered_binding BEFORE INSERT ON treasury_prepared BEGIN SELECT RAISE(ABORT, 'fixture recovered saga checkpoint'); END;")?;
                assert!(
                    executor
                        .recover_preparation(issued.intent.id)
                        .await
                        .is_err()
                );
                assert_eq!(database.get_saga(&operation).await?.unwrap().id, operation);
                fault.execute_batch("DROP TRIGGER refuse_recovered_binding;")?;
                assert_eq!(
                    executor.recover_preparation(issued.intent.id).await?,
                    operation
                );
                assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
                assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
                assert!(executor.prepare_reserved(issued.intent.id).await.is_err());
            }
            let mut changed = database.get_melt_quote(&quote.id).await?.unwrap();
            changed.used_by_operation = Some(Uuid::new_v4().to_string());
            database.add_melt_quote(changed).await?;
            assert!(executor.resume_prepared(issued.intent.id).await.is_err());
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
            let mut restored = database.get_melt_quote(&quote.id).await?.unwrap();
            restored.used_by_operation = Some(operation.to_string());
            database.add_melt_quote(restored).await?;
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                executor.resume_prepared(issued.intent.id),
            )
            .await;
            assert!(!matches!(result, Ok(Ok(()))));
            let db = rusqlite::Connection::open(&path)?;
            let retained: String = db.query_row(
                "SELECT operation_id FROM treasury_prepared WHERE intent_id=?",
                [issued.intent.id.to_string()],
                |r| r.get(0),
            )?;
            assert_eq!(retained, operation.to_string());
            assert!(executor.resume_prepared(issued.intent.id).await.is_err());
        }
        assert!(executor.submit_reserved(issued.intent.id).await.is_err());
        assert_eq!(
            harness.mint.melt_attempts.load(Ordering::SeqCst),
            expected_melts
        );
        let db = rusqlite::Connection::open(path)?;
        let claims: i64 = db.query_row(
            "SELECT COUNT(*) FROM treasury_preparation_claims",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(claims, 1);
        let state: String = db.query_row(
            "SELECT state FROM treasury_instructions WHERE id=?",
            [issued.intent.id.to_string()],
            |r| r.get(0),
        )?;
        assert_eq!(state, if unavailable { "bound" } else { "submitted" });
        return Ok(());
    }
    if real_transport {
        coordinator
            .wallet
            .set_client(Arc::new(CustomHttpClient::with_transport(
                coordinator.mint_url.clone(),
                treasury_gate.clone(),
                None,
            )));
        let request = cdk::nuts::MeltQuoteBolt11Request {
            request: issued.invoice.parse()?,
            unit: CurrencyUnit::Sat,
            options: None,
        };
        let destination = coordinator.mint_url.join("v1/melt/quote/bolt11")?;
        let unarmed: Result<Value, _> = treasury_gate
            .http_post(destination.clone(), None, &request)
            .await;
        assert!(unarmed.is_err());
        let lease = treasury_gate.arm_treasury(issued.intent.id)?;
        let mut altered = request.clone();
        altered.unit = CurrencyUnit::Msat;
        let wrong_unit: Result<Value, _> = treasury_gate
            .http_post(destination.clone(), None, &altered)
            .await;
        assert!(wrong_unit.is_err());
        assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 0);
        *harness.mint.quote_reply.lock().unwrap() = Some(json!({
            "quote": quote.id,
            "amount": 7,
            "fee_reserve": 1,
            "state": "UNPAID",
            "expiry": quote.expiry,
            "payment_preimage": null,
            "change": null,
            "request": issued.invoice,
            "unit": "sat",
            "method": "bolt11"
        }));
        if matches!(scenario, TreasuryScenario::RetainedQuote) {
            let path = harness.directory.path().join("journal.sqlite");
            let fault = rusqlite::Connection::open(&path)?;
            fault.execute_batch("CREATE TRIGGER abort_quote_binding BEFORE INSERT ON treasury_quotes BEGIN SELECT RAISE(ABORT, 'fixture crash boundary'); END;")?;
            let failed: Result<Value, _> =
                treasury_gate.http_post(destination, None, &request).await;
            assert!(failed.is_err());
            assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
            assert!(
                coordinator
                    .database
                    .get_melt_quote(&quote.id)
                    .await?
                    .is_none()
            );
            fault.execute_batch("DROP TRIGGER abort_quote_binding;")?;
            drop(fault);
            drop(lease);
            drop(treasury_gate);
            drop(coordinator);
            let writer = Arc::new(WriterLock::acquire(harness.directory.path())?);
            let database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
            let journal = Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?));
            let recovery = crate::treasury_recovery::Recovery {
                transport: &harness.mint,
                journal,
                database: database.clone(),
                writer: writer.clone(),
                binding: &binding,
                keys: &harness.keys,
                seed: &harness.seed,
            };
            // A retained reply cannot first bind expired execution terms.
            assert!(
                recovery
                    .recover_quote(issued.intent.id, &harness.policy(), now + 400)
                    .await
                    .is_err()
            );
            *writer.coordinator().lock().unwrap() = Some(crate::writer_lock::Operation {
                id: Uuid::new_v4(),
                authority: crate::writer_lock::Authority::TreasuryRecovery,
            });
            assert!(
                recovery
                    .recover_quote(issued.intent.id, &harness.policy(), now)
                    .await
                    .is_err()
            );
            *writer.coordinator().lock().unwrap() = None;
            let mut changed_mint = harness.policy();
            changed_mint.advertised_url = "https://replacement.example".into();
            assert!(
                recovery
                    .recover_quote(issued.intent.id, &changed_mint, now)
                    .await
                    .is_err()
            );
            let restored = recovery
                .recover_quote(issued.intent.id, &harness.policy(), now)
                .await?
                .unwrap();
            assert_eq!(restored.id, quote.id);
            assert_eq!(restored.request, quote.request);
            // Replay after CDK persistence must be local and must not overwrite it.
            assert!(
                recovery
                    .recover_quote(issued.intent.id, &harness.policy(), now + 400)
                    .await?
                    .is_some()
            );
            let mut conflicting = database.get_melt_quote(&quote.id).await?.unwrap();
            conflicting.amount = 8u64.into();
            database.add_melt_quote(conflicting).await?;
            assert!(
                recovery
                    .recover_quote(issued.intent.id, &harness.policy(), now)
                    .await
                    .is_err()
            );
            assert_eq!(
                u64::from(database.get_melt_quote(&quote.id).await?.unwrap().amount),
                8
            );
            assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
            return Ok(());
        }
        if matches!(
            scenario,
            TreasuryScenario::LostQuoteReply | TreasuryScenario::InvalidQuoteTerms
        ) {
            if matches!(scenario, TreasuryScenario::LostQuoteReply) {
                *harness.mint.quote_reply.lock().unwrap() = None;
            } else {
                harness.mint.quote_reply.lock().unwrap().as_mut().unwrap()["amount"] = json!(8);
            }
            let refused_quote: Result<Value, _> = treasury_gate
                .http_post(destination.clone(), None, &request)
                .await;
            assert!(refused_quote.is_err());
            assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
            drop(lease);
            let path = harness.directory.path().join("journal.sqlite");
            let db = rusqlite::Connection::open(&path)?;
            let (retained_request, retained_reply): (Vec<u8>, Option<Vec<u8>>) = db.query_row(
                "SELECT request, reply FROM treasury_quote_requests WHERE intent_id=?",
                [issued.intent.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(retained_request, serde_json::to_vec(&request)?);
            assert_eq!(
                retained_reply.is_some(),
                matches!(scenario, TreasuryScenario::InvalidQuoteTerms)
            );
            let bound: i64 =
                db.query_row("SELECT COUNT(*) FROM treasury_quotes", [], |r| r.get(0))?;
            assert_eq!(bound, 0);
            assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
            drop(db);
            let mut reopened = Journal::open(&path, harness.epoch, 8)?;
            let adoption = reopened.adopt_retained_treasury_quote(
                &binding,
                &harness.policy(),
                issued.intent.id,
                now,
            );
            if matches!(scenario, TreasuryScenario::LostQuoteReply) {
                assert!(adoption?.is_none());
            } else {
                assert!(adoption.is_err());
            }
            assert!(
                reopened
                    .claim_treasury_quote_request(
                        &binding,
                        &harness.policy(),
                        issued.intent.id,
                        &destination,
                        &request,
                        now,
                    )
                    .is_err()
            );
            assert!(
                reopened
                    .claim_treasury_quote_request(
                        &binding,
                        &harness.policy(),
                        issued.intent.id,
                        &destination,
                        &request,
                        now + 400,
                    )
                    .is_err()
            );
            return Ok(());
        }
        let received = coordinator
            .wallet
            .melt_quote(
                cdk::nuts::PaymentMethod::BOLT11,
                &issued.invoice,
                None,
                None,
            )
            .await?;
        assert_eq!(received.id, quote.id);
        assert_eq!(received.amount, quote.amount);
        assert_eq!(received.fee_reserve, quote.fee_reserve);
        assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
        let duplicate: Result<Value, _> =
            treasury_gate.http_post(destination, None, &request).await;
        assert!(duplicate.is_err());
        assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
        drop(lease);
    }

    let prepared = coordinator
        .wallet
        .prepare_melt_proofs(&quote.id, proofs, Default::default())
        .await?;
    {
        let mut journal = coordinator.journal.lock().unwrap();
        journal.bind_prepared_treasury_melt(&binding, issued.intent.id, &prepared, now)?;
        if real_transport {
            assert!(
                journal
                    .adopt_retained_treasury_quote(
                        &binding,
                        &harness.policy(),
                        issued.intent.id,
                        now,
                    )
                    .is_err()
            );
        }
    }
    if real_transport {
        let lease = treasury_gate.arm_treasury(issued.intent.id)?;
        let confirmation = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            prepared.confirm_with_options(cdk::wallet::MeltConfirmOptions::skip_swap()),
        )
        .await;
        assert!(!matches!(confirmation, Ok(Ok(_))));
        assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
        drop(lease);
        // CDK may retain or adjust its saga after an error; our durable claim
        // must remain and refuse any second outbound call independently.
        let db = rusqlite::Connection::open(harness.directory.path().join("journal.sqlite"))?;
        let (state, bytes): (String,Vec<u8>) = db.query_row("SELECT i.state,r.request FROM treasury_instructions i JOIN treasury_melt_requests r ON r.intent_id=i.id WHERE i.id=?", [issued.intent.id.to_string()], |r|Ok((r.get(0)?,r.get(1)?)))?;
        assert_eq!(state, "submitted");
        let lease = treasury_gate.arm_treasury(issued.intent.id)?;
        let payload: Value = serde_json::from_slice(&bytes)?;
        let retry: Result<Value, _> = treasury_gate
            .http_post(coordinator.mint_url.join("v1/melt/bolt11")?, None, &payload)
            .await;
        assert!(retry.is_err());
        assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
        drop(lease);
        return Ok(());
    }
    // Reopen our durable journal while CDK retains the original reserved saga.
    let mut reopened = Journal::open(
        &harness.directory.path().join("journal.sqlite"),
        harness.epoch,
        8,
    )?;
    reopened.bind_prepared_treasury_melt(&binding, issued.intent.id, &prepared, now + 400)?;
    // Model the pre-POST transition using the actual prepared saga and proofs.
    // Transport integration will separately capture CDK's generated transition.
    use cdk::wallet::types::{MeltSagaState, OperationData, WalletSagaState};
    let mut saga = coordinator
        .database
        .get_incomplete_sagas()
        .await?
        .into_iter()
        .find(|s| s.id == prepared.operation_id())
        .unwrap();
    saga.state = WalletSagaState::Melt(MeltSagaState::MeltRequested);
    let final_proofs = prepared
        .proofs()
        .iter()
        .chain(prepared.proofs_to_swap())
        .cloned()
        .collect::<Vec<_>>();
    let outputs = cdk::nuts::PreMintSecrets::from_seed_blank(
        final_proofs[0].keyset_id,
        100,
        &harness.seed,
        1u64.into(),
    )?
    .blinded_messages();
    let OperationData::Melt(data) = &mut saga.data else {
        unreachable!()
    };
    data.counter_start = Some(100);
    data.counter_end = Some(100 + outputs.len() as u32);
    data.change_amount = Some(1u64.into());
    data.final_proof_ys = Some(
        final_proofs
            .iter()
            .map(|p| p.y())
            .collect::<Result<Vec<_>, _>>()?,
    );
    data.change_blinded_messages = Some(outputs.clone());
    let request = cdk::nuts::MeltRequest::new(quote.id.clone(), final_proofs, Some(outputs));
    assert!(
        reopened
            .freeze_treasury_melt_request(
                &binding,
                issued.intent.id,
                &saga,
                &request,
                &harness.seed,
                now + 400
            )
            .is_err()
    );
    reopened.freeze_treasury_melt_request(
        &binding,
        issued.intent.id,
        &saga,
        &request,
        &harness.seed,
        now,
    )?;
    drop(reopened);
    let mut reopened = Journal::open(
        &harness.directory.path().join("journal.sqlite"),
        harness.epoch,
        8,
    )?;
    reopened.freeze_treasury_melt_request(
        &binding,
        issued.intent.id,
        &saga,
        &request,
        &harness.seed,
        now + 400,
    )?;
    let changed = request.clone().prefer_async(true);
    assert!(
        reopened
            .freeze_treasury_melt_request(
                &binding,
                issued.intent.id,
                &saga,
                &changed,
                &harness.seed,
                now
            )
            .is_err()
    );
    let mut wrong_operation = saga.clone();
    wrong_operation.id = Uuid::new_v4();
    assert!(
        reopened
            .freeze_treasury_melt_request(
                &binding,
                issued.intent.id,
                &wrong_operation,
                &request,
                &harness.seed,
                now
            )
            .is_err()
    );
    let unspent = CheckStateResponse {
        states: request
            .inputs()
            .iter()
            .map(|p| Ok((p.y()?, State::Unspent).into()))
            .collect::<Result<Vec<_>>>()?,
    };
    let owned_inputs = coordinator
        .database
        .get_proofs_by_ys(
            request
                .inputs()
                .iter()
                .map(|p| p.y())
                .collect::<Result<Vec<_>, _>>()?,
        )
        .await?;
    crate::melt_derivation::verify_melt_inputs(
        &request,
        "fixture",
        &coordinator.mint_url,
        &owned_inputs,
        &harness.keys,
        &unspent,
    )?;
    for state in [State::Pending, State::Spent] {
        let mut unavailable = unspent.clone();
        unavailable.states[0].state = state;
        assert!(
            crate::melt_derivation::verify_melt_inputs(
                &request,
                "fixture",
                &coordinator.mint_url,
                &owned_inputs,
                &harness.keys,
                &unavailable
            )
            .is_err()
        );
    }
    assert!(
        crate::melt_derivation::verify_melt_inputs(
            &request,
            "other-mint",
            &coordinator.mint_url,
            &owned_inputs,
            &harness.keys,
            &unspent
        )
        .is_err()
    );
    let mut missing_dleq = owned_inputs.clone();
    missing_dleq[0].proof.dleq = None;
    assert!(
        crate::melt_derivation::verify_melt_inputs(
            &request,
            "fixture",
            &coordinator.mint_url,
            &missing_dleq,
            &harness.keys,
            &unspent
        )
        .is_err()
    );
    let mut forged = request.clone();
    forged.inputs_mut()[0].c = SecretKey::from_slice(&[99; 32])?.public_key();
    assert!(
        crate::melt_derivation::verify_melt_inputs(
            &forged,
            "fixture",
            &coordinator.mint_url,
            &owned_inputs,
            &harness.keys,
            &unspent
        )
        .is_err()
    );
    let pending_reply = br#"{"state":"PENDING"}"#;
    assert!(
        reopened
            .record_treasury_melt_reply(issued.intent.id, pending_reply)
            .is_err()
    );
    let request_bytes = serde_json::to_vec(&request)?;
    let destination = coordinator.mint_url.join("v1/melt/bolt11")?;
    assert!(
        reopened
            .claim_treasury_melt_submission(
                &binding,
                issued.intent.id,
                &destination,
                &request_bytes,
                now + 400
            )
            .is_err()
    );
    assert!(
        reopened
            .claim_treasury_melt_submission(
                &binding,
                issued.intent.id,
                &"https://other.example/v1/melt/bolt11".parse()?,
                &request_bytes,
                now
            )
            .is_err()
    );
    assert!(
        reopened
            .claim_treasury_melt_submission(
                &binding,
                issued.intent.id,
                &destination,
                &serde_json::to_vec(&changed)?,
                now
            )
            .is_err()
    );
    reopened.claim_treasury_melt_submission(
        &binding,
        issued.intent.id,
        &destination,
        &request_bytes,
        now,
    )?;
    assert!(
        reopened
            .claim_treasury_melt_submission(
                &binding,
                issued.intent.id,
                &destination,
                &request_bytes,
                now
            )
            .is_err()
    );
    drop(reopened);
    let mut reopened = Journal::open(
        &harness.directory.path().join("journal.sqlite"),
        harness.epoch,
        8,
    )?;
    assert!(
        reopened
            .claim_treasury_melt_submission(
                &binding,
                issued.intent.id,
                &destination,
                &request_bytes,
                now
            )
            .is_err()
    );
    reopened.record_treasury_melt_reply(issued.intent.id, pending_reply)?;
    reopened.record_treasury_melt_reply(issued.intent.id, pending_reply)?;
    assert!(
        reopened
            .record_treasury_melt_reply(issued.intent.id, br#"{"state":"PAID"}"#)
            .is_err()
    );
    binding.wallet_epoch = Uuid::new_v4();
    assert!(
        reopened
            .bind_prepared_treasury_melt(&binding, issued.intent.id, &prepared, now)
            .is_err()
    );
    // The fixture transport rejects melt POSTs. Preparation must never pay.
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn retired_descriptor_preserves_exact_admission_retry_after_its_window() -> Result<()> {
    use crate::descriptor::{Descriptor, Encoding, RetiredDescriptor, RetiredMint};
    use crate::ingress::Intake;
    use lightning_goats_cashu_receipt::receive_status::{
        AdmissionOutcome, AdmissionQuery, AdmissionReply, AdmitOperation, Status, http_delivery_id,
    };
    let harness = Harness::new(0)?;
    let writer = WriterLock::acquire(harness.directory.path())?;
    let journal = Arc::new(Mutex::new(Journal::open(
        &harness.directory.path().join("journal.sqlite"),
        harness.epoch,
        8,
    )?));
    let original = harness.policy();
    let receiver = harness.receiver.public_key();
    let endpoint = "https://lightning-goats.example/api/v1/cashu";
    let old = Descriptor::prepare(
        &MintRegistry::new(vec![original.clone()])?,
        endpoint,
        receiver,
        Encoding::CreqA,
    )?;
    let mut current = original.clone();
    current.policy_version = "current-v2".into();
    let other = MintPolicy {
        id: "second".into(),
        advertised_url: "https://second.example".into(),
        ..current.clone()
    };
    let registry = MintRegistry::new(vec![current.clone(), other.clone()])?;
    let current_descriptor = Descriptor::prepare(&registry, endpoint, receiver, Encoding::CreqA)?;
    let active = [current_descriptor.id().to_owned()];
    assert_ne!(old.id(), current_descriptor.id());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let retired = |accept_before| RetiredDescriptor {
        id: old.id().into(),
        endpoint: endpoint.into(),
        receiving_key: None,
        retired_at: now - 60,
        accept_before,
        mints: vec![RetiredMint {
            id: original.id.clone(),
            advertised_url: original.advertised_url.clone(),
            policy_version: original.policy_version.clone(),
        }],
    };
    let within_window = [retired(now + 600)];
    let expired = [retired(now)];
    within_window[0].verify(receiver)?;
    assert!(!within_window[0].accepts(old.id(), &current, now - 61));
    assert!(within_window[0].accepts(old.id(), &current, now - 60));
    assert!(within_window[0].accepts(old.id(), &current, now + 599));
    assert!(!within_window[0].accepts(old.id(), &current, now + 600));
    let mut keys = (*harness.keys).clone();
    for (amount, key) in harness.mint.keyset.keys.iter() {
        keys.insert(&other.id, harness.mint.keyset.id, u64::from(*amount), *key)?;
    }
    let mut intake = Intake {
        core_uid: 1,
        wallet_instance: Uuid::new_v4(),
        wallet_epoch: harness.epoch,
        receive_cashu: true,
        receive_nutzap: false,
        registry: &registry,
        keys: &keys,
        limits: &Limits {
            body_bytes: 65536,
            proof_count: 8,
            receipt_sats: 10000,
        },
        receiver,
        recipient: "",
        descriptors: &active,
        retired_descriptors: &within_window,
        retained_receivers: &[],
        public_request: Some(&current_descriptor),
        journal: journal.clone(),
        writer: &writer,
    };
    let query = |descriptor, mint, label: &str| {
        let body = json!({
            "id": descriptor, "mint": mint, "unit": "sat",
            "proofs": [harness.mint.issue(label.into())],
        })
        .to_string();
        AdmissionQuery {
            version: 1,
            operation: AdmitOperation::OrdinaryAdmit,
            wallet_instance: intake.wallet_instance,
            wallet_epoch: harness.epoch,
            delivery_id: http_delivery_id(body.as_bytes()),
            body,
        }
    };
    let reply = |intake: &Intake<'_>, query: &AdmissionQuery| -> Result<AdmissionOutcome> {
        let response: AdmissionReply =
            serde_json::from_slice(&intake.reply_frame(&serde_json::to_vec(query)?)?)?;
        response.validate(query)?;
        Ok(response.result)
    };
    let retired_submission = query(old.id(), original.advertised_url.as_str(), "before-cutoff");
    let late_submission = query(old.id(), original.advertised_url.as_str(), "after-cutoff");
    let other_mint = query(old.id(), other.advertised_url.as_str(), "never-listed");
    let current_submission = query(
        current_descriptor.id(),
        other.advertised_url.as_str(),
        "current-qr",
    );
    let AdmissionOutcome::Admitted {
        capability,
        status: Status::Pending,
    } = reply(&intake, &retired_submission)?
    else {
        bail!("retired request was rejected within window")
    };
    assert!(matches!(
        reply(&intake, &other_mint)?,
        AdmissionOutcome::Rejected
    ));
    let mut disabled = current;
    disabled.receive_cashu = false;
    let disabled_registry = MintRegistry::new(vec![disabled, other])?;
    intake.registry = &disabled_registry;
    assert!(matches!(
        reply(&intake, &late_submission)?,
        AdmissionOutcome::Rejected
    ));
    intake.registry = &registry;
    intake.retired_descriptors = &expired;
    assert!(matches!(
        reply(&intake, &late_submission)?,
        AdmissionOutcome::Rejected
    ));
    let AdmissionOutcome::Admitted {
        capability: retry,
        status: Status::Pending,
    } = reply(&intake, &retired_submission)?
    else {
        bail!("admitted request lost its retry status")
    };
    assert!(capability == retry);
    let mut changed = late_submission;
    changed.delivery_id = retired_submission.delivery_id;
    assert!(reply(&intake, &changed).is_err());
    assert!(matches!(
        reply(&intake, &current_submission)?,
        AdmissionOutcome::Admitted { .. }
    ));
    assert_eq!(journal.lock().unwrap().pending_operations(8)?.len(), 2);
    assert_eq!(harness.mint.swap_attempts.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn private_intake_authenticates_before_read_and_deduplicates_across_transports() -> Result<()>
{
    use lightning_goats_cashu_receipt::ingress::{Disposition, Payload, Request};
    let harness = Harness::new(0)?;
    let writer = WriterLock::acquire(harness.directory.path())?;
    let journal = Arc::new(Mutex::new(Journal::open(
        &harness.directory.path().join("journal.sqlite"),
        harness.epoch,
        8,
    )?));
    let registry = MintRegistry::new(vec![harness.policy()])?;
    let proof = harness.locked_proof();
    let (nutzap, recipient) = harness.nutzap_body(proof.clone())?;
    let mut intake = crate::ingress::Intake {
        core_uid: rustix::process::geteuid().as_raw(),
        wallet_instance: Uuid::new_v4(),
        wallet_epoch: harness.epoch,
        receive_cashu: true,
        receive_nutzap: true,
        registry: &registry,
        keys: &harness.keys,
        limits: &Limits {
            body_bytes: 65536,
            proof_count: 8,
            receipt_sats: 10000,
        },
        receiver: harness.receiver.public_key(),
        recipient: &recipient,
        descriptors: &["reusable".into()],
        retired_descriptors: &[],
        retained_receivers: &[],
        public_request: None,
        journal: journal.clone(),
        writer: &writer,
    };
    // No bytes are sent. Same-UID access must fail before waiting for a frame.
    let (mut receiver, _sender) = tokio::net::UnixStream::pair()?;
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        intake.serve_connection(&mut receiver, std::time::Duration::from_secs(1)),
    )
    .await?;
    assert!(result.is_err());
    // Public metadata uses the same authenticated transport but never enters
    // acquisition admission or creates a delivery receipt.
    use lightning_goats_cashu_receipt::public_request::{
        Operation, Query as PublicQuery, Reply as PublicReply,
    };
    let descriptor = crate::descriptor::Descriptor::prepare(
        &registry,
        "https://lightning-goats.example/api/v1/cashu",
        harness.receiver.public_key(),
        crate::descriptor::Encoding::CreqA,
    )?;
    let descriptor_ids = vec!["reusable".to_owned(), descriptor.id().to_owned()];
    intake.descriptors = &descriptor_ids;
    intake.public_request = Some(&descriptor);
    let query = PublicQuery {
        version: 1,
        operation: Operation::PublicRequest,
        wallet_instance: intake.wallet_instance,
        wallet_epoch: harness.epoch,
        request_id: Uuid::new_v4(),
    };
    let bytes = serde_json::to_vec(&query)?;
    let reply = PublicReply::decode(&intake.reply_frame(&bytes)?, &query)?;
    assert_eq!(reply.descriptor.unwrap().request, descriptor.encoded());
    intake.receive_cashu = false;
    assert!(
        PublicReply::decode(&intake.reply_frame(&bytes)?, &query)?
            .descriptor
            .is_none()
    );
    intake.receive_cashu = true;
    let mut wrong = serde_json::to_value(&query)?;
    wrong["wallet_epoch"] = json!(Uuid::new_v4());
    assert!(intake.reply_frame(&serde_json::to_vec(&wrong)?).is_err());
    wrong = serde_json::to_value(&query)?;
    wrong["proofs"] = json!([]);
    assert!(intake.reply_frame(&serde_json::to_vec(&wrong)?).is_err());
    assert!(journal.lock().unwrap().pending_operations(10)?.is_empty());
    let mut request = Request {
        version: 1,
        wallet_instance: intake.wallet_instance,
        wallet_epoch: harness.epoch,
        delivery_id: Uuid::new_v4(),
        payload: Payload::Cashu(
            json!({"id":"reusable","mint":harness.policy().advertised_url,
            "unit":"sat","proofs":[proof]})
            .to_string(),
        ),
    };
    let original = intake.admit_frame(&serde_json::to_vec(&request)?)?;
    assert!(original.disposition == Disposition::Admitted);
    // The public admission operation uses the same proof index and creates
    // one stable status capability, never another acquisition or mint request.
    use lightning_goats_cashu_receipt::receive_status as receipt_status;
    let Payload::Cashu(body) = &request.payload else {
        unreachable!()
    };
    let mut public = receipt_status::AdmissionQuery {
        version: 1,
        operation: receipt_status::AdmitOperation::OrdinaryAdmit,
        wallet_instance: intake.wallet_instance,
        wallet_epoch: harness.epoch,
        delivery_id: Uuid::new_v4(),
        body: body.clone(),
    };
    let response: receipt_status::AdmissionReply =
        serde_json::from_slice(&intake.reply_frame(&serde_json::to_vec(&public)?)?)?;
    response.validate(&public)?;
    let receipt_status::AdmissionOutcome::Admitted { capability, status } = response.result else {
        bail!("valid ordinary receipt rejected")
    };
    assert_eq!(status, receipt_status::Status::Pending);
    public.delivery_id = Uuid::new_v4();
    let repeated: receipt_status::AdmissionReply =
        serde_json::from_slice(&intake.reply_frame(&serde_json::to_vec(&public)?)?)?;
    repeated.validate(&public)?;
    let receipt_status::AdmissionOutcome::Admitted {
        capability: again, ..
    } = repeated.result
    else {
        bail!("duplicate ordinary receipt rejected")
    };
    assert!(again == capability);
    let reopened = Journal::open(
        &harness.directory.path().join("journal.sqlite"),
        harness.epoch,
        8,
    )?;
    assert_eq!(
        reopened.receipt_status(&capability)?,
        Some(receipt_status::Status::Pending)
    );
    drop(reopened);
    intake.receive_cashu = false;
    assert!(intake.reply_frame(&serde_json::to_vec(&public)?).is_err());
    let query = receipt_status::Query {
        version: 1,
        operation: receipt_status::QueryOperation::ReceiveStatus,
        wallet_instance: intake.wallet_instance,
        wallet_epoch: harness.epoch,
        request_id: Uuid::new_v4(),
        capability,
    };
    let response: receipt_status::Reply =
        serde_json::from_slice(&intake.reply_frame(&serde_json::to_vec(&query)?)?)?;
    response.validate(&query)?;
    assert_eq!(response.status, Some(receipt_status::Status::Pending));
    intake.receive_cashu = true;
    public.body = "{}".into();
    let rejected: receipt_status::AdmissionReply =
        serde_json::from_slice(&intake.reply_frame(&serde_json::to_vec(&public)?)?)?;
    assert!(matches!(
        rejected.result,
        receipt_status::AdmissionOutcome::Rejected
    ));
    assert_eq!(journal.lock().unwrap().pending_operations(10)?.len(), 1);
    request.delivery_id = Uuid::new_v4();
    request.payload = Payload::Nutzap(nutzap);
    let duplicate = intake.admit_frame(&serde_json::to_vec(&request)?)?;
    assert_eq!(original.acquisition_id, duplicate.acquisition_id);
    assert!(duplicate.disposition == Disposition::Existing);
    assert!(
        journal
            .lock()
            .unwrap()
            .export_receipt(original.acquisition_id)
            .is_err()
    );
    request.wallet_epoch = Uuid::new_v4();
    assert!(intake.admit_frame(&serde_json::to_vec(&request)?).is_err());
    request.wallet_epoch = harness.epoch;
    intake.receive_nutzap = false;
    assert!(intake.admit_frame(&serde_json::to_vec(&request)?).is_err());
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 0);
    Ok(())
}

mod core_settlement_fixture;
mod split_tests;
mod wss_receipt_fixture;

mod multi_mint;

#[tokio::test]
async fn operator_ownership_check_uses_existing_outputs_without_swap_or_receipt_export()
-> Result<()> {
    let harness = Harness::new(0)?;
    harness.mint.fail_check_once.store(true, Ordering::SeqCst);
    let mut coordinator = harness.open().await?;
    let id = new_id(coordinator.admit(&harness.ordinary(
        harness.mint.issue("operator-owned-only".into()),
        "operator-owned-only",
    )?)?);
    assert!(matches!(
        coordinator.acquire_new(id, &harness.receiver).await?,
        Progress::Pending(_)
    ));
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    coordinator.set_permissions(Permissions::default());
    assert!(
        matches!(coordinator.reconcile_owned(id).await?,Progress::Acquired(found) if found==id)
    );
    assert_eq!(coordinator.journal.lock().unwrap().state(id)?, "acquired");
    assert!(
        matches!(coordinator.reconcile_owned(id).await?,Progress::Acquired(found) if found==id)
    );
    assert_eq!(coordinator.journal.lock().unwrap().state(id)?, "acquired");
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
    Ok(())
}
