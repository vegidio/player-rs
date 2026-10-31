## 1. crates/player: options and view

- [x] 1.1 Remove `Options::controls` and add `Parts` (one flag per part, all shown, `#[non_exhaustive]`) through `OptionsBuilder::parts`. Change the default `playback_rates` to 0.25..=3.0 in steps of 0.25. Update the `options.rs` tests (defaults, empty rates, parts) and verify with `cargo test -p player-rs ui::options`.
- [x] 1.2 Report each part in `View` as shown only when visible and available, with the Settings button shown iff at least one Settings item is shown. Remove the `controls` gate from `video_clicked`, `video_double_clicked` and `View`. Add tests for the part-visibility and Settings scenarios and for clicks without a controls gate.
- [x] 1.3 Add `BarPolicy { before_start, on_error }` to `Controls::new`, and `View::controls_enabled`, which is false on error. While the error is shown, ignore every action except fullscreen and Retry. Verify with the "Shown before start by policy", "Shown on error by policy" and "Load failure" scenario tests.
- [x] 1.4 Add `TimeDisplay` (starting on remaining) and `Action::ToggleTimeDisplay`, with the mode and label in `View`. Verify with the "Starts with remaining time" and "Toggle" scenario tests, including a reload that keeps the mode.

## 2. crates/player: Settings menu and Retry

- [x] 2.1 Add the Settings menu model to `Controls` (closed, root page, Speed page, focused entry). Add the actions `ToggleSettings`, `OpenPage`, `Back` and `Select`, and expose the menu in `View`: items with label, value, checked and focus. Selecting a speed sets the rate and returns to the root page. Verify with the "Open and navigate", "Select a speed" and Playback rate control scenario tests.
- [x] 2.2 Add `Focus::Menu` to `ui::keys`: Down/Up move with wrapping, Enter/Space activate, Escape closes, Left goes back. Hold the user active while the menu is open, and close the menu when the bar hides, when other media loads and when the Settings button disappears. Verify with the "Keys in the menu", "Escape", "Closes with the bar" and "Open Settings menu" scenario tests.
- [x] 2.3 Add `Player::reload()`, which reloads the recorded last source through the existing `loadfile <url> replace` path and does nothing without one, and `Action::Retry`, which reloads and unpauses. Verify with headless tests in `crates/player/tests`: retry after a failing file becomes readable, retry that fails again, and nothing to retry.

## 3. crates/gpui: skins

- [x] 3.1 Add the private `SkinRenderer` trait and the public `Skin { Default, None }` (`#[non_exhaustive]`, default `Default`), with `VideoPlayer::skin`. Read the skin with the options at state creation and on player change, and pass the skin's `BarPolicy` to `ui::Controls::new`. Verify that the crate builds and that a test shows a later skin change keeps the first skin.
- [x] 3.2 Move `controls/render.rs` into `skins/interim.rs` as the Default skin, keeping the root input listeners in `ControlsView`. Draw each part only while `View` says it is shown, and make the time display clickable to toggle. Verify that the existing `controls/tests.rs` pass, plus new "Hidden parts" and "Time display toggle" tests.
- [x] 3.3 Add `skins/none.rs`, which draws nothing, and drop `controls(false)` handling from `ControlsView` (`key_down`, `action`). Verify with tests for the None skin scenarios: nothing drawn, clicks work, clicks disabled, hotkeys.

## 4. Docs and project rules

- [x] 4.1 Update the README ("Customising the controls", the migration from `controls(false)` to `Skin::None`, and the new default rates), the `player_gpui` and `player::ui` crate docs, and the `play` example. Verify with `cargo test --doc` and by running the example.
- [x] 4.2 Reword the Video.js bullet in `openspec/config.yaml` so that behaviour follows Video.js and each skin's look follows its design (the "Video Player Skins" Claude Design canvas). Verify with `openspec validate add-player-skins-1-core --strict`.

## 5. Verification

- [x] 5.1 Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` on macOS, and confirm the CI test workflow passes on Windows and Linux. Run the `play` example on macOS with `Skin::Default` and `Skin::None`, and check by hand the toggles, the time display toggle and that clicks and hotkeys work under None.
