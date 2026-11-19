//! Shared signed NIP-65 direction/authority rules. Destination policy stays with the caller.
use crate::{VerifiedEvent, validate_public_key};
use anyhow::{Result, bail};
use std::collections::BTreeSet;

pub struct Directions {
    pub read: Vec<String>,
    pub write: Vec<String>,
}
/// `normalize` must reject unsafe destinations and write-only broadcasters.
/// This function never queries a relay and accepts only natively verified events.
pub fn directions(
    event: &VerifiedEvent,
    author: &str,
    now: i64,
    normalize: impl Fn(&str) -> Option<String>,
) -> Result<Directions> {
    validate_public_key(author)?;
    if now < 0
        || event.event().pubkey != author
        || event.event().kind != 10002
        || event.event().created_at > now as u64
    {
        bail!("relay list author, kind or time differs");
    }
    if event
        .tag_values("expiration")
        .any(|value| value.parse::<i64>().map_or(true, |time| time <= now))
    {
        bail!("relay list is expired or malformed");
    }
    let mut read = BTreeSet::new();
    let mut write = BTreeSet::new();
    for tag in &event.event().tags {
        if tag.first().map(String::as_str) != Some("r") || !(2..=3).contains(&tag.len()) {
            continue;
        }
        let Some(relay) = normalize(&tag[1]) else {
            continue;
        };
        match tag.get(2).map(String::as_str) {
            None => {
                read.insert(relay.clone());
                write.insert(relay);
            }
            Some("read") => {
                read.insert(relay);
            }
            Some("write") => {
                write.insert(relay);
            }
            _ => {}
        }
    }
    Ok(Directions {
        read: read.into_iter().take(8).collect(),
        write: write.into_iter().take(8).collect(),
    })
}
