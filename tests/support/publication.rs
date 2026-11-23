//! Isolated adapter: exercise the real durable routing worker with a synthetic
//! public destination, a no-network metadata source, and the existing real/fake
//! nak loopback transport. Production uses ProtocolTransport instead.
use anyhow::Result;
use lightning_goats::{
    cyberherd::relay::{Filter, Query, ReadBatch},
    ledger::LedgerStore,
    messaging::{PublicationTransport, run_publication_worker},
    nostr::NakClient,
    nostr_event::VerifiedEvent,
    relay_routing::Router,
};
struct NoMetadata;
impl Query for NoMetadata {
    async fn query(&self, _filters: &[Filter]) -> Result<ReadBatch> {
        Ok(ReadBatch {
            events: vec![],
            completed_urls: Default::default(),
            completed_relays: 1,
            failed_relays: 0,
            saturated: false,
        })
    }
}
struct NakTransport(NakClient);
impl PublicationTransport for NakTransport {
    async fn publish(&self, event: &VerifiedEvent, _relay: &str) -> Result<()> {
        self.0.publish_signed(event.event()).await
    }
}
pub async fn run_outbox_publisher(ledger: LedgerStore, nak: NakClient) -> Result<()> {
    run_publication_worker(
        ledger,
        nak.clone(),
        Router::with_source(NoMetadata)?,
        vec!["wss://isolated.example/".into()],
        NakTransport(nak),
    )
    .await
}
