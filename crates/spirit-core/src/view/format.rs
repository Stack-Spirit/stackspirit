//! Small pure formatting helpers. The UI formats with these; the core invents no data.

use crate::time::Timestamp;

/// Formats a byte count as binary GiB with one decimal.
#[must_use]
pub fn format_bytes_gib(bytes: u64) -> String {
    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    format!("{gib:.1} GiB")
}

/// Formats an age in seconds compactly.
#[must_use]
pub fn format_age_secs(secs: u64) -> String {
    if secs < 1 {
        return "just now".to_owned();
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    let remainder = secs % 60;
    if minutes < 60 {
        return if remainder == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {remainder}s")
        };
    }
    let hours = minutes / 60;
    let minute_remainder = minutes % 60;
    if hours < 24 {
        return if minute_remainder == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minute_remainder:02}m")
        };
    }
    let days = hours / 24;
    format!("{days}d {}h", hours % 24)
}

/// Formats a percentage with one decimal.
#[must_use]
pub fn format_percent(value: f64) -> String {
    format!("{value:.1}%")
}

/// Returns the short form of a ULID-like id.
#[must_use]
pub fn short_id(id: &str) -> String {
    if id.len() <= 8 {
        id.to_owned()
    } else {
        id[..8].to_owned()
    }
}

/// Returns the local-looking clock portion of a canonical timestamp (`HH:MM:SS`).
#[must_use]
pub fn format_clock(timestamp: Timestamp) -> String {
    let canonical = timestamp.as_rfc3339();
    canonical
        .get(11..19)
        .map_or_else(|| canonical.clone(), str::to_owned)
}

/// Truncates to `limit` characters, appending an ellipsis when cut.
#[must_use]
pub fn truncate_chars(input: &str, limit: usize) -> String {
    let mut out: String = input.chars().take(limit).collect();
    if input.chars().count() > limit {
        out.push('…');
    }
    out
}
