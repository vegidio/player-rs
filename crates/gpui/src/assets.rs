//! The Lucide icons the controls draw, as GPUI asset sources, and which icon each control shows.

use gpui_kit::assets::IconName;
use gpui_kit::{AssetSource, Result, SharedString};
use player::ui::PlayIcon;
use std::borrow::Cow;

mod extra {
    // GPUI-Kit's default bundle lacks these; `icon_assets!` embeds them one by one from its full catalogue.
    gpui_kit::assets::icon_assets!(pub Icons, [RotateCcw, Volume, Volume1, Volume2, VolumeX]);
}

/// The player's icons that GPUI-Kit's default bundle (`gpui_kit::assets::Assets`) lacks, alone, for apps that compose
/// their own [`AssetSource`]. Unlisted paths load as `None`. Most apps register [`Assets`] instead.
pub use extra::Icons;

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

/// The play control's icon.
pub(crate) fn play(icon: PlayIcon) -> IconName {
    match icon {
        PlayIcon::Play => IconName::Play,
        PlayIcon::Pause => IconName::Pause,
        PlayIcon::Replay => IconName::RotateCcw,
    }
}

/// The mute control's icon for a volume level from 0 (muted) to 3, as Video.js's `vjs-vol-0` to `vjs-vol-3`.
pub(crate) fn volume(level: u8) -> IconName {
    match level {
        0 => IconName::VolumeX,
        1 => IconName::Volume,
        2 => IconName::Volume1,
        _ => IconName::Volume2,
    }
}

/// The fullscreen control's icon: enter, or exit while the player is fullscreen.
pub(crate) fn fullscreen(on: bool) -> IconName {
    if on { IconName::Minimize } else { IconName::Maximize }
}

/// The loading spinner's icon.
pub(crate) const SPINNER: IconName = IconName::LoaderCircle;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon the controls can draw.
    fn used() -> Vec<IconName> {
        let mut icons: Vec<_> = [PlayIcon::Play, PlayIcon::Pause, PlayIcon::Replay].into_iter().map(play).collect();
        icons.extend((0..=3).map(volume));
        icons.extend([fullscreen(false), fullscreen(true), SPINNER]);
        icons
    }

    #[test]
    fn every_control_icon_loads() {
        for icon in used() {
            let path = icon.path();
            let bytes = Assets.load(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(bytes.is_some_and(|bytes| bytes.starts_with(b"<svg")), "{path} loads as an SVG");
        }
    }

    /// An app's own composition: its sources, then the player's extra icons, then GPUI-Kit's default bundle.
    #[test]
    fn composed_icons_serve_every_control_icon() {
        for icon in used() {
            let path = icon.path();
            let loaded = match Icons.load(&path).unwrap() {
                Some(bytes) => Some(bytes),
                None => gpui_kit::assets::Assets.load(&path).unwrap(),
            };
            assert!(loaded.is_some(), "{path} loads");
        }
    }

    #[test]
    fn falls_back_to_the_default_bundle() {
        // A GPUI-Kit component icon the player does not use.
        assert!(Assets.load(&IconName::ChevronDown.path()).unwrap().is_some());
        assert!(Icons.load(&IconName::ChevronDown.path()).unwrap().is_none());
    }

    #[test]
    fn lists_both_sources_once() {
        let listed = Assets.list("icons/").unwrap();
        let mut sorted = listed.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(listed, sorted, "sorted and deduplicated");
        for icon in [IconName::RotateCcw, IconName::VolumeX, IconName::Play, IconName::ChevronDown] {
            assert!(listed.contains(&icon.path()), "{} listed", icon.path());
        }
    }
}
