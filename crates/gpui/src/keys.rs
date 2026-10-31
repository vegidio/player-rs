//! GPUI keystrokes as the UI layer's keys.

use gpui_kit::Keystroke;
use player::ui::{Key, Modifiers};

/// The UI layer's key and modifiers for a GPUI keystroke, or `None` for a key it has no name for (function keys, Tab
/// and so on), which then goes to the application.
pub(crate) fn translate(keystroke: &Keystroke) -> Option<(Key, Modifiers)> {
    let key = match keystroke.key.as_str() {
        "space" => Key::Space,
        "enter" => Key::Enter,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "escape" => Key::Escape,
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(char), None) => Key::Char(char),
                _ => return None,
            }
        }
    };
    let held = &keystroke.modifiers;
    let modifiers = Modifiers { ctrl: held.control, alt: held.alt, shift: held.shift, platform: held.platform };
    Some((key, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &'static str) -> Option<(Key, Modifiers)> {
        translate(&Keystroke::parse(source).unwrap())
    }

    #[test]
    fn named_keys() {
        let named = [
            ("space", Key::Space),
            ("enter", Key::Enter),
            ("left", Key::Left),
            ("right", Key::Right),
            ("up", Key::Up),
            ("down", Key::Down),
            ("home", Key::Home),
            ("end", Key::End),
            ("pageup", Key::PageUp),
            ("pagedown", Key::PageDown),
            ("escape", Key::Escape),
        ];
        for (source, key) in named {
            assert_eq!(parse(source), Some((key, Modifiers::NONE)), "{source}");
        }
    }

    #[test]
    fn characters() {
        assert_eq!(parse("k"), Some((Key::Char('k'), Modifiers::NONE)));
        assert_eq!(parse("3"), Some((Key::Char('3'), Modifiers::NONE)));
        let upper = Keystroke { key: "K".into(), ..Keystroke::parse("k").unwrap() };
        assert_eq!(translate(&upper), Some((Key::Char('K'), Modifiers::NONE)));
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        assert_eq!(parse("shift-k"), Some((Key::Char('k'), shift)));
    }

    #[test]
    fn unknown_keys() {
        for source in ["tab", "f1", "backspace", "delete"] {
            assert_eq!(parse(source), None, "{source}");
        }
    }

    #[test]
    fn modifiers() {
        let with = |source| parse(source).unwrap().1;
        assert_eq!(with("ctrl-f"), Modifiers { ctrl: true, ..Modifiers::NONE });
        assert_eq!(with("alt-f"), Modifiers { alt: true, ..Modifiers::NONE });
        assert_eq!(with("shift-f"), Modifiers { shift: true, ..Modifiers::NONE });
        assert_eq!(with("cmd-f"), Modifiers { platform: true, ..Modifiers::NONE });
        assert_eq!(
            with("ctrl-alt-shift-cmd-right"),
            Modifiers { ctrl: true, alt: true, shift: true, platform: true }
        );
    }
}
