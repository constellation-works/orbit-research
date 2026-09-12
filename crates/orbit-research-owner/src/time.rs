//! Registration clock and ISO 8601 instants.
//!
//! Registration time is measured here, never supplied by a caller. Comparisons use exact
//! instants so an offset-shifted timestamp cannot reorder an append chain.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{OwnerError, Result, require};

/// The measured registration clock. Tests supply a fixed clock; owners never do.
pub trait Clock: Send + Sync {
    /// Current UTC instant formatted like CPython `datetime.now(timezone.utc).isoformat()`.
    fn now(&self) -> String;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format_utc(
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
            elapsed.subsec_micros(),
        )
    }
}

/// A fixed instant, for chronology tests that must not race a real clock.
#[derive(Clone, Debug)]
pub struct FixedClock(pub String);

impl Clock for FixedClock {
    fn now(&self) -> String {
        self.0.clone()
    }
}

fn format_utc(seconds: i64, microseconds: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (remainder / 3600, (remainder % 3600) / 60, remainder % 60);
    let fraction = if microseconds == 0 {
        String::new()
    } else {
        format!(".{microseconds:06}")
    };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{fraction}+00:00")
}

/// Parse an ISO 8601 timestamp into nanoseconds since the epoch; a timezone is required.
pub fn instant(value: &str) -> Result<i128> {
    parse(value).ok_or_else(|| {
        OwnerError::Invalid(format!(
            "timestamp must be an ISO 8601 string with timezone: {value}"
        ))
    })
}

/// Compare two ISO 8601 instants, reporting the error of whichever fails to parse.
pub fn ordered(earlier: &str, later: &str) -> Result<bool> {
    Ok(instant(earlier)? <= instant(later)?)
}

fn parse(value: &str) -> Option<i128> {
    let (date, rest) = value.split_once(['T', 't', ' '])?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = two_digits(date_parts.next()?)?;
    let day: u32 = two_digits(date_parts.next()?)?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset) = split_offset(rest)?;
    let mut clock_parts = clock.split(':');
    let hour: i64 = i64::from(two_digits(clock_parts.next()?)?);
    let minute: i64 = i64::from(two_digits(clock_parts.next()?)?);
    let (second_text, fraction_text) = match clock_parts.next() {
        Some(field) => field.split_once('.').unwrap_or((field, "")),
        None => ("00", ""),
    };
    if clock_parts.next().is_some() {
        return None;
    }
    let second: i64 = i64::from(two_digits(second_text)?);
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut nanoseconds: i128 = 0;
    if !fraction_text.is_empty() {
        if !fraction_text.bytes().all(|b| b.is_ascii_digit()) || fraction_text.len() > 9 {
            return None;
        }
        let mut digits = fraction_text.to_owned();
        while digits.len() < 9 {
            digits.push('0');
        }
        nanoseconds = digits.parse::<i128>().ok()?;
    }
    let days = days_from_civil(year, month, day);
    let local = i128::from(days) * 86_400 * 1_000_000_000
        + i128::from(hour * 3600 + minute * 60 + second) * 1_000_000_000
        + nanoseconds;
    Some(local - i128::from(offset) * 1_000_000_000)
}

/// Split the timezone designator; a naive timestamp is rejected by returning `None`.
fn split_offset(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return Some((clock, 0));
    }
    let position = rest.rfind(['+', '-'])?;
    let (clock, designator) = rest.split_at(position);
    let sign = if designator.starts_with('-') { -1 } else { 1 };
    let mut fields = designator[1..].split(':');
    let hours = i64::from(two_digits(fields.next()?)?);
    let minutes = fields
        .next()
        .map_or(Some(0), |value| two_digits(value).map(i64::from))?;
    let seconds = fields.next().map_or(Some(0), |value| {
        two_digits(value.split('.').next().unwrap_or("00")).map(i64::from)
    })?;
    if fields.next().is_some() || hours > 23 || minutes > 59 {
        return None;
    }
    Some((clock, sign * (hours * 3600 + minutes * 60 + seconds)))
}

fn two_digits(value: &str) -> Option<u32> {
    if value.len() == 2 && value.bytes().all(|b| b.is_ascii_digit()) {
        value.parse().ok()
    } else {
        None
    }
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    })
    .unwrap_or(1);
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Require a non-decreasing registration clock between two measured instants.
pub fn require_monotonic(previous: &str, current: &str, message: &str) -> Result<()> {
    require(ordered(previous, current)?, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_and_fractions_compare_as_exact_instants() {
        assert_eq!(
            instant("2026-01-01T00:00:00Z").expect("utc"),
            instant("2026-01-01T01:00:00+01:00").expect("offset")
        );
        assert!(
            instant("2026-01-01T00:00:00.000001+00:00").expect("micro")
                > instant("2026-01-01T00:00:00+00:00").expect("second")
        );
    }

    #[test]
    fn naive_timestamps_are_refused() {
        assert!(instant("2020-01-01").is_err());
        assert!(instant("2020-01-01T00:00:00").is_err());
    }

    #[test]
    fn measured_now_round_trips_through_the_parser() {
        let measured = SystemClock.now();
        assert!(measured.ends_with("+00:00"), "{measured}");
        assert!(instant(&measured).is_ok(), "{measured}");
        assert_eq!(
            "2026-09-12T23:14:05.000123+00:00",
            format_utc(1_789_254_845, 123)
        );
        assert_eq!("1970-01-01T00:00:00+00:00", format_utc(0, 0));
    }
}
