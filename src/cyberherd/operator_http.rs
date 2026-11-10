//! Optional exact-request NIP-98 routes. No cookie, ambient session or body key.
use super::{
    calendar,
    conversation::Context,
    http_auth::{self, Authorization},
    operator::Command,
    store::Store,
    targets::TargetPolicy,
    view_runtime::Cache,
};
use crate::core_bridge::{Availability, unix::Client};
use axum::{
    Router,
    extract::{OriginalUri, Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct Service {
    pub store: Store,
    pub owner: String,
    pub policy: TargetPolicy,
    pub timezone: String,
    pub config: http_auth::Config,
    pub core: Arc<Client>,
    pub cache: Cache,
}
pub(crate) fn router(service: Service) -> Router {
    Router::new()
        .route("/api/v1/cyberherd/me", get(me))
        .route("/api/v1/cyberherd/operator/state", get(state))
        .route("/api/v1/cyberherd/operator/templates", get(templates))
        .route("/api/v1/cyberherd/operator/command", post(command))
        .with_state(service)
        .layer(middleware::from_fn_with_state(
            Arc::new(Semaphore::new(4)),
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
    let request = if matches!(*request.method(), Method::GET | Method::HEAD) {
        let (parts, body) = request.into_parts();
        if !matches!(
            tokio::time::timeout(Duration::from_secs(1), axum::body::to_bytes(body, 0)).await,
            Ok(Ok(_))
        ) {
            return StatusCode::BAD_REQUEST.into_response();
        }
        Request::from_parts(parts, axum::body::Body::empty())
    } else {
        request
    };
    let mut response = match tokio::time::timeout(Duration::from_secs(5), next.run(request)).await {
        Ok(r) => r,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
fn header_value(headers: &HeaderMap) -> Option<&str> {
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return None;
    }
    headers.get(header::AUTHORIZATION)?.to_str().ok()
}
fn auth(
    s: &Service,
    headers: &HeaderMap,
    uri: &str,
    method: &str,
    operator: bool,
) -> anyhow::Result<Authorization> {
    let header = header_value(headers).ok_or_else(|| anyhow::anyhow!("authentication required"))?;
    if let Some(origin) = headers.get(header::ORIGIN) {
        if origin.to_str()? != s.config.public_origin {
            anyhow::bail!("wrong origin");
        }
    }
    let proof = s
        .config
        .verify(header, method, uri, &[], calendar::now()?)?;
    if operator && !s.config.operator(&proof.actor) {
        anyhow::bail!("operator required");
    }
    Ok(proof)
}
fn json(value: serde_json::Value) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) if body.len() <= 96 * 1024 => {
            ([(header::CONTENT_TYPE, "application/json")], body).into_response()
        }
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
async fn me(
    State(s): State<Service>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if uri.query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(proof) = auth(&s, &headers, &uri.to_string(), method.as_str(), false) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if s.store
        .consume_http_auth(&proof, calendar::now().unwrap_or(-1))
        .await
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(snapshot) = s.store.snapshot(&s.owner).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let m = snapshot.roster.members.get(&proof.actor);
    json(
        serde_json::json!({"pubkey":proof.actor,"operator":s.config.operator(&proof.actor),"cycle":snapshot.cycle,"revision":snapshot.revision,
        "member":{"active":m.is_some_and(|m|m.active),"banned":snapshot.roster.banned.contains(&proof.actor),"amount_sats":m.map_or(0,|m|m.amount_sats).to_string(),"lifetime_sats":m.map_or(0,|m|m.lifetime_sats).to_string(),"awards":snapshot.roster.awards.get(&proof.actor).copied().unwrap_or_default()}}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    #[serde(default)]
    offset: usize,
    revision: Option<i64>,
}
async fn state(
    State(s): State<Service>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Response {
    let Ok(proof) = auth(&s, &headers, &uri.to_string(), method.as_str(), true) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if s.store
        .consume_http_auth(&proof, calendar::now().unwrap_or(-1))
        .await
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match s
        .store
        .operator_overview(&s.owner, page.offset, page.revision)
        .await
    {
        Ok(v) => json(v),
        Err(_) => StatusCode::CONFLICT.into_response(),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplatePage {
    category: String,
    #[serde(default)]
    offset: usize,
    revision: Option<i64>,
}
async fn templates(
    State(s): State<Service>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    Query(page): Query<TemplatePage>,
) -> Response {
    let Ok(proof) = auth(&s, &headers, &uri.to_string(), method.as_str(), true) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if s.store
        .consume_http_auth(&proof, calendar::now().unwrap_or(-1))
        .await
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match s
        .store
        .operator_template_page(&s.owner, &page.category, page.offset, page.revision)
        .await
    {
        Ok(v) => json(v),
        Err(_) => StatusCode::CONFLICT.into_response(),
    }
}
async fn command(State(s): State<Service>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !matches!(
        content_type,
        "application/json" | "application/json; charset=utf-8"
    ) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Some(header) = header_value(request.headers()).map(str::to_owned) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|v| v.to_str().ok() != Some(s.config.public_origin.as_str()))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let uri = request.uri().to_string();
    let Ok(body) = axum::body::to_bytes(request.into_body(), 16384).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let Ok(now) = calendar::now() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(proof) = s.config.verify_operator(&header, &uri, &body, now) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(command) = serde_json::from_slice::<Command>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if command.validate().is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let recorded = s
        .store
        .operator_recorded(&s.owner, &proof, &command)
        .await
        .unwrap_or(false);
    let ordering = if recorded || matches!(command.action, super::operator::Action::Template { .. })
    {
        None
    } else {
        match tokio::time::timeout(
            Duration::from_secs(1),
            super::operator::CoreFence::observe(&s.store, &s.owner, &s.core),
        )
        .await
        {
            Ok(Ok(fence)) => Some(fence),
            _ => return StatusCode::CONFLICT.into_response(),
        }
    };
    let availability = if matches!(
        command.action,
        super::operator::Action::Activate { .. } | super::operator::Action::Amount { .. }
    ) {
        match tokio::time::timeout(Duration::from_secs(1), s.core.safety()).await {
            Ok(Ok(v)) => v.current_availability(calendar::now().unwrap_or(-1)),
            _ => Availability::Unknown,
        }
    } else {
        Availability::Unknown
    };
    let context = match s.store.public_targets(&s.policy, &s.timezone, now).await {
        Ok(Some(targets)) => Context {
            note: targets.iter().find(|t| t.kind == 1).cloned(),
            stream: targets.iter().find(|t| t.kind == 30311).cloned(),
        },
        _ => Context::default(),
    };
    let feeder = match availability {
        Availability::Ready => super::membership::FeederAvailability::Ready,
        Availability::Paused => super::membership::FeederAvailability::Paused,
        Availability::Unknown => super::membership::FeederAvailability::Unknown,
    };
    let now = calendar::now().unwrap_or(-1);
    match s
        .store
        .operator_command(
            &s.owner,
            &proof,
            &command,
            super::operator::Environment {
                feeder,
                context,
                ordering,
                now,
            },
        )
        .await
    {
        Ok(receipt) => {
            *s.cache.0.write().await = None;
            json(serde_json::json!(receipt))
        }
        Err(error) if error.is::<http_auth::Rejected>() => StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => StatusCode::CONFLICT.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::RuntimeMode,
        core_bridge::{
            BridgeConfig,
            unix::{Server, self_credentials},
        },
        cyberherd::{ingress::CoreBinding, membership::Settings, operator::Action},
        ledger::LedgerStore,
        nostr_event::test_event,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    fn key(n: u8) -> String {
        test_event(n, 0, 100, "", vec![]).pubkey
    }
    fn auth(actor: u8, path: &str, method: &str, body: &str) -> String {
        let mut tags = vec![
            vec!["u".into(), format!("https://example.com{path}")],
            vec!["method".into(), method.into()],
            vec!["nonce".into(), uuid::Uuid::new_v4().to_string()],
        ];
        if method == "POST" {
            tags.push(vec![
                "payload".into(),
                hex::encode(Sha256::digest(body.as_bytes())),
            ]);
        }
        format!(
            "Nostr {}",
            STANDARD.encode(
                serde_json::to_vec(&test_event(
                    actor,
                    27235,
                    calendar::now().unwrap() as u64,
                    "",
                    tags
                ))
                .unwrap()
            )
        )
    }
    #[tokio::test]
    async fn real_http_authentication_scopes_identity_commands_nonce_recovery_and_core_ordering() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::set_permissions(
            root.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o750),
        )
        .unwrap();
        let store = Store::connect(&format!(
            "sqlite://{}",
            root.path().join("members.db").display()
        ))
        .await
        .unwrap();
        store
            .initialize(
                &key(1),
                &Settings {
                    minimum_sats: 10,
                    max_members: 10,
                    member_percent: 10,
                },
                100,
            )
            .await
            .unwrap();
        let core = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        let (uid, gid) = self_credentials().unwrap();
        let config = BridgeConfig {
            socket: root.path().join("core.sock"),
            peer_uid: uid,
            group_gid: gid,
            reward_policy: None,
            payout_peer_uid: None,
            delegated_feeder_messages: false,
            payout_execution_enabled: false,
            payout_account_id: None,
        };
        let server = Server::bind(&config).await.unwrap();
        let ledger = core.clone();
        let core_task = tokio::spawn(server.run(ledger, RuntimeMode::Canary));
        store
            .initialize_core_ingress(
                &key(1),
                &CoreBinding {
                    core_uid: uid,
                    epoch: core.overlay_stream_id().await.unwrap(),
                    start_after: 0,
                    credit_pool: "herd".into(),
                    payees: std::collections::BTreeMap::from([("herd".into(), key(1))]),
                },
            )
            .await
            .unwrap();
        let service = Service {
            store: store.clone(),
            owner: key(1),
            policy: TargetPolicy {
                project_pubkey: key(1),
                tracked_tags: vec!["CyberHerd".into()],
            },
            timezone: "America/Denver".into(),
            config: http_auth::Config {
                public_origin: "https://example.com".into(),
                operators: vec![key(3)],
            },
            core: Arc::new(Client::new(config.socket, uid).unwrap()),
            cache: Cache::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = router(service);
        let http_task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let me = "/api/v1/cyberherd/me";
        assert_eq!(
            client
                .get(format!("{url}{me}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let header = auth(2, me, "GET", "");
        let response = client
            .get(format!("{url}{me}"))
            .header(header::AUTHORIZATION, &header)
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let mine: serde_json::Value = response.json().await.unwrap();
        assert_eq!(mine["pubkey"], key(2));
        assert_eq!(mine["operator"], false);
        assert!(mine.get("members").is_none());
        assert_eq!(
            client
                .get(format!("{url}{me}"))
                .header(header::AUTHORIZATION, &header)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .head(format!("{url}{me}"))
                .header(header::AUTHORIZATION, auth(2, me, "GET", ""))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .get(format!("{url}{me}"))
                .body("unexpected")
                .header(header::AUTHORIZATION, auth(2, me, "GET", ""))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let state = "/api/v1/cyberherd/operator/state?offset=0";
        assert_eq!(
            client
                .get(format!("{url}{state}"))
                .header(header::AUTHORIZATION, auth(2, state, "GET", ""))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .get(format!("{url}{state}"))
                .header(header::AUTHORIZATION, auth(3, state, "GET", ""))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let path = "/api/v1/cyberherd/operator/command";
        let command = Command {
            id: uuid::Uuid::new_v4(),
            expected_revision: 0,
            action: Action::Settings {
                minimum_sats: "20".into(),
                max_members: 10,
                member_percent: 0,
            },
        };
        let body = serde_json::to_string(&command).unwrap();
        let signed = auth(3, path, "POST", &body);
        assert_eq!(
            client
                .post(format!("{url}{path}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, &signed)
                .body(body.replace("20", "21"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .post(format!("{url}{path}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, auth(2, path, "POST", &body))
                .body(body.clone())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let send = |signature: String| {
            client
                .post(format!("{url}{path}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, signature)
                .body(body.clone())
        };
        let saved = send(signed.clone()).send().await.unwrap();
        assert_eq!(saved.status(), StatusCode::OK);
        let receipt: serde_json::Value = saved.json().await.unwrap();
        assert_eq!(receipt["revision"], 1);
        assert_eq!(
            send(signed).send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            send(auth(3, path, "POST", &body))
                .send()
                .await
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(store.snapshot(&key(1)).await.unwrap().revision, 1);
        let mut other = command.clone();
        other.id = uuid::Uuid::new_v4();
        let different = serde_json::to_string(&other).unwrap();
        assert_eq!(
            client
                .post(format!("{url}{path}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, auth(3, path, "POST", &different))
                .body(different)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(core.feed_credit_sats().await.unwrap(), 0);
        assert!(core.next_outbox_entry().await.unwrap().is_none());
        assert!(core.events_after(0, 100).await.unwrap().is_empty());
        http_task.abort();
        core_task.abort();
    }
}
