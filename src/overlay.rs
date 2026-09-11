use std::{collections::VecDeque, time::Duration};

use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{Instant, MissedTickBehavior, interval};
use uuid::Uuid;

use crate::{
    ledger::{DurableEvent, LedgerStore},
    presentation::MessageRenderer,
};

const MAX_MESSAGE: usize = 16 * 1024;
const MAX_QUEUE: usize = 512 * 1024;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayResume {
    pub version: Option<u8>,
    pub stream: Option<Uuid>,
    pub after: Option<u64>,
}
impl OverlayResume {
    pub fn validate(&self) -> Result<()> {
        if self.version.is_some_and(|v| v != 1)
            || self.after.is_some_and(|seq| seq > i64::MAX as u64)
            || (self.after.is_some() && (self.version != Some(1) || self.stream.is_none()))
        {
            bail!("unsupported overlay resume request");
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Timing {
    heartbeat: Duration,
    pong: Duration,
    send: Duration,
    poll: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(25),
            pong: Duration::from_secs(10),
            send: Duration::from_secs(5),
            poll: Duration::from_millis(250),
        }
    }
}

pub async fn serve_overlay_socket(
    socket: WebSocket,
    ledger: LedgerStore,
    renderer: MessageRenderer,
    threshold_sats: u64,
    resume: OverlayResume,
) -> Result<()> {
    serve_with_timing(
        socket,
        ledger,
        renderer,
        threshold_sats,
        resume,
        Timing::default(),
    )
    .await
}

struct Queued {
    seq: u64,
    body: String,
    weather_observed_at: Option<String>,
}

fn fresh_observation(raw: Option<&str>) -> bool {
    raw.is_some_and(|time| {
        crate::gateway::now_epoch().is_ok_and(|now| {
            crate::gateway::validate_observation_time(time, now, Duration::from_secs(300)).is_ok()
        })
    })
}

fn skipped_weather(seq: u64) -> String {
    json!({"type":"event_skipped","source_type":"weather_status","seq":seq,"reason":"stale_or_invalid_observation"}).to_string()
}

impl Queued {
    fn current_body(self) -> String {
        if self.weather_observed_at.is_some()
            && !fresh_observation(self.weather_observed_at.as_deref())
        {
            skipped_weather(self.seq)
        } else {
            self.body
        }
    }
}

async fn snapshot(
    ledger: &LedgerStore,
    threshold: u64,
    stream: Uuid,
    reason: Option<&str>,
) -> Result<Queued> {
    let (seq, encoded) = ledger.overlay_snapshot_message(threshold).await?;
    let mut value: Value = serde_json::from_str(&encoded)?;
    value["version"] = 1.into();
    value["stream_id"] = stream.to_string().into();
    if let Some(reason) = reason {
        value["reset_reason"] = reason.into();
    }
    Ok(Queued {
        seq,
        body: value.to_string(),
        weather_observed_at: None,
    })
}

fn render_window(
    events: Vec<DurableEvent>,
    renderer: &MessageRenderer,
    threshold: u64,
) -> Result<VecDeque<Queued>> {
    let mut queue = VecDeque::new();
    let mut size = 0;
    for event in events {
        let seq = event.seq;
        let is_weather = event.event_type == "weather_status";
        let body = durable_event_message(event, renderer, threshold)?;
        size += body.len();
        if body.len() > MAX_MESSAGE || size > MAX_QUEUE {
            bail!("overlay replay exceeds output bounds");
        }
        let weather_observed_at = if is_weather {
            let value: Value = serde_json::from_str(&body)?;
            value["data"]["observed_at"].as_str().map(str::to_owned)
        } else {
            None
        };
        queue.push_back(Queued {
            seq,
            body,
            weather_observed_at,
        });
    }
    Ok(queue)
}

async fn send(
    sender: &mut SplitSink<WebSocket, Message>,
    message: Message,
    deadline: Duration,
) -> Result<()> {
    tokio::time::timeout(deadline, sender.send(message))
        .await
        .context("overlay send deadline exceeded")?
        .context("overlay send failed")
}

async fn serve_with_timing(
    socket: WebSocket,
    ledger: LedgerStore,
    renderer: MessageRenderer,
    threshold: u64,
    resume: OverlayResume,
    timing: Timing,
) -> Result<()> {
    resume.validate()?;
    let stream = ledger.overlay_stream_id().await?;
    let (mut sender, mut receiver) = socket.split();
    let mut checkpoint = snapshot(&ledger, threshold, stream, None).await?;
    let mut last_seq = checkpoint.seq;
    let mut queue = VecDeque::new();
    if let Some(after) = resume.after {
        let replay = if resume.stream == Some(stream) {
            ledger
                .overlay_event_window(after, checkpoint.seq)
                .await?
                .and_then(|events| render_window(events, &renderer, threshold).ok())
        } else {
            None
        };
        let replay = replay.filter(|queue| {
            queue.iter().map(|item| item.body.len()).sum::<usize>() + checkpoint.body.len()
                <= MAX_QUEUE
        });
        if let Some(replay) = replay {
            send(&mut sender, Message::Text(json!({"type":"resume","version":1,"stream_id":stream,"after":after,"through":checkpoint.seq}).to_string().into()), timing.send).await?;
            last_seq = after;
            queue = replay;
        } else {
            checkpoint = snapshot(&ledger, threshold, stream, Some("resume_unavailable")).await?;
            last_seq = checkpoint.seq;
        }
    }
    // A resumed connection sees its replay before the current-state checkpoint.
    queue.push_back(checkpoint);
    let mut ticker = interval(timing.poll);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut heartbeat = interval(timing.heartbeat);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    heartbeat.tick().await; // first heartbeat is one full interval after connect
    let mut pending_pong: Option<(Vec<u8>, Instant)> = None;
    let mut nonce = 0_u64;
    let mut input_window = Instant::now();
    let mut inputs = 0_u32;
    loop {
        tokio::select! {
            biased;
            _ = async { if let Some((_, deadline)) = &pending_pong { tokio::time::sleep_until(*deadline).await; } else { std::future::pending::<()>().await; } } => bail!("overlay pong deadline exceeded"),
            _ = heartbeat.tick() => {
                if pending_pong.is_none() {
                    nonce = nonce.wrapping_add(1);
                    let payload = nonce.to_be_bytes().to_vec();
                    send(&mut sender, Message::Ping(payload.clone().into()), timing.send).await?;
                    pending_pong = Some((payload, Instant::now() + timing.pong));
                }
            }
            incoming = receiver.next() => {
                if input_window.elapsed() >= Duration::from_secs(10) { input_window = Instant::now(); inputs = 0; }
                inputs += 1;
                if inputs > 20 { bail!("overlay input rate exceeded"); }
                match incoming {
                    Some(Ok(Message::Pong(payload))) => {
                        if pending_pong.as_ref().is_some_and(|(expected,_)| payload.as_ref() == expected.as_slice()) { pending_pong = None; }
                    }
                    Some(Ok(Message::Ping(payload))) => send(&mut sender, Message::Pong(payload), timing.send).await?,
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(Message::Text(_) | Message::Binary(_))) => bail!("overlay accepts control frames only"),
                    Some(Err(error)) => return Err(error).context("overlay receive failed"),
                }
            }
            _ = async {}, if !queue.is_empty() => {
                let next = queue.pop_front().expect("guarded queue");
                last_seq = next.seq;
                send(&mut sender, Message::Text(next.current_body().into()), timing.send).await?;
            }
            _ = ticker.tick(), if queue.is_empty() => {
                let latest = ledger.latest_event_seq().await?;
                if latest == last_seq { continue; }
                let through = latest.min(last_seq.saturating_add(100));
                let messages = ledger.overlay_event_window(last_seq, through).await?
                    .and_then(|events| render_window(events, &renderer, threshold).ok());
                match messages {
                    Some(messages) => queue = messages,
                    None => queue.push_back(snapshot(&ledger, threshold, stream, Some("event_window_unavailable")).await?),
                }
            }
        }
    }
}

fn durable_event_message(
    event: DurableEvent,
    renderer: &MessageRenderer,
    threshold_sats: u64,
) -> Result<String> {
    let mut payload: Value = serde_json::from_str(&event.payload_json)
        .context("durable overlay event contains invalid JSON")?;
    if event.event_type == "weather_status"
        && !fresh_observation(payload["data"]["observed_at"].as_str())
    {
        return Ok(skipped_weather(event.seq));
    }

    let presentation = renderer.render(&event, threshold_sats)?;
    let object = payload
        .as_object_mut()
        .context("durable overlay event payload must be a JSON object")?;

    let overlay_type = presentation
        .overlay_type
        .unwrap_or_else(|| event.event_type.clone());
    object.insert("type".to_owned(), Value::String(overlay_type));
    object.insert("source_type".to_owned(), Value::String(event.event_type));
    object.insert("seq".to_owned(), Value::from(event.seq));
    if let Some(message) = presentation.overlay_message {
        object.insert("message".to_owned(), Value::String(message));
    }
    if !presentation.overlay_goats.is_empty() {
        object.insert("goats".to_owned(), json!(presentation.overlay_goats));
    }

    serde_json::to_string(&payload).context("failed serializing overlay event")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn renderer() -> MessageRenderer {
        MessageRenderer::embedded().unwrap()
    }

    #[test]
    fn payment_event_is_presented_as_legacy_sats_received_message() {
        let message = durable_event_message(
            DurableEvent {
                seq: 42,
                event_type: "payment_received".to_owned(),
                payload_json: json!({
                    "amount_sats": 250,
                    "feed_credit_sats": 750,
                    "address_user": "dexter",
                    "type": "attacker-supplied",
                    "seq": 999
                })
                .to_string(),
            },
            &renderer(),
            1_000,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["type"], "sats_received");
        assert_eq!(value["source_type"], "payment_received");
        assert_eq!(value["seq"], 42);
        assert_eq!(value["amount_sats"], 250);
        assert!(value["message"].as_str().unwrap().contains("Dexter"));
        assert_eq!(value["goats"][0]["name"], "Dexter");
        assert_eq!(value["goats"][0]["imageUrl"], "images/dexter.png");
    }

    #[test]
    fn informational_event_is_overlay_only_message() {
        let message = durable_event_message(
            DurableEvent {
                seq: 9,
                event_type: "interface_info".to_owned(),
                payload_json: json!({}).to_string(),
            },
            &renderer(),
            1_000,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["type"], "interface_info");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
    }

    #[test]
    fn weather_payload_preserves_structured_data_and_adds_message() {
        let message = durable_event_message(
            DurableEvent {
                seq: 10,
                event_type: "weather_status".to_owned(),
                payload_json: json!({
                    "message": "Sunny and 72°F",
                    "data": {"temperature_f": 72.0, "observed_at": chrono::DateTime::from_timestamp(crate::gateway::now_epoch().unwrap(), 0).unwrap().to_rfc3339()}
                })
                .to_string(),
            },
            &renderer(),
            1_000,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(value["type"], "weather_status");
        assert_eq!(value["message"], "Sunny and 72°F");
        assert_eq!(value["data"]["temperature_f"], 72.0);
    }

    #[test]
    fn durable_event_rejects_non_object_payloads() {
        assert!(
            durable_event_message(
                DurableEvent {
                    seq: 1,
                    event_type: "bad".to_owned(),
                    payload_json: "[]".to_owned(),
                },
                &renderer(),
                1_000,
            )
            .is_err()
        );
    }
}

#[cfg(test)]
#[path = "overlay_socket_tests.rs"]
mod socket_tests;
