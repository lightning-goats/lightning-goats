//! Read-only ordered plan export. Peer credentials prove membership-process
//! provenance; core reservation/source qualification separately proves money.
use super::{rewards::Plan, store::Store};
use crate::{private_ipc as ipc, reward_funding::Reservation};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{net::UnixListener, sync::Semaphore, time::timeout};

pub const MAX_EXPORT: usize = 4 * 1024 * 1024;
const MAX_FRAME: usize = MAX_EXPORT + 4096;
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub socket: PathBuf,
    pub peer_uid: u32,
    pub group_gid: u32,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ipc::socket_path(&self.socket)?;
        if self.peer_uid == 0 || self.group_gid == 0 {
            bail!("plan export requires non-root identity/group");
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub sequence: i64,
    pub hash: String,
    pub plan: Plan,
    pub reservation: Option<Reservation>,
}
impl Export {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.plan.owner)?;
        if self.sequence <= 0
            || self.plan.version != 1
            || self.plan.hash()? != self.hash
            || self.plan.rows.len() > 1000
            || serde_json::to_vec(self)?.len() > MAX_EXPORT
        {
            bail!("invalid or oversized plan export");
        }
        if let Some(reservation) = &self.reservation {
            reservation.validate()?;
            if reservation.owner != self.plan.owner
                || reservation.epoch != self.plan.epoch
                || reservation.request_id != self.plan.request_id
                || reservation.plan_hash != self.hash
                || self.plan.reserved_sats != reservation.reserved_sats
                || self.plan.funding_hash.as_ref() != Some(&reservation.funding_hash)
            {
                bail!("export reservation contradicts frozen plan");
            }
        } else if self.plan.reserved_sats != 0 {
            bail!("payable plan is not reserved");
        }
        Ok(())
    }
}
pub struct AuthenticatedExport {
    export: Export,
    peer_uid: u32,
}
impl AuthenticatedExport {
    pub fn export(&self) -> &Export {
        &self.export
    }
    pub fn peer_uid(&self) -> u32 {
        self.peer_uid
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Call {
    Next {
        owner: String,
        after: i64,
    },
    Joining {
        owner: String,
        request_id: uuid::Uuid,
        member: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Plan {
        export: Box<Export>,
    },
    Waiting,
    Rejected,
    Joining {
        owner: String,
        request_id: uuid::Uuid,
        member: String,
        reference: Option<Box<super::conversation::PublishedReference>>,
    },
}
pub struct Server {
    listener: UnixListener,
    peer_uid: u32,
    owner: String,
}
impl Server {
    pub async fn bind(config: &Config, owner: &str) -> Result<Self> {
        config.validate()?;
        crate::nostr_event::validate_public_key(owner)?;
        Ok(Self {
            listener: ipc::bind(&config.socket, config.group_gid).await?,
            peer_uid: config.peer_uid,
            owner: owner.into(),
        })
    }
    pub async fn run(self, store: Store) -> Result<()> {
        let work = Arc::new(Semaphore::new(1));
        loop {
            let (mut stream, _) = self.listener.accept().await?;
            if stream.peer_cred()?.uid() != self.peer_uid {
                continue;
            }
            let Ok(permit) = Arc::clone(&work).try_acquire_owned() else {
                continue;
            };
            let store = store.clone();
            let owner = self.owner.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = timeout(Duration::from_secs(10), async {
                    let reply =
                        match timeout(Duration::from_secs(2), ipc::read::<Call>(&mut stream, 4096))
                            .await
                        {
                            Ok(Ok(Call::Next {
                                owner: claimed,
                                after,
                            })) if claimed == owner && after >= 0 => {
                                match store.next_reward_export(&owner, after).await {
                                    Ok(Some(export)) => Reply::Plan {
                                        export: Box::new(export),
                                    },
                                    Ok(None) => Reply::Waiting,
                                    Err(_) => Reply::Rejected,
                                }
                            }
                            Ok(Ok(Call::Joining {
                                owner: claimed,
                                request_id,
                                member,
                            })) if claimed == owner => {
                                match store.reward_plan(&owner, request_id).await {
                                    Ok(Some(plan)) => {
                                        let conversation = plan
                                            .plan
                                            .rows
                                            .iter()
                                            .find(|r| r.pubkey == member && r.skip.is_none())
                                            .and_then(|r| r.conversation.as_ref());
                                        let reference = match conversation {
                                            Some(conversation) => {
                                                store
                                                    .published_joining(
                                                        &owner,
                                                        conversation,
                                                        plan.plan.cycle,
                                                        &member,
                                                    )
                                                    .await
                                            }
                                            None => Ok(None),
                                        };
                                        match reference {
                                            Ok(reference) => Reply::Joining {
                                                owner: owner.clone(),
                                                request_id,
                                                member,
                                                reference: reference.map(Box::new),
                                            },
                                            Err(_) => Reply::Rejected,
                                        }
                                    }
                                    _ => Reply::Rejected,
                                }
                            }
                            _ => Reply::Rejected,
                        };
                    ipc::write(&mut stream, &reply, MAX_FRAME).await
                })
                .await;
            });
        }
    }
}
pub struct Client {
    socket: PathBuf,
    peer_uid: u32,
    owner: String,
}
impl Client {
    pub async fn joining(
        &self,
        request_id: uuid::Uuid,
        member: &str,
    ) -> Result<Option<super::conversation::PublishedReference>> {
        crate::nostr_event::validate_public_key(member)?;
        let reply: Reply = ipc::request(
            &self.socket,
            self.peer_uid,
            &Call::Joining {
                owner: self.owner.clone(),
                request_id,
                member: member.into(),
            },
            MAX_FRAME,
            3,
        )
        .await?;
        match reply {
            Reply::Joining {
                owner,
                request_id: actual,
                member: actual_member,
                reference,
            } if owner == self.owner && actual == request_id && actual_member == member => {
                if let Some(reference) = &reference {
                    reference.validate(&self.owner, member)?;
                }
                Ok(reference.map(|r| *r))
            }
            _ => bail!("intake rejected original joining reference"),
        }
    }
    pub fn new(socket: PathBuf, peer_uid: u32, owner: String) -> Result<Self> {
        ipc::socket_path(&socket)?;
        crate::nostr_event::validate_public_key(&owner)?;
        if peer_uid == 0 {
            bail!("intake process must be non-root");
        }
        Ok(Self {
            socket,
            peer_uid,
            owner,
        })
    }
    pub async fn next(&self, after: i64) -> Result<Option<AuthenticatedExport>> {
        if after < 0 {
            bail!("invalid export cursor");
        }
        let reply: Reply = ipc::request(
            &self.socket,
            self.peer_uid,
            &Call::Next {
                owner: self.owner.clone(),
                after,
            },
            MAX_FRAME,
            12,
        )
        .await?;
        match reply {
            Reply::Plan { export } => {
                export.validate()?;
                if export.plan.owner != self.owner || export.sequence <= after {
                    bail!("intake returned wrong owner/cursor");
                }
                Ok(Some(AuthenticatedExport {
                    export: *export,
                    peer_uid: self.peer_uid,
                }))
            }
            Reply::Waiting => Ok(None),
            Reply::Rejected => bail!("intake rejected plan export"),
            _ => bail!("intake returned a different result method"),
        }
    }
}
