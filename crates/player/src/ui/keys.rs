//! Keys, as the frontend reports them, and the Video.js key map.

use crate::ui::action::Action;
use crate::ui::options::Hotkeys;
use std::time::Duration;

/// A key, translated by the frontend from its toolkit's key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// A key that types this character, such as `'f'` or `'3'`. Letters match in either case.
    Char(char),
    /// The space bar.
    Space,
    /// Enter or Return.
    Enter,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
}

/// The modifier keys held with a [`Key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Control.
    pub ctrl: bool,
    /// Alt, or Option on macOS.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// The platform key: Cmd on macOS, Super or Win elsewhere.
    pub platform: bool,
}

impl Modifiers {
    /// No modifier held.
    pub const NONE: Self = Self { ctrl: false, alt: false, shift: false, platform: false };
}

/// Where keyboard focus is inside the player, which decides what a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Focus {
    /// The player, or a control that is not a slider.
    Player,
    /// The seek bar.
    SeekBar,
    /// The volume bar.
    VolumeBar,
}

/// How far the seek bar's arrow keys move, Video.js's `stepSeconds`.
const STEP_SECONDS: f64 = 5.0;

/// How much further Page Up and Page Down move than the arrows, Video.js's `pageMultiplier`.
const PAGE_MULTIPLIER: f64 = 12.0;

/// How much the volume bar's arrow keys change the volume.
const VOLUME_STEP: f64 = 0.1;

/// The action a key performs, or `None` if it is unhandled and should go to the application. `duration` is the
/// media's, if known.
///
/// In order:
/// 1. Keys held with Ctrl, Alt or the platform key are unhandled, so application shortcuts keep working.
/// 2. With [`Focus::SeekBar`], as Video.js's seek bar: Left/Down and Right/Up seek 5 seconds back and forward,
///    Page Down/Page Up 60 seconds, Home and End go to the start and end, the digits `0`-`9` to 0-90 % of the
///    duration, and Space or Enter toggles play. End and the digits are unhandled while the duration is unknown.
/// 3. With [`Focus::VolumeBar`]: Left/Down and Right/Up change the volume by 0.1.
/// 4. The [`Hotkeys`], if they are enabled. Letters match in either case.
pub fn map(
    key: Key,
    modifiers: Modifiers,
    focus: Focus,
    hotkeys: &Hotkeys,
    duration: Option<Duration>,
) -> Option<Action> {
    if modifiers.ctrl || modifiers.alt || modifiers.platform {
        return None;
    }
    let slider = match focus {
        Focus::SeekBar => seek_bar(key, duration),
        Focus::VolumeBar => volume_bar(key),
        Focus::Player => None,
    };
    slider.or_else(|| hotkey(key, hotkeys))
}

fn seek_bar(key: Key, duration: Option<Duration>) -> Option<Action> {
    let action = match key {
        Key::Left | Key::Down => Action::SeekBy(-STEP_SECONDS),
        Key::Right | Key::Up => Action::SeekBy(STEP_SECONDS),
        Key::PageDown => Action::SeekBy(-STEP_SECONDS * PAGE_MULTIPLIER),
        Key::PageUp => Action::SeekBy(STEP_SECONDS * PAGE_MULTIPLIER),
        Key::Home => Action::SeekTo(Duration::ZERO),
        Key::End => Action::SeekTo(duration?),
        Key::Char(digit @ '0'..='9') => {
            let tenths = digit.to_digit(10)?;
            Action::SeekTo(duration?.mul_f64(f64::from(tenths) / 10.0))
        }
        Key::Space | Key::Enter => Action::TogglePlay,
        _ => return None,
    };
    Some(action)
}

fn volume_bar(key: Key) -> Option<Action> {
    match key {
        Key::Left | Key::Down => Some(Action::StepVolume(-VOLUME_STEP)),
        Key::Right | Key::Up => Some(Action::StepVolume(VOLUME_STEP)),
        _ => None,
    }
}

fn hotkey(key: Key, hotkeys: &Hotkeys) -> Option<Action> {
    if !hotkeys.is_enabled() {
        return None;
    }
    let matches = |keys: &[Key]| keys.iter().any(|k| same_key(*k, key));
    if matches(hotkeys.fullscreen_keys()) {
        Some(Action::ToggleFullscreen)
    } else if matches(hotkeys.mute_keys()) {
        Some(Action::ToggleMute)
    } else if matches(hotkeys.play_pause_keys()) {
        Some(Action::TogglePlay)
    } else {
        None
    }
}

/// Keys are equal, with characters compared case-insensitively.
fn same_key(a: Key, b: Key) -> bool {
    match (a, b) {
        (Key::Char(a), Key::Char(b)) => a.to_lowercase().eq(b.to_lowercase()),
        (a, b) => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATION: Option<Duration> = Some(Duration::from_secs(200));

    fn player(key: Key) -> Option<Action> {
        map(key, Modifiers::NONE, Focus::Player, &Hotkeys::default(), DURATION)
    }

    fn seek_bar(key: Key) -> Option<Action> {
        map(key, Modifiers::NONE, Focus::SeekBar, &Hotkeys::default(), DURATION)
    }

    fn volume_bar(key: Key) -> Option<Action> {
        map(key, Modifiers::NONE, Focus::VolumeBar, &Hotkeys::default(), DURATION)
    }

    #[test]
    fn global_hotkeys() {
        assert_eq!(player(Key::Char('f')), Some(Action::ToggleFullscreen));
        assert_eq!(player(Key::Char('m')), Some(Action::ToggleMute));
        assert_eq!(player(Key::Char('k')), Some(Action::TogglePlay));
        assert_eq!(player(Key::Space), Some(Action::TogglePlay));
        for key in [Key::Left, Key::Right, Key::Up, Key::Down, Key::Home, Key::End, Key::Enter, Key::Char('3')] {
            assert_eq!(player(key), None, "{key:?} without slider focus");
        }
    }

    #[test]
    fn seek_bar_keys() {
        let at = |secs| Some(Action::SeekTo(Duration::from_secs(secs)));
        assert_eq!(seek_bar(Key::Left), Some(Action::SeekBy(-5.0)));
        assert_eq!(seek_bar(Key::Down), Some(Action::SeekBy(-5.0)));
        assert_eq!(seek_bar(Key::Right), Some(Action::SeekBy(5.0)));
        assert_eq!(seek_bar(Key::Up), Some(Action::SeekBy(5.0)));
        assert_eq!(seek_bar(Key::PageDown), Some(Action::SeekBy(-60.0)));
        assert_eq!(seek_bar(Key::PageUp), Some(Action::SeekBy(60.0)));
        assert_eq!(seek_bar(Key::Home), at(0));
        assert_eq!(seek_bar(Key::End), at(200));
        assert_eq!(seek_bar(Key::Char('0')), at(0));
        assert_eq!(seek_bar(Key::Char('3')), at(60));
        assert_eq!(seek_bar(Key::Char('9')), at(180));
        assert_eq!(seek_bar(Key::Space), Some(Action::TogglePlay));
        assert_eq!(seek_bar(Key::Enter), Some(Action::TogglePlay));
        // Keys the seek bar does not handle are global hotkeys.
        assert_eq!(seek_bar(Key::Char('m')), Some(Action::ToggleMute));
        assert_eq!(seek_bar(Key::Char('x')), None);
    }

    #[test]
    fn seek_bar_keys_with_unknown_duration() {
        let unknown = |key| map(key, Modifiers::NONE, Focus::SeekBar, &Hotkeys::default(), None);
        assert_eq!(unknown(Key::End), None);
        assert_eq!(unknown(Key::Char('5')), None);
        assert_eq!(unknown(Key::Right), Some(Action::SeekBy(5.0)));
        assert_eq!(unknown(Key::Home), Some(Action::SeekTo(Duration::ZERO)));
    }

    #[test]
    fn volume_bar_keys() {
        assert_eq!(volume_bar(Key::Left), Some(Action::StepVolume(-0.1)));
        assert_eq!(volume_bar(Key::Down), Some(Action::StepVolume(-0.1)));
        assert_eq!(volume_bar(Key::Right), Some(Action::StepVolume(0.1)));
        assert_eq!(volume_bar(Key::Up), Some(Action::StepVolume(0.1)));
        assert_eq!(volume_bar(Key::Home), None);
        assert_eq!(volume_bar(Key::Space), Some(Action::TogglePlay));
    }

    #[test]
    fn modifiers() {
        let with = |modifiers, focus| map(Key::Char('f'), modifiers, focus, &Hotkeys::default(), DURATION);
        let ctrl = Modifiers { ctrl: true, ..Modifiers::NONE };
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        let platform = Modifiers { platform: true, ..Modifiers::NONE };
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        for modifiers in [ctrl, alt, platform] {
            assert_eq!(with(modifiers, Focus::Player), None, "{modifiers:?}");
            let arrow = map(Key::Right, modifiers, Focus::SeekBar, &Hotkeys::default(), DURATION);
            assert_eq!(arrow, None, "{modifiers:?} on the seek bar");
        }
        assert_eq!(with(shift, Focus::Player), Some(Action::ToggleFullscreen));
        assert_eq!(
            map(Key::Char('F'), shift, Focus::Player, &Hotkeys::default(), DURATION),
            Some(Action::ToggleFullscreen)
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(player(Key::Char('M')), Some(Action::ToggleMute));
        let upper = Hotkeys::default().mute([Key::Char('X')]);
        assert_eq!(map(Key::Char('x'), Modifiers::NONE, Focus::Player, &upper, DURATION), Some(Action::ToggleMute));
    }

    #[test]
    fn remapped_hotkeys() {
        let hotkeys = Hotkeys::default().mute([Key::Char('x')]).play_pause([Key::Enter]).fullscreen([Key::Char('g')]);
        let map = |key| map(key, Modifiers::NONE, Focus::Player, &hotkeys, DURATION);
        assert_eq!(map(Key::Char('x')), Some(Action::ToggleMute));
        assert_eq!(map(Key::Char('m')), None);
        assert_eq!(map(Key::Enter), Some(Action::TogglePlay));
        assert_eq!(map(Key::Char('k')), None);
        assert_eq!(map(Key::Space), None);
        assert_eq!(map(Key::Char('g')), Some(Action::ToggleFullscreen));
        assert_eq!(map(Key::Char('f')), None);
    }

    #[test]
    fn disabled_hotkeys() {
        let disabled = Hotkeys::disabled();
        for key in [Key::Char('f'), Key::Char('m'), Key::Char('k'), Key::Space] {
            assert_eq!(map(key, Modifiers::NONE, Focus::Player, &disabled, DURATION), None, "{key:?}");
        }
        // Slider keys still work.
        assert_eq!(map(Key::Right, Modifiers::NONE, Focus::SeekBar, &disabled, DURATION), Some(Action::SeekBy(5.0)));
        assert_eq!(map(Key::Space, Modifiers::NONE, Focus::SeekBar, &disabled, DURATION), Some(Action::TogglePlay));
        assert_eq!(
            map(Key::Up, Modifiers::NONE, Focus::VolumeBar, &disabled, DURATION),
            Some(Action::StepVolume(0.1))
        );
        assert_eq!(map(Key::Char('m'), Modifiers::NONE, Focus::VolumeBar, &disabled, DURATION), None);
    }
}
