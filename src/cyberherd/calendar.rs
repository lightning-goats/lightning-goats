//! Calendar-day boundaries, never fixed 24-hour increments or process-global TZ.
use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::{GapInfo, Tz};
use serde::{Deserialize, Serialize};

pub const DEFAULT_TIMEZONE: &str = "America/Denver";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayWindow {
    pub date: String,
    pub timezone: String,
    pub start: i64,
    /// Exclusive UTC boundary, including the actual DST/date-line transition.
    pub end: i64,
}

impl DayWindow {
    pub fn at(timezone: &str, now: i64) -> Result<Self> {
        if now < 0 {
            bail!("CyberHerd calendar requires a nonnegative UTC timestamp");
        }
        let zone: Tz = timezone
            .parse()
            .context("unknown CyberHerd IANA timezone")?;
        let now =
            DateTime::<Utc>::from_timestamp(now, 0).context("calendar timestamp out of range")?;
        let date = now.with_timezone(&zone).date_naive();
        let start = first_in_day(zone, date)?;
        let next = date.succ_opt().context("calendar date overflow")?;
        let end = first_in_day(zone, next)?;
        if start > now.timestamp() || end <= now.timestamp() || start >= end {
            bail!("invalid CyberHerd calendar boundaries");
        }
        Ok(Self {
            date: date.to_string(),
            timezone: zone.name().into(),
            start,
            end,
        })
    }

    #[must_use]
    pub fn contains(&self, timestamp: i64) -> bool {
        timestamp >= self.start && timestamp < self.end
    }

    pub fn validate(&self) -> Result<()> {
        let expected = Self::at(&self.timezone, self.start)?;
        if expected != *self {
            bail!("calendar window does not match the configured timezone");
        }
        Ok(())
    }
}

fn first_in_day(zone: Tz, date: NaiveDate) -> Result<i64> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .context("calendar midnight overflow")?;
    if let Some(value) = zone.from_local_datetime(&midnight).earliest() {
        return Ok(value.timestamp());
    }
    // Some zones advance at midnight, or skip a calendar date entirely. The
    // end of that gap is the first real instant, without second/minute rounding.
    GapInfo::new(&midnight, &zone)
        .and_then(|gap| gap.end)
        .map(|value| value.timestamp())
        .context("unresolvable timezone midnight gap")
}

pub fn now() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(value: &str) -> i64 {
        DateTime::parse_from_rfc3339(value).unwrap().timestamp()
    }

    #[test]
    fn mountain_days_have_exact_spring_and_autumn_boundaries() {
        let spring = DayWindow::at(DEFAULT_TIMEZONE, utc("2026-03-08T18:00:00Z")).unwrap();
        assert_eq!(spring.start, utc("2026-03-08T07:00:00Z"));
        assert_eq!(spring.end, utc("2026-03-09T06:00:00Z"));
        assert_eq!(spring.end - spring.start, 23 * 3600);
        let autumn = DayWindow::at(DEFAULT_TIMEZONE, utc("2026-11-01T18:00:00Z")).unwrap();
        assert_eq!(autumn.start, utc("2026-11-01T06:00:00Z"));
        assert_eq!(autumn.end, utc("2026-11-02T07:00:00Z"));
        assert_eq!(autumn.end - autumn.start, 25 * 3600);
        assert!(autumn.contains(autumn.start));
        assert!(!autumn.contains(autumn.end));
        spring.validate().unwrap();
        autumn.validate().unwrap();
    }

    #[test]
    fn midnight_gaps_skipped_dates_and_non_dst_zones_are_exact() {
        let chile = DayWindow::at("America/Santiago", utc("2026-09-06T15:00:00Z")).unwrap();
        assert_eq!(chile.start, utc("2026-09-06T04:00:00Z"));
        assert_eq!(chile.end - chile.start, 23 * 3600);
        let samoa = DayWindow::at("Pacific/Apia", utc("2011-12-29T20:00:00Z")).unwrap();
        assert_eq!(samoa.end, utc("2011-12-30T10:00:00Z"));
        assert_eq!(
            DayWindow::at("Pacific/Apia", samoa.end).unwrap().date,
            "2011-12-31"
        );
        let utc_day = DayWindow::at("UTC", utc("2026-11-01T18:00:00Z")).unwrap();
        assert_eq!(utc_day.end - utc_day.start, 24 * 3600);
        assert!(DayWindow::at("not/a/zone", 1).is_err());
        assert!(DayWindow::at("UTC", -1).is_err());
    }
}
