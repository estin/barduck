#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]

//! Shared "N ago" formatting for panel update times (web UI and TUI).

/// How long ago `ts_epoch` (unix seconds) was, rounded down to a single
/// coarse unit: seconds, minutes, hours, or days. `None` means never updated
/// (`ts_epoch <= 0.0`).
///
/// Not `humantime::format_duration`: its day/month/year units pluralize
/// ("1day", "2days"), but panels want one consistent single-letter unit.
#[must_use]
pub fn ago(now: f64, ts_epoch: f64) -> Option<String> {
    if ts_epoch <= 0.0 {
        return None;
    }
    let age = (now - ts_epoch).max(0.0);
    if age < 1.0 {
        return Some("just now".into());
    }
    let secs = age as u64;
    let (value, unit) = if secs < 60 {
        (secs, "s")
    } else if secs < 3600 {
        (secs / 60, "m")
    } else if secs < 86400 {
        (secs / 3600, "h")
    } else {
        (secs / 86400, "d")
    };
    Some(format!("{value}{unit} ago"))
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn never_updated() {
        assert_eq!(ago(1000.0, 0.0), None);
    }

    #[test]
    fn just_now() {
        assert_eq!(ago(1000.0, 999.5), Some("just now".into()));
    }

    #[test]
    fn seconds() {
        assert_eq!(ago(1012.0, 1000.0), Some("12s ago".into()));
    }

    #[test]
    fn rounds_down_to_minutes() {
        assert_eq!(ago(1629.0, 1000.0), Some("10m ago".into()));
    }

    #[test]
    fn rounds_down_to_hours() {
        assert_eq!(ago(1000.0 + 3_600.0 * 3.0 + 59.0, 1000.0), Some("3h ago".into()));
    }

    #[test]
    fn rounds_down_to_days() {
        assert_eq!(ago(1000.0 + 86_400.0 * 2.0 + 3_600.0, 1000.0), Some("2d ago".into()));
    }
}
