#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

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

/// How long ago `ts_epoch` (unix seconds) was, at up to two units of
/// precision, coarsest first (e.g. "1d 6h", "1h 12m", "12m 3s", "45s"). The
/// finer unit is omitted when it would be zero (e.g. exactly one hour shows
/// "1h", not "1h 0m"); an age under a minute shows seconds alone, since
/// there is no finer unit. `None` means never updated (`ts_epoch <= 0.0`).
///
/// Distinct from [`ago`], which panels use for their single-unit "updated X
/// ago" text — that format is unchanged by this function (spec: web-ui —
/// log view relative timestamps).
#[must_use]
pub fn ago_precise(now: f64, ts_epoch: f64) -> Option<String> {
    if ts_epoch <= 0.0 {
        return None;
    }
    let age = (now - ts_epoch).max(0.0);
    if age < 1.0 {
        return Some("just now".into());
    }
    let mut secs = age as u64;
    if secs < 60 {
        return Some(format!("{secs}s"));
    }
    let days = secs / 86400;
    secs %= 86400;
    let hours = secs / 3600;
    secs %= 3600;
    let minutes = secs / 60;
    secs %= 60;

    let (major, major_unit, minor, minor_unit) = if days > 0 {
        (days, "d", hours, "h")
    } else if hours > 0 {
        (hours, "h", minutes, "m")
    } else {
        (minutes, "m", secs, "s")
    };
    Some(if minor == 0 {
        format!("{major}{major_unit}")
    } else {
        format!("{major}{major_unit} {minor}{minor_unit}")
    })
}

#[cfg(test)]
mod tests {
    use super::{ago, ago_precise};

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
        assert_eq!(
            ago(1000.0 + 3_600.0 * 3.0 + 59.0, 1000.0),
            Some("3h ago".into())
        );
    }

    #[test]
    fn rounds_down_to_days() {
        assert_eq!(
            ago(1000.0 + 86_400.0 * 2.0 + 3_600.0, 1000.0),
            Some("2d ago".into())
        );
    }

    #[test]
    fn precise_never_updated() {
        assert_eq!(ago_precise(1000.0, 0.0), None);
    }

    #[test]
    fn precise_just_now() {
        assert_eq!(ago_precise(1000.0, 999.5), Some("just now".into()));
    }

    #[test]
    fn precise_seconds_only_under_a_minute() {
        assert_eq!(ago_precise(1045.0, 1000.0), Some("45s".into()));
    }

    #[test]
    fn precise_minutes_and_seconds() {
        assert_eq!(
            ago_precise(1000.0 + 12.0 * 60.0 + 3.0, 1000.0),
            Some("12m 3s".into())
        );
    }

    #[test]
    fn precise_hours_and_minutes() {
        assert_eq!(
            ago_precise(1000.0 + 3_600.0 + 12.0 * 60.0, 1000.0),
            Some("1h 12m".into())
        );
    }

    #[test]
    fn precise_days_and_hours() {
        assert_eq!(
            ago_precise(1000.0 + 86_400.0 + 6.0 * 3_600.0, 1000.0),
            Some("1d 6h".into())
        );
    }

    #[test]
    fn precise_omits_zero_minutes() {
        assert_eq!(ago_precise(1000.0 + 3_600.0, 1000.0), Some("1h".into()));
    }

    #[test]
    fn precise_omits_zero_hours() {
        assert_eq!(ago_precise(1000.0 + 86_400.0, 1000.0), Some("1d".into()));
    }
}
