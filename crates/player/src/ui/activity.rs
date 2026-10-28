//! The user-activity state machine, as Video.js's `listenForUserActivity_`.
//!
//! Video.js polls every 250 ms; this computes the moment activity times out instead, so it needs no timer while
//! nothing is pending and is deterministic under test.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(crate) struct Activity {
    last_input: Instant,
    /// A pointer button is held.
    held: bool,
    /// The pointer is over the control bar.
    over_bar: bool,
    timeout: Duration,
}

impl Activity {
    pub(crate) fn new(timeout: Duration, now: Instant) -> Self {
        Self { last_input: now, held: false, over_bar: false, timeout }
    }

    /// Any input: pointer movement, a key, a click or an action.
    pub(crate) fn input(&mut self, now: Instant) {
        self.last_input = now;
    }

    /// A pointer button went down; the user stays active until it is released.
    pub(crate) fn press(&mut self, now: Instant) {
        self.held = true;
        self.input(now);
    }

    /// A pointer button went up.
    pub(crate) fn release(&mut self, now: Instant) {
        self.held = false;
        self.input(now);
    }

    /// The pointer left the player, which also ends a hold and leaves the control bar, as Video.js's `mouseleave`.
    pub(crate) fn leave(&mut self, now: Instant) {
        self.held = false;
        self.over_bar = false;
        self.input(now);
    }

    /// The pointer entered (`true`) or left the control bar; the user stays active while it is over it.
    pub(crate) fn bar_hover(&mut self, over: bool, now: Instant) {
        self.over_bar = over;
        self.input(now);
    }

    pub(crate) fn active(&self, now: Instant) -> bool {
        self.deadline().is_none_or(|deadline| now < deadline)
    }

    /// When the user becomes inactive without further input, or `None` if they cannot: with a zero timeout, a held
    /// button or the pointer over the control bar.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        if self.timeout.is_zero() || self.held || self.over_bar {
            None
        } else {
            Some(self.last_input + self.timeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(2);

    fn secs(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    #[test]
    fn times_out_without_input() {
        let start = Instant::now();
        let activity = Activity::new(TIMEOUT, start);
        assert!(activity.active(start));
        assert!(activity.active(secs(start, 1.99)));
        assert!(!activity.active(secs(start, 2.0)));
        assert_eq!(activity.deadline(), Some(secs(start, 2.0)));
    }

    #[test]
    fn input_reactivates() {
        let start = Instant::now();
        let mut activity = Activity::new(TIMEOUT, start);
        assert!(!activity.active(secs(start, 3.0)));
        activity.input(secs(start, 3.0));
        assert!(activity.active(secs(start, 3.0)));
        assert!(activity.active(secs(start, 4.9)));
        assert!(!activity.active(secs(start, 5.0)));
        assert_eq!(activity.deadline(), Some(secs(start, 5.0)));
    }

    #[test]
    fn hovering_the_control_bar() {
        let start = Instant::now();
        let mut activity = Activity::new(TIMEOUT, start);
        activity.bar_hover(true, start);
        assert!(activity.active(secs(start, 10.0)));
        assert_eq!(activity.deadline(), None);
        activity.bar_hover(false, secs(start, 10.0));
        assert!(activity.active(secs(start, 11.9)));
        assert!(!activity.active(secs(start, 12.0)));
    }

    #[test]
    fn holding_a_button() {
        let start = Instant::now();
        let mut activity = Activity::new(TIMEOUT, start);
        activity.press(start);
        assert!(activity.active(secs(start, 10.0)));
        assert_eq!(activity.deadline(), None);
        activity.release(secs(start, 10.0));
        assert!(activity.active(secs(start, 11.0)));
        assert!(!activity.active(secs(start, 12.0)));
    }

    #[test]
    fn leaving_ends_holds_and_hover() {
        let start = Instant::now();
        let mut activity = Activity::new(TIMEOUT, start);
        activity.press(start);
        activity.bar_hover(true, start);
        activity.leave(secs(start, 5.0));
        assert!(activity.active(secs(start, 6.9)));
        assert!(!activity.active(secs(start, 7.0)));
    }

    #[test]
    fn zero_timeout_never_times_out() {
        let start = Instant::now();
        let activity = Activity::new(Duration::ZERO, start);
        assert!(activity.active(secs(start, 3600.0)));
        assert_eq!(activity.deadline(), None);
    }
}
