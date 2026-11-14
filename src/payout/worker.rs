//! One journaled action per scheduler step. No HTTP result alone authorizes
//! member announcements. Pending/unknown execution keeps its original identity.
use super::{
    invoice::{Intent, Request, ValidatedInvoice},
    outcome::{Progress, UnsubmittedReason},
    provider::{Client as Sender, FeeProbeReply, QuoteReply},
    signer::Client as Signer,
    store::{PayoutStatus, Store},
};
use crate::{
    core_bridge::{AuthenticatedFundingPage, unix::Client as Core},
    cyberherd::calendar,
};
use anyhow::{Context, Result, bail};

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
pub struct Worker {
    pub store: Store,
    pub core: Core,
    pub signer: Signer,
    pub sender: Sender,
    pub presentation: Option<crate::cyberherd::payout_bridge::Client>,
}
pub(crate) trait InvoiceFetcher {
    fn fetch(
        &self,
        request: &Request,
    ) -> impl std::future::Future<Output = Result<ValidatedInvoice>> + Send;
}
struct HttpFetcher;
impl InvoiceFetcher for HttpFetcher {
    async fn fetch(&self, request: &Request) -> Result<ValidatedInvoice> {
        request.fetch().await
    }
}
impl Worker {
    pub async fn tick(&self) -> Result<Step> {
        self.tick_with(&HttpFetcher).await
    }
    pub(crate) async fn tick_with<F: InvoiceFetcher>(&self, fetcher: &F) -> Result<Step> {
        let Some(work) = self.store.take_work(calendar::now()?).await? else {
            return Ok(Step::Idle);
        };
        let Some(member) = work.member else {
            self.store
                .freeze_outcomes(work.grant, calendar::now()?)
                .await?;
            return Ok(Step::Outcome(
                self.store
                    .reconcile_outcomes(&self.core, work.grant)
                    .await?,
            ));
        };
        let intent = match self.store.saved_invoice_intent(work.grant, &member).await? {
            Some(intent) => intent,
            None => {
                let mode = self.store.grant_fee_mode(work.grant).await?;
                if mode != self.store.policy().fee_mode {
                    return Ok(Step::Waiting);
                }
                if !mode.is_fixed() {
                    return self.discover_fee(work.grant, &member, fetcher).await;
                }
                let nostr = self.store.row_uses_nostr(work.grant, &member).await?;
                if self.permission(work.grant, &member, nostr).await?.is_none() {
                    return Ok(Step::Waiting);
                }
                let joining = if let Some(presentation) = &self.presentation {
                    let grant = self
                        .store
                        .grant(work.grant)
                        .await?
                        .context("payout work lost its grant")?;
                    // Receipt availability never blocks or downgrades a zap:
                    // absent/unavailable joining evidence uses a profile zap.
                    presentation
                        .joining(grant.request_id, &member)
                        .await
                        .unwrap_or(None)
                } else {
                    None
                };
                self.store
                    .prepare_invoice_with_reference(
                        work.grant,
                        &member,
                        calendar::now()?,
                        joining.as_ref(),
                    )
                    .await?;
                return Ok(Step::InvoiceIntent);
            }
        };
        let status = self.store.payout_status(intent.id()).await?;
        let quote = self.store.current_quote(intent.id()).await?;
        if let Some(quote) = &quote {
            // Read known payments even when execution/Nostr have been disabled.
            if let Some(bound) = self.store.bound_payment(quote.id()).await? {
                let observation = self.sender.read_payment(&bound).await?;
                self.store
                    .observe_payment(&observation, calendar::now()?)
                    .await?;
                return Ok(Step::Observed);
            }
        }
        // Reconciliation above remains available after a limit reduction. New
        // callbacks, quotes, signatures and original-quote execution stay held.
        if intent.0.net_sats > self.store.policy().max_net_sats
            || self.store.grant_fee_mode(work.grant).await? != self.store.policy().fee_mode
        {
            return Ok(Step::Waiting);
        }
        let Some(fresh) = self
            .permission(work.grant, &member, intent.uses_nostr())
            .await?
        else {
            return Ok(Step::Waiting);
        };
        if status == PayoutStatus::ExecutionUnknown || fresh.claim().is_some() {
            let quote = quote.context("uncertain execution lost original quote")?;
            return self.submit(quote.id(), &fresh).await;
        }
        // Only the no-execute paths may expire/close. A paused switch alone
        // never discards a reserved distribution or retries a possible payment.
        let saved = self.store.saved_invoice(intent.id()).await?;
        let now = calendar::now()?;
        let timestamp = u64::try_from(now)?;
        if saved
            .as_ref()
            .is_some_and(|invoice| timestamp >= invoice.expires_at())
            || (saved.is_none() && now.saturating_sub(intent.0.created_at) > 3600)
        {
            self.store
                .close_unsubmitted(
                    work.grant,
                    &member,
                    UnsubmittedReason::Expired,
                    calendar::now()?,
                )
                .await?;
            return Ok(Step::Closed);
        }
        if saved.is_none() {
            return self.invoice(&intent, fresh.page(), fetcher).await;
        }
        let quote = match quote {
            Some(quote) => quote,
            None => {
                self.store
                    .prepare_quote(intent.id(), calendar::now()?)
                    .await?
            }
        };
        if let Some(terms) = self.store.quote_terms(quote.id()).await? {
            if calendar::now()? >= terms.usable_until {
                if !self.store.policy().fee_mode.is_fixed() {
                    self.store
                        .close_unsubmitted(
                            work.grant,
                            &member,
                            UnsubmittedReason::Expired,
                            calendar::now()?,
                        )
                        .await?;
                    return Ok(Step::Closed);
                }
                self.store
                    .retire_quote(quote.id(), calendar::now()?)
                    .await?;
                return Ok(Step::QuoteRetired);
            }
            return self.submit(quote.id(), &fresh).await;
        }
        if !fresh.page().page().execution_permitted(calendar::now()?) {
            return Ok(Step::Waiting);
        }
        match self.sender.create_quote(&quote).await? {
            QuoteReply::Ready(quote) => {
                self.store.freeze_quote(&quote, calendar::now()?).await?;
                Ok(Step::QuoteSaved)
            }
            QuoteReply::Duplicate(duplicate) => {
                self.store
                    .observe_duplicate_quote(&duplicate, calendar::now()?)
                    .await?;
                self.store
                    .retire_quote(quote.id(), calendar::now()?)
                    .await?;
                Ok(Step::QuoteRetired)
            }
        }
    }
    async fn discover_fee<F: InvoiceFetcher>(
        &self,
        grant: uuid::Uuid,
        member: &str,
        fetcher: &F,
    ) -> Result<Step> {
        let nostr = self.store.row_uses_nostr(grant, member).await?;
        let Some(fresh) = self.permission(grant, member, nostr).await? else {
            return Ok(Step::Waiting);
        };
        if fresh.claim().is_some() {
            bail!("execution history is missing its original promoted invoice");
        }
        let Some((round, intent)) = self.store.latest_fee_probe(grant, member).await? else {
            let joining = if let Some(presentation) = &self.presentation {
                let row = self
                    .store
                    .grant(grant)
                    .await?
                    .context("fee work lost grant")?;
                presentation
                    .joining(row.request_id, member)
                    .await
                    .unwrap_or(None)
            } else {
                None
            };
            self.store
                .prepare_fee_probe(grant, member, 0, calendar::now()?, joining.as_ref())
                .await?;
            return Ok(Step::InvoiceIntent);
        };
        let now = calendar::now()?;
        if intent.0.net_sats > self.store.policy().max_net_sats {
            return Ok(Step::Waiting);
        }
        if let Some((observed_round, observation)) =
            self.store.saved_fee_observation(grant, member).await?
        {
            if observed_round == round {
                let minimum = intent.0.recipient.min_msat.div_ceil(1000).max(
                    self.store
                        .grant_planning_policy(grant)
                        .await?
                        .minimum_net_sats,
                );
                // The journal independently enforces the frozen planning minimum
                // when persisting the observation and preparing the next round.
                match super::inclusive_fees::evaluate(
                    observation.0.request.gross_sats,
                    minimum,
                    self.store.policy().max_net_sats,
                    intent.0.net_sats,
                    observation.fee_sats(),
                    observation.total_sats(),
                )
                .map_err(anyhow::Error::msg)?
                {
                    super::inclusive_fees::Decision::Fits { .. } => {
                        if now >= observation.0.usable_until {
                            self.store
                                .close_unsubmitted(grant, member, UnsubmittedReason::Expired, now)
                                .await?;
                            return Ok(Step::Closed);
                        }
                        self.store.promote_fee_probe(grant, member, now).await?;
                        return Ok(Step::QuoteSaved);
                    }
                    super::inclusive_fees::Decision::ReduceInvoice { .. } if round < 7 => {
                        self.store
                            .prepare_fee_probe(
                                grant,
                                member,
                                round + 1,
                                now,
                                intent.0.joining.as_ref(),
                            )
                            .await?;
                        return Ok(Step::InvoiceIntent);
                    }
                    _ => {
                        self.store
                            .close_unsubmitted(grant, member, UnsubmittedReason::Unavailable, now)
                            .await?;
                        return Ok(Step::Closed);
                    }
                }
            }
            if observed_round.checked_add(1) != Some(round) {
                bail!("fee discovery round lost its predecessor");
            }
        }
        let invoice = self.store.saved_fee_probe_invoice(intent.id()).await?;
        if invoice
            .as_ref()
            .is_some_and(|i| u64::try_from(now).is_ok_and(|n| n >= i.expires_at()))
            || (invoice.is_none() && now.saturating_sub(intent.0.created_at) > 3600)
        {
            self.store
                .close_unsubmitted(grant, member, UnsubmittedReason::Expired, now)
                .await?;
            return Ok(Step::Closed);
        }
        if invoice.is_none() {
            let Some(callback) = self.store.fee_probe_callback(intent.id()).await? else {
                let raw = if intent.uses_nostr() {
                    Some(self.signer.sign(&intent).await?)
                } else {
                    None
                };
                self.store
                    .freeze_fee_probe_request(intent.id(), raw.as_deref(), calendar::now()?)
                    .await?;
                return Ok(Step::InvoiceSigned);
            };
            let invoice = fetcher.fetch(&callback).await?;
            self.store
                .freeze_fee_probe_invoice(&invoice, calendar::now()?)
                .await?;
            return Ok(Step::InvoiceSaved);
        }
        let quote = self
            .store
            .prepare_fee_probe_quote(intent.id(), calendar::now()?)
            .await?;
        if calendar::now()?
            >= quote
                .0
                .created_at
                .saturating_add(super::provider::QUOTE_LIFETIME)
        {
            self.store
                .close_unsubmitted(grant, member, UnsubmittedReason::Expired, calendar::now()?)
                .await?;
            return Ok(Step::Closed);
        }
        match self.sender.probe_fee(&quote).await? {
            FeeProbeReply::Observed(observation) => {
                self.store
                    .record_fee_observation(&observation, round, calendar::now()?)
                    .await?;
                Ok(Step::FeeObserved)
            }
            FeeProbeReply::Duplicate(duplicate) => {
                // An id-only duplicate provides no fee evidence. No execute was
                // submitted: retain its identity and release this unused share.
                self.store
                    .record_fee_probe_duplicate(&duplicate, calendar::now()?)
                    .await?;
                self.store
                    .close_unsubmitted(
                        grant,
                        member,
                        UnsubmittedReason::Unavailable,
                        calendar::now()?,
                    )
                    .await?;
                Ok(Step::Closed)
            }
        }
    }

    async fn permission(
        &self,
        grant: uuid::Uuid,
        member: &str,
        nip57: bool,
    ) -> Result<Option<crate::payout::execution::AuthenticatedState>> {
        let policy = self.store.policy();
        if !policy.execution_enabled {
            return Ok(None);
        }
        let page = self
            .core
            .execution_state(policy.scope.epoch, grant, &policy.scope.owner, member)
            .await?;
        if let Some(claim) = page.claim() {
            self.store.record_execution_claim(claim).await?;
        }
        if !page.page().page().execution_permitted(calendar::now()?)
            || (nip57 && !page.page().page().nostr_enabled)
        {
            return Ok(None);
        }
        Ok(Some(page))
    }
    async fn invoice<F: InvoiceFetcher>(
        &self,
        intent: &Intent,
        fresh: &AuthenticatedFundingPage,
        fetcher: &F,
    ) -> Result<Step> {
        let callback = self.store.callback_request(intent.id()).await?;
        if !fresh.page().execution_permitted(calendar::now()?) {
            return Ok(Step::Waiting);
        }
        let request = match callback {
            Some(request) => request,
            None => {
                let raw = if intent.uses_nostr() {
                    Some(self.signer.sign(intent).await?)
                } else {
                    None
                };
                self.store
                    .freeze_request(intent.id(), raw.as_deref(), calendar::now()?)
                    .await?;
                return Ok(Step::InvoiceSigned);
            }
        };
        let invoice = fetcher.fetch(&request).await?;
        self.store
            .freeze_invoice(&invoice, calendar::now()?)
            .await?;
        Ok(Step::InvoiceSaved)
    }
    async fn submit(
        &self,
        quote: uuid::Uuid,
        fresh: &crate::payout::execution::AuthenticatedState,
    ) -> Result<Step> {
        let permitted = self
            .store
            .prepare_execute(quote, fresh.page(), calendar::now()?)
            .await?;
        let claim = match fresh.claim() {
            Some(claim) => claim.clone(),
            None => self.core.claim_execution(permitted.claim_request()).await?,
        };
        self.store.record_execution_claim(&claim).await?;
        let observed = self.sender.execute(&permitted, &claim).await?;
        self.store.bind_payment(&observed, calendar::now()?).await?;
        Ok(Step::Submitted)
    }
}
