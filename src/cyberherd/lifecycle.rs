//! Optional broadcast notices. The observer owns no feeder/stream commands and
//! cannot create membership, reward or paid-ledger effects.
use anyhow::{Context as _, Result};
use chrono::Timelike;

use super::calendar::DayWindow;
use crate::gateway::{SolarContext, SunPhase};

pub fn evening(timezone: &str, solar: Option<&SolarContext>, now: i64) -> Result<bool> {
    let day = DayWindow::at(timezone, now)?;
    let zone: chrono_tz::Tz = timezone.parse()?;
    let local = chrono::DateTime::from_timestamp(now, 0)
        .context("invalid evening clock")?
        .with_timezone(&zone);
    if local.hour() < 12 {
        return Ok(false);
    }
    if let Some(solar) = solar.filter(|s| s.fresh(now)) {
        if matches!(solar.phase, Some(SunPhase::Dusk | SunPhase::Night)) {
            return Ok(true);
        }
        if let Some(sunset) = solar.sunset_at.filter(|s| day.contains(*s))
            && (now >= sunset
                || (sunset - now <= 3600 && solar.direct_milli_w_m2.is_some_and(|r| r <= 500)))
        {
            return Ok(true);
        }
        if solar.phase == Some(SunPhase::Daylight) || solar.direct_milli_w_m2.is_some_and(|r| r > 0)
        {
            return Ok(false);
        }
    }
    Ok(local.hour() >= 18)
}

#[cfg(unix)]
pub(crate) struct Observation<'a> {
    pub state: &'a crate::core_bridge::LifecycleContext,
    pub discovery_available: bool,
    pub timezone: &'a str,
    pub relays: &'a [String],
    pub now: i64,
}

#[cfg(all(test, unix))]
#[path = "lifecycle_tests.rs"]
mod tests;
