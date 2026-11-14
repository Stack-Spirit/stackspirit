//! Canonical UTC timestamps and the injectable clock used by pure view code.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use crate::error::SpiritError;

const RFC3339_MILLIS: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

/// A UTC instant with millisecond precision.
///
/// The wire form is RFC 3339 with a `Z` suffix and exactly three fractional digits,
/// matching VOWP, for example `2026-11-14T12:00:00.000Z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// Returns the current instant, truncated to milliseconds.
    #[must_use]
    pub fn now() -> Self {
        Self::truncated(OffsetDateTime::now_utc())
    }

    /// Builds a timestamp from Unix milliseconds.
    #[must_use]
    pub fn from_unix_millis(ms: i64) -> Self {
        let secs = ms.div_euclid(1000);
        let millis = ms.rem_euclid(1000);
        match OffsetDateTime::from_unix_timestamp(secs) {
            Ok(odt) => Self::truncated(
                odt.checked_add(time::Duration::milliseconds(millis))
                    .unwrap_or(odt),
            ),
            Err(_) => Self::from_unix_secs(if ms < 0 { 0 } else { 253_402_300_799 }),
        }
    }

    /// Builds a timestamp from whole Unix seconds.
    #[must_use]
    pub fn from_unix_secs(secs: i64) -> Self {
        match OffsetDateTime::from_unix_timestamp(secs) {
            Ok(odt) => Self::truncated(odt),
            Err(_) => Self::epoch(),
        }
    }

    /// Returns the Unix epoch.
    #[must_use]
    pub fn epoch() -> Self {
        Self::truncated(OffsetDateTime::UNIX_EPOCH)
    }

    /// Parses an RFC 3339 timestamp with any offset, normalising to UTC and truncating
    /// to milliseconds. Up to nine fractional digits are accepted.
    pub fn parse(input: &str) -> Result<Self, SpiritError> {
        if let Some(dot) = input.find('.') {
            let digits = input[dot + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            if digits > 9 {
                return Err(SpiritError::parse(
                    "timestamp",
                    "more than 9 fractional digits",
                ));
            }
        }
        let odt = OffsetDateTime::parse(input, &Rfc3339)
            .map_err(|err| SpiritError::parse("timestamp", err))?;
        Ok(Self::truncated(odt.to_offset(UtcOffset::UTC)))
    }

    /// Returns the canonical RFC 3339 wire form.
    #[must_use]
    pub fn as_rfc3339(&self) -> String {
        self.0.format(RFC3339_MILLIS).unwrap_or_default()
    }

    /// Returns whole Unix seconds.
    #[must_use]
    pub fn unix_secs(&self) -> i64 {
        self.0.unix_timestamp()
    }

    /// Returns Unix milliseconds.
    #[must_use]
    pub fn unix_millis(&self) -> i64 {
        self.0.unix_timestamp() * 1000 + i64::from(self.0.millisecond())
    }

    /// Adds a whole number of seconds.
    #[must_use]
    pub fn plus_secs(&self, secs: u64) -> Self {
        let delta = time::Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX));
        self.0.checked_add(delta).map_or(*self, Self::truncated)
    }

    /// Subtracts a whole number of seconds.
    #[must_use]
    pub fn minus_secs(&self, secs: u64) -> Self {
        let delta = time::Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX));
        self.0.checked_sub(delta).map_or(*self, Self::truncated)
    }

    /// Adds a [`std::time::Duration`].
    #[must_use]
    pub fn plus(&self, duration: Duration) -> Self {
        let delta = time::Duration::try_from(duration).unwrap_or(time::Duration::MAX);
        self.0.checked_add(delta).map_or(*self, Self::truncated)
    }

    /// Returns the elapsed time since `earlier`, saturating at zero for future instants.
    #[must_use]
    pub fn since(&self, earlier: Self) -> Duration {
        let delta = self.0 - earlier.0;
        if delta.is_negative() {
            Duration::ZERO
        } else {
            delta.try_into().unwrap_or(Duration::MAX)
        }
    }

    /// Truncates to milliseconds, converts to UTC and rejects instants outside the
    /// canonical four-digit-year range by clamping to the epoch.
    fn truncated(odt: OffsetDateTime) -> Self {
        let minimum = OffsetDateTime::from_unix_timestamp(0).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let maximum = OffsetDateTime::from_unix_timestamp(253_402_300_799).unwrap_or(minimum);
        let odt = odt.to_offset(UtcOffset::UTC).clamp(minimum, maximum);
        let nanos = odt.nanosecond();
        let truncated = nanos - nanos % 1_000_000;
        Self(odt.replace_nanosecond(truncated).unwrap_or(odt))
    }
}

impl Default for Timestamp {
    fn default() -> Self {
        Self::epoch()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_rfc3339())
    }
}

impl TryFrom<String> for Timestamp {
    type Error = SpiritError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Timestamp> for String {
    fn from(value: Timestamp) -> Self {
        value.as_rfc3339()
    }
}

/// A source of the current time, injected wherever behaviour depends on the clock.
pub trait Clock: Send + Sync {
    /// Returns the current instant.
    fn now(&self) -> Timestamp;
}

/// The real clock, backed by the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A manually advanced clock for tests. Clones share the same instant.
#[derive(Debug, Clone, Default)]
pub struct FakeClock {
    now: Arc<Mutex<Timestamp>>,
}

impl FakeClock {
    /// Creates a clock fixed at `at`.
    #[must_use]
    pub fn at(at: Timestamp) -> Self {
        Self {
            now: Arc::new(Mutex::new(at)),
        }
    }

    /// Moves the clock forward by `duration`.
    pub fn advance(&self, duration: Duration) {
        let mut guard = self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = guard.plus(duration);
    }

    /// Sets the clock to an exact instant.
    pub fn set(&self, at: Timestamp) {
        let mut guard = self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = at;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Returns `at` plus `secs`, or `None` when the addition leaves the canonical range.
#[must_use]
pub fn saturating_age_secs(now: Timestamp, earlier: Timestamp) -> u64 {
    now.since(earlier).as_secs()
}
