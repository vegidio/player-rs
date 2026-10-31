//! The icons the controls draw, as GPUI asset sources, and which icon each control shows: the Default skin's own
//! glyphs (`assets/icons/player/`, from its design) and the Lucide icons it uses.

use gpui_kit::assets::IconName;
use gpui_kit::{AssetSource, Result, SharedString};
use player::ui::PlayIcon;
use std::borrow::Cow;

mod lucide {
    // The Lucide icons the Default skin draws, embedded one by one from GPUI-Kit's full catalogue: Gauge and RotateCcw
    // are missing from its default bundle, and the rest are listed so `Icons` serves every icon the skin draws.
    gpui_kit::assets::icon_assets!(pub(super) Lucide, [Settings, Gauge, ChevronLeft, ChevronRight, Check, CircleAlert, RotateCcw]);
}

/// The Default skin's own glyphs, drawn from its design.
const PLAYER: [(&str, &[u8]); 10] = [
    ("icons/player/play.svg", include_bytes!("../assets/icons/player/play.svg")),
    ("icons/player/pause.svg", include_bytes!("../assets/icons/player/pause.svg")),
    ("icons/player/volume-0.svg", include_bytes!("../assets/icons/player/volume-0.svg")),
    ("icons/player/volume-1.svg", include_bytes!("../assets/icons/player/volume-1.svg")),
    ("icons/player/volume-2.svg", include_bytes!("../assets/icons/player/volume-2.svg")),
    ("icons/player/volume-3.svg", include_bytes!("../assets/icons/player/volume-3.svg")),
    (
        "icons/player/fullscreen-enter.svg",
        include_bytes!("../assets/icons/player/fullscreen-enter.svg"),
    ),
    ("icons/player/fullscreen-exit.svg", include_bytes!("../assets/icons/player/fullscreen-exit.svg")),
    ("icons/player/spinner-track.svg", include_bytes!("../assets/icons/player/spinner-track.svg")),
    ("icons/player/spinner-arc.svg", include_bytes!("../assets/icons/player/spinner-arc.svg")),
];

/// Every icon the player's controls draw, alone, for apps that compose their own [`AssetSource`]: the skin's glyphs
/// and the Lucide icons it uses. Unlisted paths load as `None`. Most apps register [`Assets`] instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = PLAYER.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        lucide::Lucide.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths: Vec<SharedString> = PLAYER
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| name.starts_with(path))
            .map(Into::into)
            .collect();
        paths.extend(lucide::Lucide.list(path)?);
        Ok(paths)
    }
}

/// Every icon the player's controls draw, falling back to GPUI-Kit's default bundle for every other path. Register it
/// with `gpui_kit::application().with_assets(player_gpui::Assets)`, in place of `gpui_kit::assets::Assets`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = Icons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(Icons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

/// The path of a Lucide icon the controls draw.
pub(crate) fn lucide(icon: IconName) -> SharedString {
    icon.path()
}

/// The play control's icon. The design has no replay glyph; Lucide's `rotate-ccw` stands in, as on Try again.
pub(crate) fn play(icon: PlayIcon) -> SharedString {
    match icon {
        PlayIcon::Play => "icons/player/play.svg".into(),
        PlayIcon::Pause => "icons/player/pause.svg".into(),
        PlayIcon::Replay => lucide(IconName::RotateCcw),
    }
}

/// The mute control's icon for a volume level from 0 (muted) to 3, as Video.js's `vjs-vol-0` to `vjs-vol-3`.
pub(crate) fn volume(level: u8) -> SharedString {
    match level {
        0 => "icons/player/volume-0.svg".into(),
        1 => "icons/player/volume-1.svg".into(),
        2 => "icons/player/volume-2.svg".into(),
        _ => "icons/player/volume-3.svg".into(),
    }
}

/// The fullscreen control's icon: enter, or exit while the player is fullscreen.
pub(crate) fn fullscreen(on: bool) -> SharedString {
    if on {
        "icons/player/fullscreen-exit.svg".into()
    } else {
        "icons/player/fullscreen-enter.svg".into()
    }
}

/// The loading spinner's faint full ring and its turning arc.
pub(crate) const SPINNER_TRACK: &str = "icons/player/spinner-track.svg";
pub(crate) const SPINNER_ARC: &str = "icons/player/spinner-arc.svg";

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon the controls can draw.
    fn used() -> Vec<SharedString> {
        let mut icons: Vec<_> = [PlayIcon::Play, PlayIcon::Pause, PlayIcon::Replay].into_iter().map(play).collect();
        icons.extend((0..=3).map(volume));
        icons.extend([fullscreen(false), fullscreen(true), SPINNER_TRACK.into(), SPINNER_ARC.into()]);
        icons.extend(
            [
                IconName::Settings,
                IconName::Gauge,
                IconName::ChevronLeft,
                IconName::ChevronRight,
                IconName::Check,
                IconName::CircleAlert,
            ]
            .map(lucide),
        );
        icons
    }

    #[test]
    fn every_control_icon_loads() {
        for path in used() {
            let bytes = Assets.load(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(bytes.is_some_and(|bytes| bytes.starts_with(b"<svg")), "{path} loads as an SVG");
        }
    }

    /// An app's own composition: its sources, then the player's icons, then GPUI-Kit's default bundle. `Icons` alone
    /// serves every icon the controls draw.
    #[test]
    fn composed_icons_serve_every_control_icon() {
        for path in used() {
            assert!(Icons.load(&path).unwrap().is_some(), "{path} loads");
        }
    }

    #[test]
    fn falls_back_to_the_default_bundle() {
        // A GPUI-Kit component icon the player does not use.
        assert!(Assets.load(&IconName::ChevronDown.path()).unwrap().is_some());
        assert!(Icons.load(&IconName::ChevronDown.path()).unwrap().is_none());
        // The volume icons the interim look drew are no longer the player's.
        assert!(Icons.load(&IconName::VolumeX.path()).unwrap().is_none());
    }

    #[test]
    fn lists_both_sources_once() {
        let listed = Assets.list("icons/").unwrap();
        let mut sorted = listed.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(listed, sorted, "sorted and deduplicated");
        for path in [
            play(PlayIcon::Play),
            lucide(IconName::Gauge),
            lucide(IconName::Play),
            lucide(IconName::ChevronDown),
        ] {
            assert!(listed.contains(&path), "{path} listed");
        }
        let player = Icons.list("icons/player/").unwrap();
        assert_eq!(player.len(), PLAYER.len(), "a prefix lists the skin's glyphs alone");
    }
}
