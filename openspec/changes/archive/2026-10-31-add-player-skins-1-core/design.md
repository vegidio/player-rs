## Context

See proposal.md, "Why". The split this design builds on already exists:

- `crates/player/src/ui`: `Controls` turns input and player events into a `View`. It draws nothing and owns every Video.js rule: activity, scrubbing, hotkeys and time labels.
- `crates/gpui/src/controls`: `ControlsView` (in `mod.rs`) forwards GPUI input to `Controls`, and `render.rs` draws the `View` as a hard-coded Video.js 8 skin. `render.rs` is the only file that knows what anything looks like.

`Options::controls(false)` currently does three things at once: it hides every control, it disables clicks and double-clicks on the video (`Controls::video_clicked`), and it makes `ControlsView::key_down` ignore keys. The player already records the last loaded `Source` (`Shared::loading`, kept for reloading when a surface attaches), which Retry reuses.

## Slices

The `add-player-skins` series:

| # | Change | Delivers | Depends on | Status |
|---|--------|----------|------------|--------|
| 1 | `add-player-skins-1-core` | Core behaviour for skins: part visibility, Settings rule and menu model, default rates, Retry, time-display toggle, bar policy. GPUI skin plumbing: `Skin::Default` (interim Video.js look) and `Skin::None`. | - | **current** |
| 2 | `add-player-skins-2-default` | The designed Default skin (canvas screens 1a-1f) in GPUI: pill bar, volume popover, Settings and Speed menus, spinner, error dialog with Try again, seek hover label and its icons. It replaces the interim look. | 1 | pending |

## Goals / Non-Goals

**Goals:**
- Every behaviour a skin needs is in `crates/player`, tested headless. A skin only draws `View` and reports input.
- The GPUI element can swap skins without changing input handling, focus, fullscreen or the deadline machinery.
- The project stays working: after this slice, `Skin::Default` looks as it does today, plus toggles.

**Non-Goals:**
- No new look in this slice. The Settings menu has a model and tests but no drawn UI until slice 2. The interim Default skin keeps the cycling rate button.
- Applications cannot implement skins themselves. The skin trait stays private.
- No responsive breakpoints. Sizes are fixed per skin.

## Decisions

### 1. Part visibility lives in `ui::Options`, the skin choice on the GPUI element

`ui::Options` gains a `Parts` value (builder: `Options::builder().parts(Parts::default().volume(false).speed(false))`), with one flag per part:

- play
- volume
- seek bar
- current time
- time display (duration/remaining)
- fullscreen
- Speed
- big play button
- spinner
- error display
- seek hover

All flags default to shown. `View` reports each part as shown or not, already combined with availability. For example, Speed is shown only when visible *and* rates are configured. Skins then never re-derive the rules.

Why here: visibility is toolkit-agnostic and must behave the same in a future egui frontend. The Settings rule ("Settings is shown iff at least one item is shown") is logic that belongs where it can be tested headless. The skin itself is drawing, so `Skin` is a `player-gpui` type passed to `VideoPlayer::skin`, and `crates/player` never names a GPUI skin.

Alternatives:
- Put everything in `ui::Options`. The core would then name GPUI skins.
- Give each skin its own settings struct. Toggles would be duplicated per skin, and the Settings rule would move into GPUI code.

A part a skin does not have (for example, the big play button in slice 2's Default) is simply not drawn. Its flag still exists, because `Parts` is shared by all skins.

### 2. Settings menu as state in `ui::Controls`

The menu state is a small enum:

```
Menu::Closed
Menu::Open { page: Page::Root | Page::Speed, focused: usize }
```

`View.settings` reports:
- whether the button is shown
- whether the menu is open
- the page and its items: label, value, checked
- the focused index

New `Action`s:
- `OpenSettings` / `CloseSettings` (toggle via `ToggleSettings`)
- `OpenPage(Page)` and `Back`
- `Select(index)`: on the Speed page, this sets the rate and returns to the root page

The menu keys, with focus inside the menu (`Focus::Menu`), follow Video.js's `Menu`/`MenuButton`:

| Key | Effect |
|-----|--------|
| Down / Up | Move the focus, wrapping |
| Enter / Space | Activate the focused item |
| Escape | Close the menu, and the frontend returns focus to the Settings button |

Left on a sub-page goes back, the one addition, because Video.js has no nested menus.

The menu closes:
- when the control bar hides
- when the media is replaced
- when the Settings button disappears

While the menu is open, the user counts as active, so the bar does not hide under it.

Why in the core: open/closed, the page and the focus are interaction state of the same kind as the seek drag, which already lives here, and each frontend would otherwise reimplement it. Alternative: menu state per skin in GPUI. That is untestable headless and duplicated per toolkit.

`Action::CycleRate` stays for the interim skin and is removed in slice 2 once nothing uses it.

### 3. Default playback rates

`Options::default().playback_rates` becomes `0.25, 0.5, ..., 3.0` (12 values). An explicitly empty list still means no Speed item. Validation is unchanged.

### 4. Retry reloads the stored source and plays

`Player::reload()` loads `recovery.last` again through the same path as `load`: it records the source, then sends `loadfile <url> replace`, asynchronously. It returns `Ok(())` and does nothing when no source was ever loaded. `Action::Retry` calls it, then unpauses (`pause=no`) whatever the `Autoplay` mode, because the user explicitly asked to play. The error clears on `Event::Loaded`, as it does today.

Alternatives:
- Ask the app to reload. Each app would need a callback.
- Resume at the failed position. Deferred, because a failed load has no meaningful position.

### 5. Time display toggle

`Controls` holds `TimeDisplay::Remaining | Duration`, which starts as `Remaining` (canvas 1a shows `-0:35`), and `Action::ToggleTimeDisplay` flips it. `View.time_display` carries the mode and its label. The state is per controller, so it resets with the element's state, not with each loaded file. This deliberately departs from Video.js, whose `remainingTimeDisplay` and `durationDisplay` are separate fixed components.

### 6. Control-bar policy chosen by the frontend

`Controls::new` takes a `BarPolicy { before_start: bool, on_error: bool }` (`Default` = both false = Video.js). Each skin declares its policy:
- The interim Default uses Video.js's.
- None never draws a bar, so its policy is irrelevant.
- Slice 2's Default uses `before_start: true, on_error: true`.

`View` adds `controls_enabled: bool`, which is false on error, so a skin can draw disabled controls. Actions other than fullscreen and Retry are ignored while the error is shown.

The policy is a constructor parameter, not an `Option`, because it is a property of the skin's design that the app does not choose. Alternative: let the core know skin names. That would couple the core to GPUI skins.

### 7. Skins in GPUI behind a private trait

`ControlsView` keeps every input listener, the focus handles, fullscreen, fades and deadlines. Rendering moves behind:

```rust
trait SkinRenderer {
    fn bar_policy(&self) -> BarPolicy;
    fn render(&self, view: &ControlsView, window: &mut Window, cx: &mut Context<ControlsView>) -> AnyElement;
}
```

The public `enum Skin { Default, None }` (`#[non_exhaustive]`, `Default` derived) maps to the implementations:
- `render.rs` becomes `skins/interim.rs` (Default in this slice).
- `skins/none.rs` returns an empty element.

The root (click target, key and pointer listeners, window listeners) stays in `ControlsView::render`, which is why None still takes clicks and hotkeys. Like the options, the skin is read when the element's state is created or gets a new player. Changing it later needs a new element id.

Why private: making it public would freeze `ControlsView`'s internals (bounds cells, focus handles, the drag state) as API before a second real skin has tested the shape. GPUI-Kit covers everything here; no raw GPUI beyond what `render.rs` already uses (`canvas` for measuring).

### 8. Backend/UI separation

Everything new in `crates/player` is plain state and maths, with no toolkit types. An egui frontend would reuse `Options`, `Parts`, the menu model, `BarPolicy`, Retry and the time toggle unchanged, and only write its own skins. No new crates are added, and nothing from `rust-sak` is needed.

### Threading, FFI and lifetimes

`Player::reload` uses the existing asynchronous `command_async` path and the `recovery` mutex, both already safe from the GUI thread. There is no new `unsafe` and no change to the render or event threads. Frames are untouched, so the zero-copy path is unaffected.

## Risks / Trade-offs

- **Two breaking changes in one release** (`controls(false)` is removed, and Speed now appears by default) → document both in the README and crate docs with the one-line migration (`Skin::None`, `playback_rates([])`).
- **The menu model ships before its UI** → the core tests cover it fully, and slice 2 is next in the series.
- **Retry on a source that fails again** shows the error again, with no back-off → acceptable; the user asked again.
- **`Parts` grows with every future part** → `#[non_exhaustive]` and builder methods keep additions non-breaking.

## Migration Plan

- Apps using `Options::builder().controls(false)` switch to `VideoPlayer::skin(Skin::None)`. To also stop clicks and keys, they add `click(false)`, `double_click(false)` and `hotkeys(Hotkeys::disabled())`.
- Apps that want no Speed item pass `playback_rates([])` or `parts(Parts::default().speed(false))`.
- Rollback is reverting the change. No persisted data is involved.
- `openspec/config.yaml`'s "UI look, feel and frontend behaviour: Video.js" bullet is reworded so that behaviour follows Video.js and each skin's look comes from its design (the Claude Design canvas "Video Player Skins").
