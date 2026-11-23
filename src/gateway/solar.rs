//! Fixed, read-only Astro context. No item names, URLs or commands cross the
//! public-host boundary. Unknown fields do not acquire daylight authority.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SunPhase {
    Daylight,
    Dusk,
    Night,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SolarContext {
    pub observed_at: i64,
    pub phase: Option<SunPhase>,
    pub sunset_at: Option<i64>,
    /// Integer milliwatts per square metre, avoiding float/NaN wire values.
    pub direct_milli_w_m2: Option<u64>,
}
impl SolarContext {
    pub fn validate(&self) -> Result<()> {
        if self.observed_at < 0
            || self.sunset_at.is_some_and(|s| s < 0)
            || self.direct_milli_w_m2.is_some_and(|r| r > 2_000_000)
        {
            bail!("invalid sanitized solar context");
        }
        Ok(())
    }
    pub fn fresh(&self, now: i64) -> bool {
        self.validate().is_ok() && (0..=90).contains(&now.saturating_sub(self.observed_at))
    }
    pub(crate) fn parse(
        phase: Option<&str>,
        sunset: Option<&str>,
        radiation: Option<&str>,
        now: i64,
    ) -> Self {
        let phase = phase.and_then(|p| match p.trim() {
            "DAYLIGHT" | "NOON" | "SUN_RISE" | "MORNING" => Some(SunPhase::Daylight),
            "SUN_SET" | "CIVIL_DUSK" | "NAUTIC_DUSK" | "ASTRO_DUSK" => Some(SunPhase::Dusk),
            "NIGHT" => Some(SunPhase::Night),
            _ => None,
        });
        let sunset_at = sunset
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s.trim()).ok())
            .map(|s| s.timestamp())
            .filter(|s| *s >= 0);
        let direct_milli_w_m2 = radiation.and_then(|r| {
            let mut parts = r.split_whitespace();
            let value = parts.next()?.parse::<f64>().ok()?;
            let unit = parts.next();
            if !value.is_finite()
                || !(0.0..=2000.0).contains(&value)
                || parts.next().is_some()
                || unit.is_some_and(|u| !matches!(u, "W/m²" | "W/m2" | "W/m^2"))
            {
                return None;
            }
            Some((value * 1000.0).round() as u64)
        });
        Self {
            observed_at: now,
            phase,
            sunset_at,
            direct_milli_w_m2,
        }
    }
}
