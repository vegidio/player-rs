## Why

The player's controls are one hard-coded look (a Video.js 8 placeholder) with a single on/off switch. Applications need to choose how the player looks and which parts it shows, and the project now has a designed Default skin (Claude Design canvas "Video Player Skins", screens 1a-1f) that a fixed renderer cannot host. This first slice puts the toolkit-agnostic behaviour skins need into `crates/player` and makes the GPUI element skinnable, so slice 2 only has to draw the Default skin.

This is slice 1 of 2 of the `add-player-skins` series (see design.md, "Slices").

## What Changes

- **BREAKING** `ui::Options::controls(bool)` is removed. Hiding all controls is now the `None` skin. Click, double-click and hotkeys no longer depend on whether controls are drawn; they follow their own options (`click`, `double_click`, `hotkeys`) under every skin.
- **New: per-part visibility.** `ui::Options` gains visibility toggles for every part a skin may draw, modelled on Video.js's per-component options (`controlBar: { volumePanel: false }`, `bigPlayButton: false`, `loadingSpinner: false`, `errorDisplay: false`): play, volume, seek bar, current time, duration/remaining time, fullscreen, each Settings item (Speed today), big play button, spinner, error display and seek hover time. All are shown by default.
- **New: the Settings button follows its items.** The view reports the Settings button as shown when, and only when, at least one Settings item is visible and available. Today that is Speed alone, so hiding Speed hides Settings.
- **New: Settings menu model.** `ui::Controls` keeps the menu's state (closed, the root page, the Speed page, the focused item), with actions to open, close, enter a page, go back and pick an item, and menu keys modelled on Video.js's menu buttons. An open menu keeps the user active, so the bar does not hide under it.
- **BREAKING** The default `playback_rates` change from none to 0.25x to 3x in steps of 0.25, so Speed is offered out of the box. An empty list still means no Speed item.
- **New: Retry.** `Action::Retry` reloads the last source loaded into the player and clears the error. It needs a new `Player::reload()`, which reissues mpv's `loadfile <url> replace` for the stored source.
- **New: duration/remaining toggle.** Clicking the time label switches it between the duration and the remaining time. This departs from Video.js, which shows the two in separate fixed components.
- **New: control-bar policy per skin.** A frontend can ask for the control bar to be shown before playback first starts (for skins without a big play button) and while an error is shown (for skins that show disabled controls with their error). Without these settings, Video.js's rules apply unchanged.
- **New (GPUI): skins.** `VideoPlayer::skin(Skin)` chooses `Skin::Default` (the default) or `Skin::None`. Skins are built-in only, behind a private trait. `None` draws nothing. Until slice 2 lands, `Default` is today's Video.js-style renderer, moved behind the trait and honouring the visibility toggles.
- The project rule that the UI *look* must match Video.js is reworded: behaviour follows Video.js, and each skin's look comes from its design.

## Capabilities

### New Capabilities

None. Skin selection belongs with the element's other controls in `gpui-controls`.

### Modified Capabilities

- `ui-core`:
  - UI options lose `controls` and gain part visibility and the new default playback rates.
  - New actions: Retry, the time-display toggle and the menu actions.
  - Clicks on the video no longer depend on controls being shown.
  - Control-bar visibility gains the per-skin policy and the open-menu hold.
  - The rate control becomes the Settings menu's Speed item.
  - New requirements: part visibility and the Settings rule, the Settings menu, the time-display toggle, and Retry.
- `gpui-controls`:
  - The element takes a skin instead of the controls switch.
  - The `None` skin draws nothing while input still works.
  - The interim Default skin honours the visibility toggles.

## Impact

- **`crates/player`**:
  - `ui::Options`, `ui::Controls`, `ui::View`, `ui::Action`, `ui::keys` (menu focus), and the `ui::bars` rate helpers.
  - `Player::reload()` in `player.rs`, built on the source the shared state already records when loading.
  - All of this is toolkit-agnostic and so belongs in the core crate. A future egui frontend reuses it unchanged.
- **`crates/gpui`**:
  - `VideoPlayer::skin` and the `Skin` enum in `element.rs`.
  - `controls/` split into the input-driving `ControlsView` and a private skin trait. Today's `render.rs` becomes the interim Default skin; `None` is a second implementation.
  - The tests drop `controls(false)`.
  - Only drawing and GPUI input live here.
- **API**: breaking for apps using `Options::controls(false)` (they switch to `Skin::None`) and for apps relying on there being no rate control by default.
- **Docs**: README ("Customising the controls"), crate docs, the `play` example, and the Video.js rule in `openspec/config.yaml`.
- **rust-sak**: nothing in it applies; this is UI state and options, with no infrastructure.
- **Platforms**: the behaviour is the same on macOS, Windows and Linux. The core is tested headless, and the GPUI tests run on all three in CI. No new platform gaps.
