use super::{AlertPolicy, AlertStore, Delivery, Observation};
use crate::{nostr::NakClient, strike::StrikeClient};
use anyhow::Result;
use std::{future::Future, time::Duration};
use tokio::{sync::watch, time::Instant};

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const DELIVERY_INTERVAL: Duration = Duration::from_secs(15);
const MAX_RETRY: Duration = Duration::from_secs(300);

/// Explicit invocation only; importing this module never activates alerting.
/// Shutdown waits for the current bounded operation to finish, preserving its
/// transaction and subprocess cleanup. Dropping the sender also requests stop.
pub async fn run_private_alert_worker(
    store: &AlertStore,
    policy: &AlertPolicy,
    strike: &StrikeClient,
    nak: &NakClient,
    shutdown: watch::Receiver<bool>,
) {
    run_loop(
        || store.poll(policy, strike, nak),
        || store.deliver_next(policy, nak),
        shutdown,
        POLL_INTERVAL,
        DELIVERY_INTERVAL,
        MAX_RETRY,
    )
    .await;
}

struct Schedule {
    due: Instant,
    interval: Duration,
    retry: Duration,
    maximum: Duration,
}
impl Schedule {
    fn new(interval: Duration, maximum: Duration) -> Self {
        Self {
            due: Instant::now(),
            interval,
            retry: interval,
            maximum,
        }
    }
    fn completed(&mut self, success: bool) {
        let delay = if success {
            self.retry = self.interval;
            self.interval
        } else {
            let delay = self.retry;
            self.retry = self.retry.saturating_mul(2).min(self.maximum);
            delay
        };
        // Schedule from completion: slow operations cannot create catch-up bursts.
        self.due = Instant::now() + delay;
    }
}

fn stopped(shutdown: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow() || shutdown.has_changed().is_err()
}

async fn run_loop<P, PF, D, DF>(
    mut poll: P,
    mut deliver: D,
    mut shutdown: watch::Receiver<bool>,
    poll_interval: Duration,
    delivery_interval: Duration,
    maximum: Duration,
) where
    P: FnMut() -> PF,
    PF: Future<Output = Result<Observation>>,
    D: FnMut() -> DF,
    DF: Future<Output = Result<Delivery>>,
{
    let mut observation = Schedule::new(poll_interval, maximum);
    let mut delivery = Schedule::new(delivery_interval, maximum);
    loop {
        if stopped(&shutdown) {
            return;
        }
        // Existing ciphertext can be delivered even when the provider is down.
        if delivery.due <= Instant::now() {
            let result = deliver().await;
            let success = matches!(result, Ok(Delivery::Empty | Delivery::Delivered));
            if !success {
                tracing::warn!("private alert delivery retry pending");
            }
            delivery.completed(success);
        }
        if stopped(&shutdown) {
            return;
        }
        if observation.due <= Instant::now() {
            let success = poll().await.is_ok();
            if !success {
                tracing::warn!("private alert observation unavailable");
            }
            observation.completed(success);
        }
        if stopped(&shutdown) {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep_until(observation.due.min(delivery.due)) => {},
            _ = shutdown.changed() => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn provider_errors_do_not_stop_delivery_and_shutdown_finishes_operation() {
        let (stop, receiver) = watch::channel(false);
        let polls = Arc::new(AtomicUsize::new(0));
        let deliveries = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        run_loop(
            || async {
                polls.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("synthetic private error");
            },
            || async {
                let attempt = deliveries.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Ok(Delivery::RetryPending);
                }
                stop.send(true).unwrap();
                tokio::task::yield_now().await;
                completed.fetch_add(1, Ordering::SeqCst);
                Ok(Delivery::Delivered)
            },
            receiver,
            Duration::from_millis(2),
            Duration::from_millis(2),
            Duration::from_millis(8),
        )
        .await;
        assert!(polls.load(Ordering::SeqCst) >= 1);
        assert_eq!(deliveries.load(Ordering::SeqCst), 2);
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn preexisting_stop_or_closed_channel_performs_no_io() {
        for closed in [false, true] {
            let (sender, receiver) = watch::channel(!closed);
            if closed {
                drop(sender);
            }
            run_loop(
                || async {
                    panic!("provider must not be contacted");
                    #[allow(unreachable_code)]
                    Ok(Observation::Queued)
                },
                || async {
                    panic!("relay must not be contacted");
                    #[allow(unreachable_code)]
                    Ok(Delivery::Delivered)
                },
                receiver,
                POLL_INTERVAL,
                DELIVERY_INTERVAL,
                MAX_RETRY,
            )
            .await;
        }
    }

    #[tokio::test]
    async fn retries_cap_reset_and_schedule_from_completion() {
        let mut schedule = Schedule::new(Duration::from_secs(2), Duration::from_secs(8));
        for expected in [2, 4, 8, 8, 8] {
            let before = Instant::now();
            schedule.completed(false);
            assert!(schedule.due >= before + Duration::from_secs(expected));
            assert!(schedule.due < before + Duration::from_secs(expected + 1));
        }
        schedule.completed(true);
        assert_eq!(schedule.retry, Duration::from_secs(2));
        schedule.due = Instant::now() - Duration::from_secs(100);
        let before = Instant::now();
        schedule.completed(false);
        assert!(schedule.due >= before + Duration::from_secs(2));
    }
}
