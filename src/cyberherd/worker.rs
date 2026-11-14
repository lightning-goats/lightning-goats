//! One ordered step. Network lookup and financial settlement never run inside a
//! membership transaction. A blocked head remains ahead of a confirmed feeding;
//! retries reuse frozen interactions and cannot move later input ahead of it.
use anyhow::{Context, Result, bail};

use super::{
    ingress::{FeedingBarrier, Input, Pending, payment_identity},
    membership::{Action, FeederAvailability, Profile, Target},
    social::{self, SocialEnvironment, SocialEvent, SocialStep},
    store::{Interaction, Receipt, Status, Store},
    targets::{AuthenticatedProfile, ConversationTarget, Targets, stream_coordinate},
};
use crate::{core_bridge::Payment, zap::ZapRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Idle,
    Waiting,
    Ignored,
    Interaction(Receipt),
    Feeding(Box<FeedingBarrier>),
}

pub struct Environment<'a> {
    pub targets: &'a Targets,
    pub discovery_available: bool,
    pub feeder: FeederAvailability,
    pub profile: Option<&'a AuthenticatedProfile>,
    pub now: i64,
}

/// Receive checkpoint and ordered work persist in one transaction. A failed
/// read/page/commit retains the previous cursor; an epoch change never resets it.
#[cfg(unix)]
pub async fn poll_core(
    store: &Store,
    owner: &str,
    client: &crate::core_bridge::unix::Client,
) -> Result<usize> {
    let cursor = store.core_cursor(owner).await?;
    let page = client
        .events(Some(cursor.binding.epoch), cursor.received_through, 128)
        .await?;
    store.ingest_core(owner, &page).await
}

pub async fn step(store: &Store, owner: &str, environment: Environment<'_>) -> Result<Step> {
    if environment.targets.owner() != owner || environment.now < 0 {
        bail!("ordered worker scope/time mismatch");
    }
    let Some(head) = store.ingress_head(owner).await? else {
        return Ok(Step::Idle);
    };
    match &head.input {
        Input::Social(id) => {
            let event = store
                .observed_event(owner, id)
                .await?
                .context("ordered social event is missing")?;
            let social = SocialEvent::parse(event, owner, environment.now)?
                .context("unsupported ordered social input")?;
            let result = social::process(
                store,
                owner,
                &social,
                SocialEnvironment {
                    targets: environment.targets,
                    discovery_available: environment.discovery_available,
                    feeder: environment.feeder,
                    profile: environment.profile,
                    now: environment.now,
                },
            )
            .await?;
            match result {
                SocialStep::AwaitingProfile => Ok(Step::Waiting),
                SocialStep::Recorded(receipt) => {
                    complete_interaction(store, owner, &head, receipt).await
                }
            }
        }
        Input::Payment(payment) => process_payment(store, owner, &head, payment, environment).await,
        Input::Feeding(_) => Ok(Step::Feeding(Box::new(
            store.freeze_feeding_barrier(owner, head.seq).await?,
        ))),
    }
}

async fn complete_interaction(
    store: &Store,
    owner: &str,
    head: &Pending,
    receipt: Receipt,
) -> Result<Step> {
    if receipt.status == Status::Deferred {
        store.defer_ingress(owner, head.seq).await?;
        Ok(Step::Waiting)
    } else {
        // A crash between the domain transaction and this checkpoint reuses
        // the durable receipt, rather than adding points/signing twice.
        store.finish_ingress(owner, head.seq, &receipt).await?;
        Ok(Step::Interaction(receipt))
    }
}

async fn process_payment(
    store: &Store,
    owner: &str,
    head: &Pending,
    payment: &Payment,
    env: Environment<'_>,
) -> Result<Step> {
    let binding = store.core_cursor(owner).await?.binding;
    let Some(zap) = payment.zap.as_ref().filter(|zap| {
        payment.credit_pool == binding.credit_pool
            && binding.payees.get(&payment.address_user) == Some(&zap.payee_pubkey)
    }) else {
        store
            .finish_ingress(owner, head.seq, &"ordinary_or_unmapped_payment")
            .await?;
        return Ok(Step::Ignored);
    };
    let request = ZapRequest::validate(
        &zap.request_json,
        &zap.payee_pubkey,
        &zap.receipt_pubkey,
        payment.amount_msat,
    )?;
    if request.event().event().pubkey == owner {
        store
            .finish_ingress(owner, head.seq, &"project_self_zap")
            .await?;
        return Ok(Step::Ignored);
    }
    let (mut target, context) = classify_zap(&request, env.targets, env.discovery_available)?;
    if env.feeder != FeederAvailability::Ready || target == Target::Unavailable {
        // Persist before a possibly failing profile lookup; paused work cannot
        // regain broad existing-member eligibility through a different retry.
        store.defer_ingress(owner, head.seq).await?;
    }
    if head.was_deferred && target == Target::ProjectContent {
        target = Target::Other;
    }
    let (source_id, source_hash) = payment_identity(payment)?;
    let pubkey = &request.event().event().pubkey;
    let action = Action::Zap {
        amount_sats: payment.amount_msat / 1000,
    };
    let interaction = match store.frozen_interaction(owner, &source_id).await? {
        Some(frozen) => {
            if frozen.source_hash != source_hash
                || frozen.pubkey != *pubkey
                || frozen.action != action
            {
                bail!("frozen zap contradicts its authoritative settlement");
            }
            frozen
        }
        None => {
            let expired_cycle = payment.paid_at < store.snapshot(owner).await?.cycle_start;
            let profile = match env.profile {
                Some(profile) if profile.event().event().pubkey == *pubkey => {
                    profile.event().require_not_future(env.now)?;
                    profile.profile().clone()
                }
                Some(_) => bail!("zap profile belongs to another signer"),
                None if expired_cycle
                    || (target == Target::Other && env.feeder == FeederAvailability::Ready) =>
                {
                    Profile::default()
                }
                None => return Ok(Step::Waiting),
            };
            Interaction {
                source_id,
                source_hash,
                pubkey: pubkey.clone(),
                profile,
                action,
                occurred_at: payment.paid_at,
            }
        }
    };
    let context = context.map(|matched| env.targets.interaction_context(matched));
    let receipt = store
        .process_with_context(owner, &interaction, target, env.feeder, context.as_ref())
        .await?;
    complete_interaction(store, owner, head, receipt).await
}

/// Unlike a relay receipt, this request has already been bound to an actual
/// settled invoice by core proof. Target authority remains a separate check.
fn classify_zap(
    request: &ZapRequest,
    targets: &Targets,
    discovery_available: bool,
) -> Result<(Target, Option<ConversationTarget>)> {
    if !discovery_available {
        return Ok((Target::Unavailable, None));
    }
    let event = request.event();
    let id = event.singleton_tag("e", false)?;
    let coordinate = event.singleton_tag("a", false)?;
    let kind = event
        .singleton_tag("k", false)?
        .map(str::parse::<u64>)
        .transpose()?;
    if let Some(id) = id {
        let Some(referenced) = targets.event(id) else {
            return Ok((Target::Unavailable, None));
        };
        if referenced.event().kind == 30311 && !targets.authenticated_stream_version(id) {
            return Ok((Target::Unavailable, None));
        }
        if !matches!(referenced.event().kind, 1 | 9041 | 30311)
            || kind.is_some_and(|kind| kind != referenced.event().kind)
            || coordinate.is_some_and(|a| stream_coordinate(referenced).ok().as_deref() != Some(a))
        {
            return Ok((Target::Other, None));
        }
    } else if coordinate.is_none() || kind.is_some_and(|kind| kind != 30311) {
        return Ok((Target::Other, None));
    }
    Ok(targets.classify(
        &id.map(str::to_owned).into_iter().collect::<Vec<_>>(),
        &coordinate
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>(),
        true,
    ))
}
