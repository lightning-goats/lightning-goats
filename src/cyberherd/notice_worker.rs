//! Exact ordered broadcast delivery, independent of member reply anchors.
use super::{
    calendar,
    delivery_worker::{RelayAck, Step, Transport},
    store::{Store, notice_state::Job},
};
use crate::{core_bridge::unix::Client, nostr_event::VerifiedEvent};
use anyhow::{Result, bail};

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
            .reserve_notice(&self.owner, calendar::now()?)
            .await?
        else {
            return Ok(Step::Idle);
        };
        let result = self.deliver(&mut job).await;
        self.store
            .finish_notice(
                &job,
                result.as_ref().ok().and_then(|a| a.as_ref()),
                calendar::now()?,
            )
            .await?;
        match result {
            Ok(Some(_)) => Ok(Step::Published),
            Ok(None) => Ok(Step::Waiting),
            Err(_) => bail!("broadcast delivery held; exact notice retained"),
        }
    }
    async fn permitted(&self, job: &Job) -> Result<bool> {
        let now = calendar::now()?;
        let state = self.core.lifecycle_context().await?;
        let Some(paused) = state.confirmed_override(now) else {
            return Ok(false);
        };
        if !state.nostr_enabled {
            return Ok(false);
        }
        if job.purpose == "invitation" {
            if paused {
                return Ok(false);
            }
            return self.store.notice_invitation_live(job, now).await;
        }
        // Ordered historical transitions may be delivered after a subsequent
        // transition. Unlike membership updates, a pause must publish while ON.
        Ok(true)
    }
    async fn deliver(&self, job: &mut Job) -> Result<Option<RelayAck>> {
        if !self.permitted(job).await? {
            return Ok(None);
        }
        let raw = match &job.signed {
            Some(raw) => raw.clone(),
            None => {
                let event = self.transport.sign(&job.unsigned).await?;
                self.store.save_notice_signature(job, &event).await?
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
                if !self.permitted(job).await? {
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
