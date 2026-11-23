//! One journaled action per scheduler step. No HTTP result alone authorizes
//! member announcements. Pending/unknown execution keeps its original identity.
use super::{
    invoice::{Intent, Request, ValidatedInvoice},
    outcome::Progress,
    signer::Client as Signer,
    store::Store,
};
use crate::{
    core_bridge::{AuthenticatedFundingPage, unix::Client as Core},
    cyberherd::calendar,
};
use anyhow::{Context, Result};

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Waiting,
    InvoiceIntent,
    InvoiceSigned,
    InvoiceSaved,
    QuoteSaved,
    FeeObserved,
    QuoteRetired,
    Submitted,
    Observed,
    Closed,
    Outcome(Progress),
}
pub(crate) trait InvoiceFetcher {
    fn fetch(
        &self,
        request: &Request,
    ) -> impl std::future::Future<Output = Result<ValidatedInvoice>> + Send;
}
async fn prepare_callback<F: InvoiceFetcher>(
    store: &Store,
    signer: &Signer,
    intent: &Intent,
    fresh: &AuthenticatedFundingPage,
    fetcher: &F,
) -> Result<Step> {
    let callback = store.callback_request(intent.id()).await?;
    let now = calendar::now()?;
    if !fresh.page().cashu_invoice_permitted(now) {
        return Ok(Step::Waiting);
    }
    let request = match callback {
        Some(request) => request,
        None => {
            let raw = if intent.uses_nostr() {
                signer.sign(intent).await.ok()
            } else {
                None
            };
            store
                .freeze_request(intent.id(), raw.as_deref(), calendar::now()?)
                .await?;
            return Ok(Step::InvoiceSigned);
        }
    };
    let invoice = fetcher.fetch(&request).await?;
    store.freeze_invoice(&invoice, calendar::now()?).await?;
    Ok(Step::InvoiceSaved)
}

pub(crate) struct CashuWorker {
    pub reclaim_failed: bool,
    pub collector_socket: Option<std::path::PathBuf>,
    pub store: Store,
    pub core: Core,
    pub signer: Signer,
    pub plans: crate::cyberherd::payout_bridge::Client,
}
struct MemberInvoiceFetcher<'a> {
    reclaim: Option<(&'a Store, uuid::Uuid)>,
    plans: &'a crate::cyberherd::payout_bridge::Client,
    feed: uuid::Uuid,
    member: &'a str,
}
impl InvoiceFetcher for MemberInvoiceFetcher<'_> {
    async fn fetch(&self, request: &Request) -> Result<ValidatedInvoice> {
        let result = request.fetch().await;
        if let (Err(error), Some((store, grant))) = (&result, self.reclaim)
            && (error.is::<super::invoice::DescriptionHashMismatch>()
                || error.is::<super::invoice::RecipientRejected>())
        {
            store.close_rejected_cashu(grant, self.member).await?;
        }
        if request.uses_primal_callback()
            && result
                .as_ref()
                .is_err_and(|err| err.is::<super::invoice::DescriptionHashMismatch>())
        {
            self.plans
                .primal_invoice_mismatch(self.feed, self.member)
                .await?;
        }
        result
    }
}
impl CashuWorker {
    pub async fn tick(&self) -> Result<Step> {
        // Production requests reach a PID 1-owned collector socket.
        self.tick_with_collector_uid(0).await
    }

    pub(crate) async fn tick_with_collector_uid(&self, collector_uid: u32) -> Result<Step> {
        let policy = self.store.policy();
        let Some(work) = self
            .store
            .take_provider_work(calendar::now()?, "cashu")
            .await?
        else {
            return Ok(Step::Idle);
        };
        let Some(member) = work.member else {
            let Some(socket) = &self.collector_socket else {
                return Ok(Step::Waiting);
            };
            if !self.store.freeze_cashu_outcomes(work.grant, socket).await? {
                return Ok(Step::Waiting);
            }
            return Ok(Step::Outcome(
                self.store
                    .reconcile_outcomes(&self.core, work.grant)
                    .await?,
            ));
        };
        if let Some(terms) = self
            .store
            .member_ecash_assignment(work.grant, &member)
            .await?
        {
            let Some(socket) = &self.collector_socket else {
                return Ok(Step::Waiting);
            };
            if !policy.execution_enabled
                || self.store.grant_fee_mode(work.grant).await? != policy.fee_mode
            {
                return Ok(Step::Waiting);
            }
            return match self
                .core
                .submit_member_ecash(&terms, socket, collector_uid)
                .await?
            {
                super::operator::DirectStatus::Ready {
                    amount_sat,
                    fee_sat,
                } => {
                    if matches!(
                        terms.destination,
                        crate::cyberherd::rewards::EcashDestination::PublicNutzap(_)
                    ) {
                        self.signer.sign_member_reward(&terms).await?;
                    }
                    self.store
                        .record_member_ecash_ready(&terms, amount_sat, fee_sat)
                        .await?;
                    Ok(Step::Observed)
                }
                super::operator::DirectStatus::Held { .. } => Ok(Step::Waiting),
            };
        }
        let intent = self.store.saved_invoice_intent(work.grant, &member).await?;
        let enabled = policy.execution_enabled
            && self.store.grant_fee_mode(work.grant).await? == policy.fee_mode;
        if let Some(intent) = &intent
            && let Some(invoice) = self.store.saved_invoice(intent.id()).await?
        {
            let Some(socket) = &self.collector_socket else {
                return Ok(Step::Waiting);
            };
            // PID 1 owns the listening socket passed to the dedicated collector service.
            let limit = self
                .store
                .cashu_member_fee_limit(work.grant, &member, invoice.amount_sats())
                .await?;
            let status = super::cashu::lookup(socket, 0, &invoice, limit).await?;
            match status {
                super::cashu::CollectorResult::Paid { .. } => {
                    self.store.freeze_cashu_outcomes(work.grant, socket).await?;
                    return Ok(Step::Observed);
                }
                super::cashu::CollectorResult::Pending { .. } => return Ok(Step::Waiting),
                _ => {}
            }
            if !enabled || invoice.amount_sats() > policy.max_net_sats {
                return Ok(Step::Waiting);
            }
            let (claim, payload) = self.store.cashu_payout_request(intent.id()).await?;
            let lease = self.core.claim_cashu_execution(&claim).await?;
            super::cashu::submit(socket, 0, &lease, &payload).await?;
            return Ok(Step::Submitted);
        }
        if !enabled
            || intent
                .as_ref()
                .is_some_and(|i| i.0.net_sats > policy.max_net_sats)
        {
            return Ok(Step::Waiting);
        }
        let grant = self
            .store
            .grant(work.grant)
            .await?
            .context("Cashu grant missing")?;
        let nostr = self.store.row_uses_nostr(work.grant, &member).await?;
        let joining = if intent.is_none() && nostr {
            self.plans
                .joining(grant.request_id, &member)
                .await
                .unwrap_or(None)
        } else {
            None
        };
        let fresh = self
            .core
            .payout_funding(policy.scope.epoch, grant.id, &policy.scope.owner, 0)
            .await?;
        let page = fresh.page();
        if !page.cashu_invoice_permitted(calendar::now()?)
            || (nostr && !page.nostr_enabled)
            || page.reservation.request_id != grant.request_id
            || page.reservation.plan_hash != grant.plan_hash
            || page.reservation.reserved_sats != grant.reserved_sats
        {
            return Ok(Step::Waiting);
        }
        let fetcher = MemberInvoiceFetcher {
            reclaim: self.reclaim_failed.then_some((&self.store, work.grant)),
            plans: &self.plans,
            feed: grant.request_id,
            member: &member,
        };
        if let Some(intent) = intent {
            return prepare_callback(&self.store, &self.signer, &intent, &fresh, &fetcher).await;
        }
        if !policy.fee_mode.is_fixed() {
            let Some(socket) = &self.collector_socket else {
                return Ok(Step::Waiting);
            };
            let probe = self
                .store
                .prepare_fee_probe(grant.id, &member, 0, calendar::now()?, joining.as_ref())
                .await?;
            if let Some(invoice) = self.store.saved_fee_probe_invoice(probe.id()).await? {
                let allocations = self.store.cashu_probe_allocations(grant.id).await?;
                let fee = super::cashu::payout_fee(
                    socket,
                    0,
                    &invoice,
                    grant.request_id,
                    probe.0.net_sats,
                    &allocations,
                )
                .await?;
                self.store
                    .prepare_member_invoice(
                        grant.id,
                        &member,
                        calendar::now()?,
                        joining.as_ref(),
                        Some(fee),
                    )
                    .await?;
                return Ok(Step::InvoiceIntent);
            }
            if let Some(request) = self.store.fee_probe_callback(probe.id()).await? {
                let invoice = fetcher.fetch(&request).await?;
                self.store
                    .freeze_fee_probe_invoice(&invoice, calendar::now()?)
                    .await?;
                return Ok(Step::InvoiceSaved);
            }
            let raw = if probe.uses_nostr() {
                self.signer.sign(&probe).await.ok()
            } else {
                None
            };
            self.store
                .freeze_fee_probe_request(probe.id(), raw.as_deref(), calendar::now()?)
                .await?;
            return Ok(Step::InvoiceSigned);
        }
        self.store
            .prepare_invoice_with_reference(grant.id, &member, calendar::now()?, joining.as_ref())
            .await?;
        Ok(Step::InvoiceIntent)
    }
}
