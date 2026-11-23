use super::*;
use crate::cyberherd::payout_bridge::{Client as PayoutClient, Config, Server};
use std::os::unix::fs::PermissionsExt;

async fn serve(f: &Fixture) -> (PayoutClient, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let (uid, gid) = self_credentials().unwrap();
    std::fs::set_permissions(f._root.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
    let socket = f._root.path().join("plans.sock");
    let config = Config {
        socket: socket.clone(),
        peer_uid: uid,
        group_gid: gid,
    };
    let task = tokio::spawn(
        Server::bind(&config, &f.owner)
            .await
            .unwrap()
            .run(f.store.clone()),
    );
    (
        PayoutClient::new(socket, uid, f.owner.clone()).unwrap(),
        task,
    )
}
#[tokio::test]
async fn protected_export_waits_for_reservation_and_preserves_frozen_plan_after_restart() {
    let f = Fixture::new(300).await;
    let plan = f.freeze(&policy()).await.unwrap();
    let (client, task) = serve(&f).await;
    assert!(client.next(0).await.unwrap().is_none());
    let r = f
        .core
        .reserve_reward(
            &f.root_policy,
            plan.plan.epoch,
            f.id,
            &plan.hash,
            plan.plan.reserved_sats,
        )
        .await
        .unwrap();
    f.store
        .attach_reward_reservation(&f.owner, &approved(r.clone()).await)
        .await
        .unwrap();
    let export = client.next(0).await.unwrap().unwrap();
    assert_eq!(export.export().plan, plan.plan);
    assert_eq!(export.export().reservation.as_ref(), Some(&r));
    assert!(
        client
            .next(export.export().sequence)
            .await
            .unwrap()
            .is_none()
    );
    let uid = self_credentials().unwrap().0;
    assert!(
        PayoutClient::new(f._root.path().join("plans.sock"), uid + 1, f.owner.clone())
            .unwrap()
            .next(0)
            .await
            .is_err()
    );
    assert!(
        PayoutClient::new(f._root.path().join("plans.sock"), uid, key(2))
            .unwrap()
            .next(0)
            .await
            .is_err()
    );
    let reopened = Store::connect(&f.url).await.unwrap();
    let frozen = reopened
        .next_reward_export(&f.owner, 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frozen.hash, plan.hash);
    task.abort();
}
#[tokio::test]
async fn no_payout_plan_can_advance_without_creating_a_reservation() {
    let f = Fixture::new(1000).await;
    let plan = f.freeze(&policy()).await.unwrap();
    assert_eq!(plan.state, State::NoPayout);
    let (client, task) = serve(&f).await;
    let export = client.next(0).await.unwrap().unwrap();
    assert!(export.export().reservation.is_none());
    assert_eq!(export.export().plan.reserved_sats, 0);
    assert!(
        client
            .next(export.export().sequence)
            .await
            .unwrap()
            .is_none()
    );
    task.abort();
}

#[tokio::test]
async fn payout_reference_uses_only_real_recorded_joining_receipts_from_the_frozen_member_conversation()
 {
    let f = Fixture::with_conversations(0, true).await;
    f.store
        .seed_templates(
            &f.owner,
            &crate::cyberherd::templates::Catalog::legacy().unwrap(),
        )
        .await
        .unwrap();
    f.freeze(&policy()).await.unwrap();
    let (client, task) = serve(&f).await;
    assert!(client.joining(f.id, &key(2)).await.unwrap().is_none());
    f.store.initialize_delivery(&f.owner, 0).await.unwrap();
    while f
        .store
        .queue_next_delivery(
            &f.owner,
            &["wss://relay.damus.io/".into()],
            calendar::now().unwrap(),
        )
        .await
        .unwrap()
    {}
    while let Some(job) = f
        .store
        .reserve_delivery(&f.owner, calendar::now().unwrap())
        .await
        .unwrap()
    {
        let event = crate::nostr_event::VerifiedEvent::verify(crate::nostr_event::test_event(
            1,
            job.unsigned.kind,
            job.unsigned.created_at,
            &job.unsigned.content,
            job.unsigned.tags.clone(),
        ))
        .unwrap();
        f.store.save_delivery_signature(&job, &event).await.unwrap();
        // An unsigned intent or saved signature alone was never sufficient.
        if job
            .unsigned
            .tags
            .iter()
            .any(|t| t.first().is_some_and(|x| x == "p") && t.get(1) == Some(&key(2)))
        {
            let before = client.joining(f.id, &key(2)).await.unwrap();
            if job.unsigned.kind == 1 {
                assert!(before.is_none());
            }
        }
        let ack = crate::cyberherd::delivery_worker::RelayAck::synthetic(&event, &job.relay);
        f.store
            .finish_delivery(&job, Some(&ack), calendar::now().unwrap())
            .await
            .unwrap();
    }
    let reference = client.joining(f.id, &key(2)).await.unwrap().unwrap();
    reference.validate(&f.owner, &key(2)).unwrap();
    assert_eq!(reference.event.kind, 1);
    assert!(
        reference
            .event
            .tags
            .iter()
            .any(|t| t.first().is_some_and(|x| x == "e") && t.get(1) == Some(&"66".repeat(32)))
    );
    assert!(client.joining(Uuid::new_v4(), &key(2)).await.is_err());
    task.abort();
}
