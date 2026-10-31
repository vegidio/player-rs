## Why

Slice 1 made the controls skinnable, but `Skin::Default` still draws the interim Video.js 8 look, and the Settings menu has a model with nothing to show it. This slice draws the designed Default skin from the Claude Design canvas "Video Player Skins" (screens 1a-1f) and replaces the interim look.

This is slice 2 of 2 of the `add-player-skins` series (see design.md, "Slices").

## What Changes

Decisions settled while planning slice 1 that this slice puts into practice:
- **Sizes**: fixed, with the canvas read as a 2x drawing, so every measurement is half the canvas's: a 44 px pill, 12 px text and 24 px buttons. Sizes do not depend on the player's size.
- **Before start**: no big play button. The bar is shown before playback starts and, with disabled controls, during an error (`BarPolicy { before_start: true, on_error: true }`).
- **Non-designed parts**: the spinner and error display come from screens 1e and 1f. The seek hover time has no screen and takes the skin's styling.
- **Error card**: the design's generic title, with mpv's message as the description.

What changes:

- **New look (GPUI): the designed Default skin** replaces the interim one:
  - **Pill bar** (1a): a floating, rounded, translucent black bar inset from the player's edges. From left to right:
    - play/pause/replay
    - mute
    - current time
    - seek bar
    - time display (remaining time or duration)
    - Settings
    - fullscreen
  - **Volume popover** (1b): a vertical volume slider in a rounded popover above the mute control, replacing Video.js's horizontal panel.
  - **Settings menu** (1c): a popover above the Settings button with one row per item: Speed, its value, and a chevron.
  - **Speed page** (1d): a header with Back, then the rates as a scrolling radio list with the current rate checked.
  - **Spinner** (1e): a ring spinner in a translucent circle.
  - **Error card** (1f): an alert icon, the title "This video can't be played", mpv's message, and a "Try again" button that performs Retry. The bar stays visible behind it, its controls dimmed and disabled except fullscreen.
  - **Seek hover time**: a small translucent label above the pointer, in the same style.
- **New (GPUI): the Inter typeface**. The Default skin draws its text in Inter, as the design does. Inter (SIL OFL 1.1) is embedded in `player-gpui` and registered with GPUI's text system the first time a player is created, so apps need no extra setup.
- **New (GPUI): the design's icons**. The design's filled play, pause and volume glyphs, its fullscreen arrows and the spinner ring are bundled as SVGs. The Lucide icons it uses are added to `Icons`: settings, gauge, chevrons, check, circle-alert and rotate-ccw. `player_gpui::Assets` and `Icons` serve them all, as they serve today's icons.
- **BREAKING (core): `Action::CycleRate` is removed**, along with `ui::bars::next_rate`. Only the interim skin used them. Slice 1's design planned their removal here. Rates are chosen on the Settings menu's Speed page.
- **Removed (GPUI)**: the interim skin (`skins/interim.rs`), its rate button and its centred big play button.
- **Known gap**: the design blurs the video behind the pill and popovers (`backdrop-filter: blur(16px)`). GPUI cannot blur behind an element, so they use the design's translucent black without the blur. This is documented.

Rate labels keep the core's `1.5x` form, as the ui-core spec defines them. The design writes `1.5×`.

## Capabilities

### New Capabilities

None. The skin's look belongs in `gpui-controls`.

### Modified Capabilities

- `gpui-controls`:
  - The control bar's contents and look become the designed pill.
  - The volume panel becomes the vertical popover.
  - The overlays become the Default skin's: no big play button, the ring spinner, and the error card with Try again.
  - The seek bar's hover label takes the skin's style.
  - Keyboard focus covers the Settings menu.
  - New requirements: the Settings menu as drawn, and the Default skin's typeface.
  - The documented platform gaps gain the missing backdrop blur.
- `ui-core`:
  - The user actions lose "cycle the rate".
  - The playback rate control loses its cycling scenario.

## Impact

- **`crates/gpui`** (all drawing and GPUI input, so it belongs here):
  - New `controls/skins/default/` (`mod.rs`, `bar.rs`, `popovers.rs`, `overlays.rs`), replacing `skins/interim.rs`.
  - `controls/mod.rs`: a focus handle for the Settings menu and the routing of `Focus::Menu` keys. The volume bar is measured vertically. Focus returns to the Settings button when the menu closes.
  - `assets.rs`: the bundled SVGs and the extra Lucide icons.
  - A new font module that embeds and registers Inter.
  - `controls/tests.rs` is updated.
  - The crate gains about 1.2 MB of embedded font and icon data (`assets/`).
- **`crates/player`**: the removal of `Action::CycleRate` and `bars::next_rate`, with their tests, and `MenuItem::opens`, the page a root item opens, so a skin picks its icon without matching labels. Everything else the skin needs, such as the menu model, Retry, the bar policy, `controls_enabled` and the time toggle, landed in slice 1 and stays toolkit-agnostic.
- **API**: breaking for anyone performing `Action::CycleRate`, who can perform `SetRate` or use the Settings menu's Speed page instead. `Skin::Default` changes look but not API.
- **Docs**: the README ("Customising the controls", the blur gap, Inter's licence), the `player_gpui` crate docs (the Controls section still describes the interim look), and the `play` example.
- **rust-sak**: nothing applies. It has no font or icon support, and embedding a font and SVGs needs no infrastructure beyond `include_bytes!` and GPUI-Kit's `icon_assets!`.
- **Platforms**: the same look and behaviour on macOS, Windows and Linux. The missing backdrop blur applies to all three. GPUI renders the embedded Inter the same on each, which is the point of embedding it.
