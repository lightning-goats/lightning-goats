//! NIP-46 signs frozen fields; only an actual matching relay OK creates a
//! joining receipt. Pause/public-mode observations are refreshed before I/O.
use super::{
    calendar,
    store::{
        Store,
        delivery_state::{Job, Unsigned},
    },
};
use crate::{
    core_bridge::{Availability, unix::Client},
    nostr::NakClient,
    nostr_event::VerifiedEvent,
};
use anyhow::{Result, bail};

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;

pub(crate) struct RelayAck {
    event_id: String,
    relay: String,
    at: i64,
}
impl RelayAck {
    #[cfg(test)]
    pub(crate) fn synthetic(event: &VerifiedEvent, relay: &str) -> Self {
        Self {
            event_id: event.event().id.clone(),
            relay: relay.into(),
            at: calendar::now().unwrap(),
        }
    }
    pub(crate) fn check(&self, event: &VerifiedEvent, relay: &str) -> Result<()> {
        if self.event_id != event.event().id
            || self.relay != relay
            || self.at < i64::try_from(event.event().created_at)?
        {
            bail!("relay acknowledgement does not match frozen event");
        }
        Ok(())
    }
    pub(crate) fn at(&self) -> i64 {
        self.at
    }
}
pub(crate) trait Transport {
    fn sign(
        &self,
        unsigned: &Unsigned,
    ) -> impl std::future::Future<Output = Result<VerifiedEvent>> + Send;
    fn publish(
        &self,
        event: &VerifiedEvent,
        relay: &str,
    ) -> impl std::future::Future<Output = Result<RelayAck>> + Send;
}
impl Transport for NakClient {
    async fn sign(&self, unsigned: &Unsigned) -> Result<VerifiedEvent> {
        self.sign_protocol_event(
            unsigned.kind,
            unsigned.created_at,
            &unsigned.content,
            unsigned.tags.clone(),
        )
        .await
    }
    async fn publish(&self, event: &VerifiedEvent, relay: &str) -> Result<RelayAck> {
        crate::zap_receipts::publish_protocol_event(event, relay).await?;
        Ok(RelayAck {
            event_id: event.event().id.clone(),
            relay: relay.into(),
            at: calendar::now()?,
        })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Idle,
    Waiting,
    Published,
}
pub(crate) struct Worker<T> {
    pub store: Store,
    pub core: Client,
    pub transport: T,
    pub owner: String,
}
impl<T: Transport> Worker<T> {
    pub async fn tick(&self) -> Result<Step> {
        let Some(mut job) = self
            .store
            .reserve_delivery(&self.owner, calendar::now()?)
            .await?
        else {
            return Ok(Step::Idle);
        };
        let result = self.deliver(&mut job).await;
        match result {
            Ok(Some(ack)) => {
                self.store
                    .finish_delivery(&job, Some(&ack), calendar::now()?)
                    .await?;
                Ok(Step::Published)
            }
            Ok(None) => {
                self.store
                    .finish_delivery(&job, None, calendar::now()?)
                    .await?;
                Ok(Step::Waiting)
            }
            Err(_) => {
                self.store
                    .finish_delivery(&job, None, calendar::now()?)
                    .await?;
                bail!("member publication held; exact intent and signed bytes retained")
            }
        }
    }
    async fn deliver(&self, job: &mut Job) -> Result<Option<RelayAck>> {
        // Idle/backoff work does not consume the core's bounded IPC admission.
        // Once selected, fresh permission still precedes signing and publication.
        if !permitted(&self.core, job.feeding).await? {
            return Ok(None);
        }
        let raw = match &job.signed {
            Some(raw) => raw.clone(),
            None => {
                let event = self.transport.sign(&job.unsigned).await?;
                self.store.save_delivery_signature(job, &event).await?
            }
        };
        let event = VerifiedEvent::from_json(&raw)?;
        job.unsigned.validate_signed(&event)?;
        if event.event().kind == 1311 && !self.store.stream_chat {
            return Ok(None);
        }
        // A failing first relay must not starve other frozen destinations.
        // Keep one signature and a bounded attempt within the 300-second lease.
        tokio::time::timeout(std::time::Duration::from_secs(180), async {
            for (index, relay) in job.remaining_relays.clone().into_iter().enumerate() {
                if index > 0 {
                    // Respect the core's one-token-per-500ms read admission.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                if !permitted(&self.core, job.feeding).await? {
                    return Ok(None);
                }
                job.relay = relay;
                if let Ok(ack) = self.transport.publish(&event, &job.relay).await {
                    ack.check(&event, &job.relay)?;
                    return Ok(Some(ack));
                }
            }
            bail!("no eligible relay acknowledged the frozen event")
        })
        .await?
    }
}
async fn permitted(core: &Client, confirmed_feeding: bool) -> Result<bool> {
    let state = core.safety().await?;
    Ok(state.nostr_enabled
        && (confirmed_feeding
            || state.current_availability(calendar::now()?) == Availability::Ready))
}
