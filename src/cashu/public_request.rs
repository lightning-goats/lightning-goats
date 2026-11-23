//! Read-only receiving metadata. Failure hides advertisement without holding credit.
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::public_request::{
    Descriptor, MAX_QUERY_BYTES, MAX_REPLY_BYTES, Operation, Query, Reply,
};
use std::time::Duration;
use tokio::net::UnixStream;
use uuid::Uuid;

pub(crate) async fn fetch(policy: &super::intake::Policy) -> Result<Option<Descriptor>> {
    let runtime_uid = crate::private_ipc::uid()?;
    ensure!(
        runtime_uid > 0 && runtime_uid != policy.wallet_uid,
        "wallet requires a separate runtime identity"
    );
    let query = Query {
        version: 1,
        operation: Operation::PublicRequest,
        wallet_instance: policy.wallet_instance,
        wallet_epoch: policy.wallet_epoch,
        request_id: Uuid::new_v4(),
    };
    // Presentation gets a short, bounded budget, independent of financial work.
    tokio::time::timeout(policy.deadline.min(Duration::from_millis(750)), async {
        let mut stream = UnixStream::connect(&policy.socket).await?;
        ensure!(
            stream.peer_cred()?.uid() == policy.wallet_uid,
            "wallet socket peer identity changed"
        );
        crate::private_ipc::write(&mut stream, &query, MAX_QUERY_BYTES).await?;
        let reply: Reply = crate::private_ipc::read(&mut stream, MAX_REPLY_BYTES).await?;
        let checked = Reply::decode(&serde_json::to_vec(&reply)?, &query)?;
        Ok::<_, anyhow::Error>(checked.descriptor)
    })
    .await
    .map_err(|_| anyhow::anyhow!("public request deadline exceeded"))?
    .map_err(|_| anyhow::anyhow!("public request unavailable or invalid"))
}

pub(crate) fn public_value(descriptor: Descriptor) -> serde_json::Value {
    serde_json::json!({"version":descriptor.version,"enabled":true,"id":descriptor.id,
        "encoding":descriptor.encoding,"request":descriptor.request})
}

#[cfg(test)]
mod cross_uid_fixture;
