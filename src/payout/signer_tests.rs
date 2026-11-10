use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
};

pub(crate) struct Fake {
    pub calls: Arc<AtomicUsize>,
    pub fail: bool,
    pub wrong: bool,
}
impl Backend for Fake {
    async fn sign(&self, event: UnsignedZap) -> Result<VerifiedEvent> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            bail!("synthetic signing outage");
        }
        let content = if self.wrong {
            "altered content"
        } else {
            &event.content
        };
        VerifiedEvent::verify(crate::nostr_event::test_event(
            1,
            event.kind,
            event.created_at,
            content,
            event.tags,
        ))
    }
}
pub(crate) fn intent() -> Intent {
    let member = crate::nostr_event::test_event(2, 0, 100, "", vec![]).pubkey;
    let owner = crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey;
    let receiver = crate::nostr_event::test_event(3, 0, 100, "", vec![]).pubkey;
    let address = "member@example.com";
    let recipient=super::super::recipient::Recipient::parse(address,&serde_json::to_vec(&serde_json::json!({
        "tag":"payRequest","minSendable":1000,"maxSendable":100000,"callback":"https://pay.example.com/invoice",
        "metadata":"[[\"text/plain\",\"Treats\"]]","allowsNostr":true,"nostrPubkey":receiver})).unwrap()).unwrap();
    Intent::restore(Document {
        id: uuid::Uuid::new_v4(),
        grant_id: uuid::Uuid::new_v4(),
        owner,
        member,
        address: address.into(),
        recipient,
        net_sats: 1,
        created_at: crate::cyberherd::calendar::now().unwrap(),
        relays: vec!["wss://relay.damus.io/".into()],
        content: String::new(),
        joining: None,
    })
    .unwrap()
}
fn policy(intent: &Intent) -> Policy {
    Policy {
        owner: intent.0.owner.clone(),
        client_pubkey: crate::nostr_event::test_event(4, 0, 100, "", vec![]).pubkey,
        peer_uid: ipc::uid().unwrap(),
        relays: intent.0.relays.clone(),
        max_net_sats: 100,
        max_new_per_hour: 4,
    }
}
fn fake(fail: bool, wrong: bool) -> Fake {
    Fake {
        calls: Arc::new(AtomicUsize::new(0)),
        fail,
        wrong,
    }
}

#[tokio::test]
async fn frozen_unsigned_survives_failure_and_signed_bytes_survive_restart_without_resigning() {
    let root = tempfile::TempDir::new().unwrap();
    let url = format!("sqlite://{}", root.path().join("signer.db").display());
    let intent = intent();
    let policy = policy(&intent);
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    assert!(
        store
            .sign(intent.clone(), &fake(true, false))
            .await
            .is_err()
    );
    let raw: String = sqlx::query_scalar("SELECT document_json FROM signer_intents")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(raw, serde_json::to_string(&intent.0).unwrap());
    assert!(
        store
            .sign(intent.clone(), &fake(false, true))
            .await
            .is_err()
    );
    let backend = fake(false, false);
    let signed = store.sign(intent.clone(), &backend).await.unwrap();
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let restored = Store::connect(&url, policy).await.unwrap();
    assert_eq!(
        restored
            .sign(intent.clone(), &fake(true, false))
            .await
            .unwrap(),
        signed
    );
    let mut changed = intent.clone();
    changed.0.created_at += 1;
    assert!(restored.sign(changed, &backend).await.is_err());
    Request::restore(intent, Some(signed)).unwrap();
    for query in [
        "DELETE FROM signer_intents",
        "UPDATE signer_results SET event_json='changed'",
        "DELETE FROM signer_binding",
        "UPDATE signer_clock SET observed_at=0",
    ] {
        assert!(sqlx::query(query).execute(&restored.pool).await.is_err());
    }
}

#[tokio::test]
async fn broker_checks_actual_peer_both_ends_and_cannot_sign_arbitrary_project_notes() {
    let root = tempfile::TempDir::new().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
    let intent = intent();
    let policy = policy(&intent);
    let (uid, gid) = crate::core_bridge::unix::self_credentials().unwrap();
    let store = Store::connect(
        &format!("sqlite://{}", root.path().join("signer.db").display()),
        policy.clone(),
    )
    .await
    .unwrap();
    let socket = root.path().join("broker.sock");
    let task = tokio::spawn(
        Server::bind(&socket, gid, uid)
            .await
            .unwrap()
            .run_with(store.clone(), fake(false, false)),
    );
    let client = Client::new(socket.clone(), uid).unwrap();
    let raw = client.sign(&intent).await.unwrap();
    assert_eq!(client.sign(&intent).await.unwrap(), raw);
    let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    ipc::write(
        &mut stream,
        &serde_json::json!({"method":"sign_event","kind":1,"content":"arbitrary"}),
        MAX_FRAME,
    )
    .await
    .unwrap();
    assert!(matches!(
        ipc::read::<Reply>(&mut stream, MAX_FRAME).await.unwrap(),
        Reply::Rejected
    ));
    assert!(
        Client::new(socket.clone(), uid + 1)
            .unwrap()
            .sign(&intent)
            .await
            .is_err()
    );
    task.abort();
    let mut denied = policy;
    denied.peer_uid = uid + 1;
    let other = Store::connect(
        &format!("sqlite://{}", root.path().join("other.db").display()),
        denied,
    )
    .await
    .unwrap();
    let socket = root.path().join("other.sock");
    let task = tokio::spawn(
        Server::bind(&socket, gid, uid + 1)
            .await
            .unwrap()
            .run_with(other, fake(false, false)),
    );
    assert!(
        Client::new(socket, uid)
            .unwrap()
            .sign(&intent)
            .await
            .is_err()
    );
    task.abort();
}

#[tokio::test]
async fn signer_budget_and_identity_policy_do_not_silently_expand_or_reset() {
    let root = tempfile::TempDir::new().unwrap();
    let intent = intent();
    let url = format!("sqlite://{}", root.path().join("signer.db").display());
    let mut p = policy(&intent);
    p.max_new_per_hour = 1;
    let store = Store::connect(&url, p.clone()).await.unwrap();
    store
        .sign(intent.clone(), &fake(false, false))
        .await
        .unwrap();
    let mut next = intent.clone();
    next.0.id = uuid::Uuid::new_v4();
    assert!(store.sign(next, &fake(false, false)).await.is_err());
    let mut other = p.clone();
    other.peer_uid += 1;
    assert!(Store::connect(&url, other).await.is_err());
    let mut zero = p;
    zero.max_net_sats = 0;
    let zero = Store::connect(&url, zero).await.unwrap();
    assert!(zero.sign(intent, &fake(false, false)).await.is_err());
    let core = crate::ledger::LedgerStore::connect(&format!(
        "sqlite://{}",
        root.path().join("core.db").display()
    ))
    .await
    .unwrap();
    drop(core);
    assert!(
        Store::connect(
            &format!("sqlite://{}", root.path().join("core.db").display()),
            policy(&self::intent())
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn client_credential_is_bound_to_its_public_identity_and_cannot_be_the_project_key() {
    let intent = intent();
    let p = policy(&intent);
    assert!(validate_client_key(&p, &hex::encode([4u8; 32])).is_ok());
    assert!(validate_client_key(&p, &hex::encode([1u8; 32])).is_err());
    let mut same = p;
    same.client_pubkey = same.owner.clone();
    assert!(same.validate().is_err());
}
