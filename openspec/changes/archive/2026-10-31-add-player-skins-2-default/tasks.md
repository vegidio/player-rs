## 1. crates/player: remove rate cycling

- [x] 1.1 Remove `Action::CycleRate`, its arm in `Controls::perform` and `ui::bars::next_rate`, keeping `bars::rate_index`. Move the cycling tests in `tests/ui_controls.rs` and `bars.rs` to `SetRate` and `Select`, and cover the "Set the rate", "Cycle the rate", "Cycle from a rate not in the list", "Label and cycle" and "Rate set elsewhere" scenarios. Verify with `cargo test -p player-rs`.

## 2. crates/gpui: assets

- [x] 2.1 Add `assets/fonts/` with Inter Regular, Medium and SemiBold (static TTFs from the official rsms/inter release) and `OFL.txt`. Add a `fonts` module that registers them once per `App` from `ControlsView::new`, logging a warning on failure. Verify with a test that, after a player element is created in an app that registers no fonts, the text system lists `Inter`, and that `cargo package -p player-gpui --list` includes the font files and `OFL.txt`.
- [x] 2.2 Add the design's SVGs under `assets/icons/player/` (design.md, decision 8): play, pause, volume 0-3, fullscreen enter and exit, and the spinner track and arc. Serve them from `Assets` and `Icons` at `icons/player/<name>.svg`. Add `Settings`, `Gauge`, `ChevronLeft`, `ChevronRight`, `Check` and `CircleAlert` to `icon_assets!`, drop the unused volume extras, and point the `assets.rs` icon helpers at the new glyphs. Verify by updating the `assets.rs` tests ("every control icon loads", composed source, listing) and running them.

## 3. crates/gpui: the Default skin

- [x] 3.1 Move the shared helpers (`Activates`, `measure`, the icon builders and a glass-style helper) to `skins/mod.rs`. Add `skins/default/` with the measurement constants (design.md, decision 1), the `{ before_start: true, on_error: true }` policy, the Inter root font, and the fading chrome layer with the scrim. Draw the pill bar: play, mute, current time, seek bar slot, time display, Settings and fullscreen, with hover and open highlights, tooltips and the dimmed disabled state. Map `Skin::Default` to it and delete `skins/interim.rs`. Remove the rate-button and big-play-button tests in `controls/tests.rs`. Verify with tests for "Bar while paused", "Hidden parts", "Rate control", "No Settings without rates", "Fixed size", "Shown before the first play" and "Disabled on error", and confirm the existing fade, time-toggle and fullscreen tests pass.
- [x] 3.2 Draw the seek bar: rounded track, buffered and played parts, white thumb, taller on hover, and the glass hover label. Verify with the existing "Hover label", "Drag while playing", "Drag outside the bar" and "Live stream" tests under the new skin.
- [x] 3.3 Draw the vertical volume popover above the mute button, positioned from its measured bounds, shown on hover of the mute button or popover, during a drag, or while the volume bar is focused. Measure the volume fraction on y from the bottom in `ControlsView` (press, drag and the window-level move). Verify with the "Expand on hover", "Drag sets volume" (40 % from the bottom), "Mute toggle" and "Keyboard" scenario tests.
- [x] 3.4 Draw the Settings menu from `View.settings.menu`:
  - the root page's rows: icon, label, dimmed value, chevron
  - the Speed page: header with Back, divider, checked highlight and check mark
  - the popover anchored at the root's right edge, its height capped above the pill
  - the scrolling list with its thin scrollbar (GPUI-Kit's `Scrollbar` if it can take the design's style, otherwise a hand-drawn thumb)

  Wire the pointer: row clicks perform `Select`, Back performs `Back`, and the Settings button toggles the menu. A press elsewhere performs `CloseSettings`, and the click still reaches the video. Verify with the "Open the menu", "Pick a speed", "Back", "Click outside" and "Long list scrolls" scenario tests.
- [x] 3.5 Add `settings_focus` and `menu_focus` to `ControlsView`:
  - report `Focus::Menu` while the menu holds focus
  - move focus into the menu when it is opened by Enter or Space on the Settings button
  - return focus to the Settings button when the menu closes with focus in it, or to the player when the button disappears
  - scroll the focused entry into view and draw its focus ring

  Verify with the "Menu from the keyboard" and "Escape returns focus" scenario tests, and the existing "Keys in the menu" behaviour through the element.
- [x] 3.6 Draw the overlays:
  - the ring spinner (track plus a turning arc in a glass circle)
  - the error card: alert badge, the "This video can't be played" title, mpv's message, and a focusable white "Try again" button performing `Retry`
  - no big play button

  Verify with the "Big play button", "Spinner", "Spinner hidden", "Error", "Try again" (a WAV made readable after a failed load) and "Error display hidden" scenario tests.

## 4. Docs

- [x] 4.1 Update the README's "Customising the controls" section (the new Default look, no big play button, the backdrop-blur gap, Inter and its OFL licence, the `CycleRate` to `SetRate` migration). Update the `player_gpui` crate docs' Controls section, which still describes the interim look, and the `play` example. Verify with `cargo test --doc` and by running the example.

## 5. Verification

- [x] 5.1 Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` on macOS. Run the `play` example on macOS and check by hand against canvas screens 1a-1f at half scale: pill, volume popover, Settings and Speed menus, spinner, error card with Try again, seek hover label, and the derived glyphs (volume 0-2, exit fullscreen).
- [x] 5.2 Confirm that the CI test workflow passes on Windows and Linux. Run the `play` example on Windows and Linux with the patched GPUI, and check that the text is drawn in Inter and the skin looks as on macOS (without blur on every platform).
