//! Optional loopback-only public read API. Requests read a bounded cache and
//! cannot access store mutation, payout, feeder or signer capabilities.
use super::{
    calendar,
    store::Store,
    targets::{ConversationTarget, TargetPolicy},
    view::{Overlay, Roster},
};
use crate::core_bridge::{Availability, unix::Client};
use anyhow::{Result, bail};
use axum::{
    Router,
    extract::{Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::{RwLock, Semaphore};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub listen: SocketAddr,
    #[serde(default)]
    pub overlay: bool,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if !self.listen.ip().is_loopback() || self.listen.port() == 0 {
            bail!("CyberHerd public view requires an explicit loopback listener");
        }
        Ok(())
    }
    pub(crate) async fn start(
        &self,
        store: Store,
        policy: TargetPolicy,
        timezone: String,
        core: Client,
        tasks: &mut tokio::task::JoinSet<()>,
    ) -> Result<()> {
        self.validate()?;
        if !self.enabled {
            return Ok(());
        }
        let listener = tokio::net::TcpListener::bind(self.listen).await?;
        let cache = Cache::default();
        let app = router(cache.clone());
        tasks.spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let overlay = self.overlay;
        tasks.spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_secs(5));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                let result = async {
                    let now = calendar::now()?;
                    let roster = store.public_roster(&policy.project_pubkey).await?;
                    let targets = store.public_targets(&policy, &timezone, now).await?;
                    let availability = core
                        .safety()
                        .await
                        .map(|s| s.current_availability(calendar::now().unwrap_or(-1)))
                        .unwrap_or(Availability::Unknown);
                    *cache.0.write().await = Some(Arc::new(Report {
                        generated_at: now,
                        roster: roster.clone(),
                        targets,
                        availability,
                    }));
                    if overlay {
                        let epoch = store
                            .core_cursor(&policy.project_pubkey)
                            .await?
                            .binding
                            .epoch;
                        let message = Overlay {
                            epoch,
                            roster: roster.cards(),
                        };
                        if store.roster_needs_delivery(&message).await? {
                            let seq = core.roster_overlay(&message).await?;
                            store.acknowledge_roster(&message, seq).await?;
                        }
                    }
                    anyhow::Ok(())
                }
                .await;
                if result.is_err() {
                    tracing::warn!(
                        "public roster refresh/overlay held; no membership or financial mutation"
                    );
                }
            }
        });
        Ok(())
    }
}
#[derive(Clone, Default)]
pub(crate) struct Cache(pub Arc<RwLock<Option<Arc<Report>>>>);
#[derive(Clone, Serialize)]
pub(crate) struct Report {
    pub generated_at: i64,
    pub roster: Roster,
    pub targets: Option<Vec<ConversationTarget>>,
    pub availability: Availability,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    #[serde(default)]
    offset: usize,
    #[serde(default = "page_size")]
    limit: usize,
    revision: Option<i64>,
}
fn page_size() -> usize {
    50
}
pub(crate) fn router(cache: Cache) -> Router {
    Router::new()
        .route("/api/v1/cyberherd", get(read_view))
        .with_state(cache)
        .layer(middleware::from_fn_with_state(
            Arc::new(Semaphore::new(16)),
            bounded,
        ))
}
async fn bounded(State(slots): State<Arc<Semaphore>>, request: Request, next: Next) -> Response {
    if request.uri().query().is_some_and(|q| q.len() > 256) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(_slot) = slots.try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    match tokio::time::timeout(Duration::from_secs(2), next.run(request)).await {
        Ok(mut response) => {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
async fn read_view(State(cache): State<Cache>, Query(page): Query<Page>) -> Response {
    if page.limit == 0 || page.limit > 50 || page.offset > 1000 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(report) = cache.0.read().await.clone() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let now = calendar::now().unwrap_or(-1);
    if !(0..=15).contains(&now.saturating_sub(report.generated_at)) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if page.revision.is_some_and(|r| r != report.roster.revision) {
        return StatusCode::CONFLICT.into_response();
    }
    let start = page.offset.min(report.roster.members.len());
    let end = (start + page.limit).min(report.roster.members.len());
    let r = &report.roster;
    let roster = serde_json::json!({
        "owner":r.owner,"cycle":r.cycle,"cycle_start":r.cycle_start,
        "revision":r.revision,"capacity":r.capacity,"minimum_sats":r.minimum_sats,
        "member_percent":r.member_percent,"total_members":r.total_members,
        "members":&r.members[start..end],
        "next_offset":(end<r.members.len()).then_some(end)
    });
    let body = serde_json::to_vec(
        &serde_json::json!({"version":1,"generated_at":report.generated_at,"roster":roster,"targets":report.targets,"availability":report.availability}),
    );
    match body {
        Ok(body) if body.len() <= 96 * 1024 => {
            ([(header::CONTENT_TYPE, "application/json")], body).into_response()
        }
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cached_view_is_read_only_fresh_bounded_and_revision_pinned() {
        let root = tempfile::TempDir::new().unwrap();
        let store = Store::connect(&format!(
            "sqlite://{}",
            root.path().join("view.db").display()
        ))
        .await
        .unwrap();
        let owner = crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey;
        store
            .initialize(
                &owner,
                &super::super::membership::Settings {
                    minimum_sats: 10,
                    max_members: 100,
                    member_percent: 10,
                },
                100,
            )
            .await
            .unwrap();
        for n in 2..55 {
            let pubkey = crate::nostr_event::test_event(n, 0, 100, "", vec![]).pubkey;
            store
                .process(
                    &owner,
                    &super::super::store::Interaction {
                        source_id: format!("settlement:mock:{n}"),
                        source_hash: format!("{n:064x}"),
                        pubkey,
                        profile: super::super::membership::Profile {
                            name: format!("Member {n}"),
                            picture: None,
                            lud16: Some(format!("member{n}@example.com")),
                        },
                        action: super::super::membership::Action::Zap { amount_sats: 10 },
                        occurred_at: 100,
                    },
                    super::super::membership::Target::Current,
                    super::super::membership::FeederAvailability::Ready,
                )
                .await
                .unwrap();
        }
        let cache = Cache::default();
        let before = store.snapshot(&owner).await.unwrap();
        let report = Report {
            generated_at: calendar::now().unwrap(),
            roster: store.public_roster(&owner).await.unwrap(),
            targets: None,
            availability: Availability::Unknown,
        };
        *cache.0.write().await = Some(Arc::new(report.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = router(cache.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{url}/api/v1/cyberherd"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let page: serde_json::Value = response.json().await.unwrap();
        assert_eq!(page["roster"]["members"].as_array().unwrap().len(), 50);
        assert_eq!(page["roster"]["next_offset"], 50);
        assert_eq!(page["targets"], serde_json::Value::Null);
        let last: serde_json::Value = client
            .get(format!(
                "{url}/api/v1/cyberherd?offset=50&revision={}",
                before.revision
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(last["roster"]["members"].as_array().unwrap().len(), 3);
        assert_eq!(last["roster"]["next_offset"], serde_json::Value::Null);
        for query in ["limit=51", "limit=0", "owner=other", "offset=1001"] {
            assert_eq!(
                client
                    .get(format!("{url}/api/v1/cyberherd?{query}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            client
                .get(format!("{url}/api/v1/cyberherd?revision=0"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            client
                .post(format!("{url}/api/v1/cyberherd"))
                .body("reset")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            client
                .post(format!("{url}/api/v1/operator/reset"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let raw = page.to_string();
        for forbidden in [
            "source_id",
            "source_hash",
            "lud16",
            "request_json",
            "bunker",
            "credential",
            "payment_id",
        ] {
            assert!(!raw.contains(forbidden));
        }
        let mut stale = report;
        stale.generated_at -= 16;
        *cache.0.write().await = Some(Arc::new(stale));
        assert_eq!(
            client
                .get(format!("{url}/api/v1/cyberherd"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(store.snapshot(&owner).await.unwrap(), before);
        task.abort();
    }
    #[test]
    fn public_view_rejects_unbound_or_trusted_network_listeners() {
        for address in ["0.0.0.0:3340", "10.8.0.6:3340", "127.0.0.1:0"] {
            assert!(
                Config {
                    enabled: false,
                    listen: address.parse().unwrap(),
                    overlay: false
                }
                .validate()
                .is_err()
            );
        }
        Config {
            enabled: false,
            listen: "127.0.0.1:3340".parse().unwrap(),
            overlay: true,
        }
        .validate()
        .unwrap();
    }
}
