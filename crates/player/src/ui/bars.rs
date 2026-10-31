//! The seek bar, volume bar and playback rate maths, as Video.js computes them.
//!
//! [`Controls`](super::Controls) uses these for its [`View`](super::View); a frontend needs only [`fraction`], to
//! turn a pointer position into the fraction the seek and volume bar methods take.

use std::time::Duration;

/// How far before the end scrubbing lands when it reaches the end, so it never ends the media.
const END_MARGIN: Duration = Duration::from_millis(100);

/// The volume the mute control restores at least, when unmuting from volume 0.
const MIN_RESTORED_VOLUME: f64 = 0.1;

/// How close a rate must be to a configured one to count as it.
const RATE_TOLERANCE: f64 = 1e-6;

/// The fraction of a bar starting at `start` and `width` long at which the pointer `x` is, clamped to `0.0..=1.0`.
/// A bar of zero width gives 0.
pub fn fraction(x: f64, start: f64, width: f64) -> f64 {
    if width <= 0.0 || !width.is_finite() {
        return 0.0;
    }
    let fraction = (x - start) / width;
    if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) }
}

/// The time at `fraction` of `duration`. The very end is moved back by 0.1 seconds, as Video.js's seek bar does, so
/// scrubbing to it never ends the media.
pub fn time_at(fraction: f64, duration: Duration) -> Duration {
    let fraction = if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) };
    let time = duration.mul_f64(fraction);
    if time >= duration { duration.saturating_sub(END_MARGIN) } else { time }
}

/// The share of `duration` that `time` covers, from 0 to 1. Zero while either is unknown or the duration is zero.
fn share(time: Option<Duration>, duration: Option<Duration>) -> f64 {
    match (time, duration) {
        (Some(time), Some(duration)) if !duration.is_zero() => {
            (time.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

/// The seek bar's played fraction: the position over the duration.
pub fn played(position: Option<Duration>, duration: Option<Duration>) -> f64 {
    share(position, duration)
}

/// The seek bar's buffered fraction: the buffered position over the duration.
pub fn buffered(buffered: Option<Duration>, duration: Option<Duration>) -> f64 {
    share(buffered, duration)
}

/// The volume bar's fraction: the volume, or 0 while muted.
pub fn volume_fraction(volume: f64, muted: bool) -> f64 {
    if muted { 0.0 } else { volume.clamp(0.0, 1.0) }
}

/// The mute control's icon level, as Video.js: 0 when muted or at volume 0, 1 below 0.33, 2 below 0.67, 3 otherwise.
pub fn volume_level(volume: f64, muted: bool) -> u8 {
    match volume {
        _ if muted || volume <= 0.0 => 0,
        v if v < 0.33 => 1,
        v if v < 0.67 => 2,
        _ => 3,
    }
}

/// The volume the mute control sets when toggled at volume 0: the last non-zero volume, but at least 0.1.
pub fn restored_volume(last_volume: f64) -> f64 {
    last_volume.max(MIN_RESTORED_VOLUME)
}

/// The index of `current` in `rates`, allowing for the tiny differences of a rate round-tripped through mpv.
pub fn rate_index(rates: &[f64], current: f64) -> Option<usize> {
    rates.iter().position(|rate| (rate - current).abs() < RATE_TOLERANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: f64) -> Option<Duration> {
        Some(Duration::from_secs_f64(seconds))
    }

    #[test]
    fn pointer_fractions() {
        assert_eq!(fraction(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction(50.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction(400.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction(150.0, 100.0, 0.0), 0.0);
        assert_eq!(fraction(150.0, 100.0, -5.0), 0.0);
        assert_eq!(fraction(f64::NAN, 100.0, 200.0), 0.0);
    }

    #[test]
    fn times_at_fractions() {
        let duration = Duration::from_secs(90);
        assert_eq!(time_at(0.5, duration), Duration::from_secs(45));
        assert_eq!(time_at(0.0, duration), Duration::ZERO);
        assert_eq!(time_at(1.0, Duration::from_secs(2)), Duration::from_millis(1900));
        assert_eq!(time_at(1.5, Duration::from_secs(2)), Duration::from_millis(1900));
        assert_eq!(time_at(-1.0, duration), Duration::ZERO);
        assert_eq!(time_at(1.0, Duration::from_millis(50)), Duration::ZERO);
    }

    #[test]
    fn played_and_buffered() {
        assert_eq!(played(secs(30.0), secs(120.0)), 0.25);
        assert_eq!(buffered(secs(60.0), secs(120.0)), 0.5);
        assert_eq!(buffered(secs(200.0), secs(120.0)), 1.0);
        assert_eq!(played(secs(30.0), None), 0.0);
        assert_eq!(played(None, secs(120.0)), 0.0);
        assert_eq!(buffered(secs(1.0), secs(0.0)), 0.0);
    }

    #[test]
    fn volume_bar() {
        assert_eq!(volume_fraction(0.6, true), 0.0);
        assert_eq!(volume_fraction(0.6, false), 0.6);
        assert_eq!(volume_level(0.6, true), 0);
        assert_eq!(volume_level(0.0, false), 0);
        assert_eq!(volume_level(0.2, false), 1);
        assert_eq!(volume_level(0.5, false), 2);
        assert_eq!(volume_level(0.9, false), 3);
        assert_eq!(volume_level(0.33, false), 2);
        assert_eq!(volume_level(0.67, false), 3);
    }

    #[test]
    fn mute_restore() {
        assert_eq!(restored_volume(0.6), 0.6);
        assert_eq!(restored_volume(0.05), 0.1);
    }

    #[test]
    fn rate_indices() {
        assert_eq!(rate_index(&[0.5, 1.0, 1.5, 2.0], 2.0), Some(3));
        assert_eq!(rate_index(&[], 1.0), None);
        // A rate mpv reports back with a tiny floating-point difference still matches its entry.
        assert_eq!(rate_index(&[0.5, 1.1, 2.0], 1.1 + 1e-9), Some(1));
        assert_eq!(rate_index(&[0.5, 2.0], 1.0), None);
    }
}
