//! Independent result reads do not wait for relays, recipients or publication.
use super::store::Store;
use crate::core_bridge::unix::Client;
use anyhow::Result;

pub struct Worker {
    pub store: Store,
    pub client: Client,
    pub owner: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Waiting,
    Saved,
}
impl Worker {
    pub async fn tick(&self) -> Result<Step> {
        let Some((reservation, after)) = self.store.next_reward_result(&self.owner).await? else {
            return Ok(Step::Idle);
        };
        let Some(page) = self.client.reward_results(&reservation, after).await? else {
            return Ok(Step::Waiting);
        };
        self.store.record_reward_results(&page).await?;
        Ok(Step::Saved)
    }
}
