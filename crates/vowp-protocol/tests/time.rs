//! Timestamp, clock and log-line tests. Spec: `docs/design/crates/vowp-protocol.md` §6.

use std::time::Duration;

use pretty_assertions::assert_eq;
use proptest::prelude::*;
use vowp_protocol::prelude::*;

#[test]
fn timestamp_roundtrip_and_ms_truncation() {
    let parsed = Timestamp::parse("2026-11-08T19:25:47.123456Z").unwrap();
    assert_eq!(parsed.as_rfc3339(), "2026-11-08T19:25:47.123Z");

    // Truncation, not rounding.
    let parsed = Timestamp::parse("2026-11-08T19:25:47.999999Z").unwrap();
    assert_eq!(parsed.as_rfc3339(), "2026-11-08T19:25:47.999Z");

    let ms = 1_760_000_000_123_i64;
    assert_eq!(Timestamp::from_unix_millis(ms).unix_millis(), ms);
    assert_eq!(Timestamp::from_unix_millis(ms).unix_secs(), 1_760_000_000);
    assert_eq!(Timestamp::from_unix_secs(1_760_000_000).unix_secs(), 1_760_000_000);

    let negative = -1_500_i64;
    assert_eq!(Timestamp::from_unix_millis(negative).unix_millis(), negative);

    let ts = Timestamp::parse("2026-11-08T19:25:47.000Z").unwrap();
    assert_eq!(ts.to_string(), "2026-11-08T19:25:47.000Z");
    assert_eq!(String::from(ts), "2026-11-08T19:25:47.000Z");

    // Serde uses the canonical string form and validates on the way in.
    let json = serde_json::to_string(&ts).unwrap();
    assert_eq!(json, "\"2026-11-08T19:25:47.000Z\"");
    assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), ts);
    assert!(serde_json::from_str::<Timestamp>("\"not a timestamp\"").is_err());
    assert!(Timestamp::parse("2026-13-08T19:25:47Z").is_err());
    assert!(Timestamp::parse("2026-11-08T19:25:47.1234567890Z").is_err());
}

#[test]
fn timestamp_normalises_offsets() {
    let with_offset = Timestamp::parse("2026-11-08T21:25:47+02:00").unwrap();
    let utc = Timestamp::parse("2026-11-08T19:25:47.000Z").unwrap();
    assert_eq!(with_offset, utc);
    assert_eq!(with_offset.as_rfc3339(), "2026-11-08T19:25:47.000Z");

    let negative_offset = Timestamp::parse("2026-11-08T14:25:47-05:00").unwrap();
    assert_eq!(negative_offset, utc);
}

proptest! {
    #[test]
    fn timestamp_ordering_matches_strings(a in 0_i64..4_000_000_000_000, b in 0_i64..4_000_000_000_000) {
        let left = Timestamp::from_unix_millis(a);
        let right = Timestamp::from_unix_millis(b);
        prop_assert_eq!(left.cmp(&right), left.as_rfc3339().cmp(&right.as_rfc3339()));
        prop_assert_eq!(left == right, left.as_rfc3339() == right.as_rfc3339());
        prop_assert_eq!(left.as_rfc3339().len(), 24);
    }
}

#[test]
fn fake_clock_advances() {
    let start = Timestamp::parse("2026-11-08T19:25:47.123Z").unwrap();
    let clock = FakeClock::at(start);
    assert_eq!(clock.now(), start);

    let clone = clock.clone();
    clone.advance(Duration::from_secs(90));
    assert_eq!(clock.now(), start.plus(Duration::from_secs(90)));
    assert_eq!(clone.now(), start.plus(Duration::from_secs(90)));

    let later = start.plus(Duration::from_secs(3600));
    clock.set(later);
    assert_eq!(clone.now(), later);

    assert_eq!(start.since(start), Duration::ZERO);
    assert_eq!(later.since(start), Duration::from_secs(3600));
    assert_eq!(start.since(later), Duration::ZERO);
}

#[test]
fn log_format_line() {
    let ts = Timestamp::parse("2026-11-08T19:25:47.123Z").unwrap();
    assert_eq!(
        format_log_line(ts, Level::Info, "l1", "job n04-graph: running -> gating"),
        "2026-11-08T19:25:47.123Z INFO l1 job n04-graph: running -> gating"
    );
    assert_eq!(Level::Debug.as_str(), "DEBUG");
    assert_eq!(Level::Info.as_str(), "INFO");
    assert_eq!(Level::Warn.as_str(), "WARN");
    assert_eq!(Level::Error.as_str(), "ERROR");

    let log = MemoryLog::new();
    log.log(Level::Info, "l1", "running -> gating");
    log.log(Level::Warn, "l6", "probe failed");
    let copy = log.clone();
    copy.log(Level::Error, "l1", "parked");
    assert_eq!(
        log.lines(),
        vec![
            (Level::Info, "l1".to_owned(), "running -> gating".to_owned()),
            (Level::Warn, "l6".to_owned(), "probe failed".to_owned()),
            (Level::Error, "l1".to_owned(), "parked".to_owned()),
        ]
    );
    assert!(log.contains("probe failed"));
    assert!(log.contains("l6"));
    assert!(!log.contains("credits"));

    let null = NullLog;
    null.log(Level::Info, "l1", "discarded");
}
