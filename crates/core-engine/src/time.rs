//! Dates and periods. Every date the engine uses comes from events (their
//! declared time or their content), never from the local clock.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month, OffsetDateTime, Weekday};

/// UTC date of a millisecond timestamp (dates out of range clamp to the epoch).
pub fn date_of(ms: i64) -> Date {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .map(|t| t.date())
        .unwrap_or(Date::MIN)
}

/// Midnight UTC of a date, in milliseconds.
pub fn ms_of(date: Date) -> i64 {
    let secs = date.midnight().assume_utc().unix_timestamp();
    secs.saturating_mul(1000)
}

pub fn add_days(date: Date, days: i64) -> Date {
    date.checked_add(Duration::days(days)).unwrap_or(date)
}

/// A reporting period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PeriodKey {
    /// No periodicity (one-off documents).
    Once,
    Month {
        year: i32,
        month: u8,
    },
    /// Week starting on this Monday.
    Week {
        monday: Date,
    },
}

impl PeriodKey {
    pub fn month(year: i32, month: u8) -> Self {
        Self::Month { year, month }
    }

    /// The week containing `date`.
    pub fn week_of(date: Date) -> Self {
        let back = i64::from(date.weekday().number_days_from_monday());
        Self::Week {
            monday: add_days(date, -back),
        }
    }

    pub fn month_of(date: Date) -> Self {
        Self::Month {
            year: date.year(),
            month: u8::from(date.month()),
        }
    }

    /// First day.
    pub fn start(&self) -> Option<Date> {
        match self {
            Self::Once => None,
            Self::Month { year, month } => {
                Date::from_calendar_date(*year, Month::try_from(*month).ok()?, 1).ok()
            }
            Self::Week { monday } => Some(*monday),
        }
    }

    /// Last day (inclusive).
    pub fn last_day(&self) -> Option<Date> {
        match self {
            Self::Once => None,
            Self::Month { .. } => self.next().and_then(|n| n.start()).map(|d| add_days(d, -1)),
            Self::Week { monday } => Some(add_days(*monday, 6)),
        }
    }

    pub fn next(&self) -> Option<Self> {
        match self {
            Self::Once => None,
            Self::Month { year, month } if *month == 12 => Some(Self::month(year + 1, 1)),
            Self::Month { year, month } => Some(Self::month(*year, month + 1)),
            Self::Week { monday } => Some(Self::Week {
                monday: add_days(*monday, 7),
            }),
        }
    }

    pub fn contains(&self, date: Date) -> bool {
        match (self.start(), self.last_day()) {
            (Some(s), Some(e)) => date >= s && date <= e,
            _ => true,
        }
    }
}

impl fmt::Display for PeriodKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Once => f.write_str("once"),
            Self::Month { year, month } => write!(f, "{year:04}-{month:02}"),
            Self::Week { monday } => write!(f, "week-{monday}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid period `{0}` (expected YYYY-MM, week-YYYY-MM-DD or once)")]
pub struct PeriodParseError(pub String);

impl FromStr for PeriodKey {
    type Err = PeriodParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || PeriodParseError(s.to_owned());
        if s == "once" {
            return Ok(Self::Once);
        }
        if let Some(rest) = s.strip_prefix("week-") {
            let d = parse_date(rest).ok_or_else(err)?;
            return Ok(Self::week_of(d));
        }
        let (y, m) = s.split_once('-').ok_or_else(err)?;
        let year: i32 = y.parse().map_err(|_| err())?;
        let month: u8 = m.parse().map_err(|_| err())?;
        if !(1..=12).contains(&month) {
            return Err(err());
        }
        Ok(Self::month(year, month))
    }
}

/// `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Option<Date> {
    let mut parts = s.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u8 = parts.next()?.parse().ok()?;
    let d: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

pub fn is_monday(date: Date) -> bool {
    date.weekday() == Weekday::Monday
}

/// RRULE weekday codes, Monday first.
pub const WEEKDAY_CODES: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

/// Days of a weekly RRULE (`FREQ=WEEKLY;BYDAY=TH,SU`); empty for other rules.
pub fn weekly_days(rrule: &str) -> Vec<Weekday> {
    let parts: Vec<&str> = rrule.split(';').collect();
    if !parts.contains(&"FREQ=WEEKLY") {
        return Vec::new();
    }
    parts
        .iter()
        .find_map(|p| p.strip_prefix("BYDAY="))
        .map(|by| by.split(',').filter_map(weekday_of_code).collect())
        .unwrap_or_default()
}

pub fn weekday_of_code(code: &str) -> Option<Weekday> {
    let i = WEEKDAY_CODES.iter().position(|c| *c == code)?;
    Some(Weekday::Monday.nth_next(i as u8))
}

/// Dates in `[from, to]` falling on one of `days`, in order.
pub fn dates_on(days: &[Weekday], from: Date, to: Date) -> Vec<Date> {
    let mut out = Vec::new();
    let mut d = from;
    while d <= to {
        if days.contains(&d.weekday()) {
            out.push(d);
        }
        d = match d.next_day() {
            Some(n) => n,
            None => break,
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn weekly_rules() {
        assert_eq!(
            weekly_days("FREQ=WEEKLY;BYDAY=TH,SU"),
            vec![Weekday::Thursday, Weekday::Sunday]
        );
        assert!(weekly_days("FREQ=MONTHLY;BYDAY=TH").is_empty());
        let nov: PeriodKey = "2026-11".parse().unwrap_or(PeriodKey::Once);
        let (Some(a), Some(b)) = (nov.start(), nov.last_day()) else {
            panic!("month bounds");
        };
        let d = dates_on(&weekly_days("FREQ=WEEKLY;BYDAY=TH,SU"), a, b);
        assert_eq!(d.len(), 9);
        assert_eq!(d[0], date!(2026 - 11 - 01));
        assert_eq!(d[1], date!(2026 - 11 - 05));
    }

    #[test]
    fn month_bounds() {
        let p: PeriodKey = "2026-02".parse().unwrap_or(PeriodKey::Once);
        assert_eq!(p.start(), Some(date!(2026 - 02 - 01)));
        assert_eq!(p.last_day(), Some(date!(2026 - 02 - 28)));
        let dec = PeriodKey::month(2026, 12);
        assert_eq!(dec.next(), Some(PeriodKey::month(2027, 1)));
        assert_eq!(dec.last_day(), Some(date!(2026 - 12 - 31)));
        assert!("2026-13".parse::<PeriodKey>().is_err());
        assert_eq!(p.to_string(), "2026-02");
    }

    #[test]
    fn weeks_start_on_monday() {
        let w = PeriodKey::week_of(date!(2026 - 11 - 05));
        assert_eq!(w.start(), Some(date!(2026 - 11 - 02)));
        assert_eq!(w.last_day(), Some(date!(2026 - 11 - 08)));
        assert_eq!("week-2026-11-05".parse::<PeriodKey>().ok(), Some(w));
    }

    #[test]
    fn millis_round_trip() {
        let d = date!(2026 - 12 - 09);
        assert_eq!(date_of(ms_of(d)), d);
        assert_eq!(date_of(ms_of(d) + 86_399_999), d);
    }
}
