//! Time and rate labels, as Video.js writes them.

use std::time::Duration;

/// Writes a time as Video.js's `formatTime` does, with the media's duration as the `guide`.
///
/// - `m:ss`, with the minutes zero-padded to two digits when the guide is ten minutes or more.
/// - `h:mm:ss` when the time or the guide reaches one hour.
/// - An unknown time as dashes in the same number of fields: `-:-`, or `-:-:-` with hours.
///
/// Fractions of a second are dropped. An unknown guide has no effect.
///
/// ```
/// use player::ui::format_time;
/// use std::time::Duration;
///
/// let minutes = |m: u64| Some(Duration::from_secs(m * 60));
/// assert_eq!(format_time(Some(Duration::from_secs(65)), minutes(1)), "1:05");
/// assert_eq!(format_time(Some(Duration::from_secs(65)), minutes(15)), "01:05");
/// assert_eq!(format_time(Some(Duration::from_secs(65)), minutes(120)), "0:01:05");
/// assert_eq!(format_time(None, minutes(1)), "-:-");
/// ```
pub fn format_time(time: Option<Duration>, guide: Option<Duration>) -> String {
    format_seconds(time.map(|t| t.as_secs() as i64), guide.map_or(0, |g| g.as_secs() as i64))
}

/// Writes the time remaining as Video.js's `RemainingTimeDisplay` does: the whole seconds of the duration minus the
/// whole seconds of the position, written as by [`format_time`] after a `-`.
///
/// An unknown position counts as the start. While the duration is unknown or zero the label is `-0:00`, Video.js's
/// initial text.
///
/// ```
/// use player::ui::format_remaining;
/// use std::time::Duration;
///
/// let label = format_remaining(Some(Duration::from_secs_f64(12.7)), Some(Duration::from_secs_f64(90.2)));
/// assert_eq!(label, "-1:18");
/// ```
pub fn format_remaining(position: Option<Duration>, duration: Option<Duration>) -> String {
    let Some(duration) = duration.filter(|d| !d.is_zero()) else {
        return "-0:00".into();
    };
    let remaining = duration.as_secs() as i64 - position.map_or(0, |p| p.as_secs() as i64);
    format!("-{}", format_seconds(Some(remaining), duration.as_secs() as i64))
}

/// Writes a playback rate as the Video.js rate control does: the rate followed by `x`, without trailing zeros (`1x`,
/// `1.5x`).
pub fn format_rate(rate: f64) -> String {
    format!("{rate}x")
}

/// Video.js's `formatTime` default implementation, over whole seconds. Negative times are written as zero.
fn format_seconds(seconds: Option<i64>, guide: i64) -> String {
    let (guide_h, guide_m) = (guide / 3600, guide / 60 % 60);
    let Some(seconds) = seconds.map(|s| s.max(0)) else {
        return if guide_h > 0 { "-:-:-".into() } else { "-:-".into() };
    };
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 || guide_h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else if guide_m >= 10 {
        format!("{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: f64) -> Option<Duration> {
        Some(Duration::from_secs_f64(seconds))
    }

    #[test]
    fn time_labels() {
        let cases = [
            // The spec's scenarios.
            (secs(65.0), secs(90.0), "1:05"),
            (secs(65.0), secs(15.0 * 60.0), "01:05"),
            (secs(65.0), secs(2.0 * 3600.0), "0:01:05"),
            (None, secs(90.0), "-:-"),
            (None, secs(2.0 * 3600.0), "-:-:-"),
            // Video.js's own format-time tests.
            (secs(0.0), secs(0.0), "0:00"),
            (secs(1.0), secs(1.0), "0:01"),
            (secs(10.0), secs(10.0), "0:10"),
            (secs(60.0), secs(60.0), "1:00"),
            (secs(600.0), secs(600.0), "10:00"),
            (secs(3600.0), secs(3600.0), "1:00:00"),
            (secs(36000.0), secs(36000.0), "10:00:00"),
            (secs(360000.0), secs(360000.0), "100:00:00"),
            (secs(1.0), secs(10.0), "0:01"),
            (secs(1.0), secs(600.0), "00:01"),
            (secs(1.0), secs(6000.0), "0:00:01"),
            (secs(1.0), secs(60000.0), "0:00:01"),
            // Fractions are dropped, and an unknown guide has no effect.
            (secs(59.99), secs(90.0), "0:59"),
            (secs(65.0), None, "1:05"),
            (secs(3725.0), None, "1:02:05"),
            (None, None, "-:-"),
        ];
        for (time, guide, label) in cases {
            assert_eq!(format_time(time, guide), label, "{time:?} with guide {guide:?}");
        }
    }

    #[test]
    fn negative_times_are_zero() {
        assert_eq!(format_seconds(Some(-5), 90), "0:00");
        assert_eq!(format_seconds(Some(-5), 7200), "0:00:00");
    }

    #[test]
    fn remaining_labels() {
        assert_eq!(format_remaining(secs(12.7), secs(90.2)), "-1:18");
        assert_eq!(format_remaining(secs(0.0), secs(2.0 * 3600.0)), "-2:00:00");
        assert_eq!(format_remaining(None, secs(65.0)), "-1:05");
        assert_eq!(format_remaining(secs(90.0), secs(90.0)), "-0:00");
        // A position past the duration, as mpv may briefly report, is written as zero.
        assert_eq!(format_remaining(secs(91.0), secs(90.0)), "-0:00");
        assert_eq!(format_remaining(secs(10.0), None), "-0:00");
        assert_eq!(format_remaining(secs(10.0), secs(0.0)), "-0:00");
    }

    #[test]
    fn rate_labels() {
        assert_eq!(format_rate(1.0), "1x");
        assert_eq!(format_rate(1.5), "1.5x");
        assert_eq!(format_rate(0.25), "0.25x");
        assert_eq!(format_rate(2.0), "2x");
    }
}
