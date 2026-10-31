## Context

See proposal.md, "Why". Slice 1 left this in place:

- **Core (`crates/player/src/ui`)**: `ui::Controls` owns every behaviour the skin needs, tested headless:
  - part visibility (`View.shown`, `View.settings.button`)
  - the Settings menu model (`View.settings.menu`, with its pages, items and focus, and the menu actions)
  - `Focus::Menu` keys
  - Retry
  - the time-display toggle
  - `BarPolicy`
  - `View.controls_enabled`
- **GPUI glue (`crates/gpui/src/controls/mod.rs`)**: `ControlsView` keeps all input: the root listeners, focus handles, drags, fullscreen, the fade and deadlines. It hands drawing to a private `SkinRenderer` (`skins/mod.rs`). `Skin::Default` maps to `skins/interim.rs`, a Video.js 8 look with a cycling rate button and a centred big play button.
- **Icons**: `assets.rs` serves Lucide icons: GPUI-Kit's default bundle, plus extras through `icon_assets!`.

The design is screens 1a-1f of the Claude Design canvas "Video Player Skins": artboards `Skin2` to `Skin2f.dc.html`, each 1404×790.

### Decisions carried over from slice 1's planning

They were agreed while slice 1 was planned but not written into its artifacts, so they are recorded here:

| Question | Decision |
|---|---|
| How canvas sizes map to the player | Fixed sizes, with the canvas read as 2x: every measurement is halved. They do not change with the player's size, as in Video.js. |
| Big play button | Not drawn by Default. The bar is shown before start instead: `BarPolicy { before_start: true, on_error: true }`. |
| Parts the design did not draw | Spinner and error display (screens 1e and 1f, added for this), and the seek hover time (in the skin's style). |
| Error text | The design's generic title, with mpv's message as the description. |
| Retry | Reload the same source (slice 1's `Action::Retry`). |
| Rate labels | The core's `1.5x` form (ui-core spec), not the design's `1.5×`. |
| Font (decided for this slice) | Embed Inter. |

## Slices

The `add-player-skins` series:

| # | Change | Delivers | Depends on | Status |
|---|--------|----------|------------|--------|
| 1 | `add-player-skins-1-core` | Core behaviour for skins: part visibility, the Settings rule and menu model, default rates, Retry, the time-display toggle, the bar policy. GPUI skin plumbing: `Skin::Default` (interim Video.js look) and `Skin::None`. | - | done (archived 2026-10-31) |
| 2 | `add-player-skins-2-default` | The designed Default skin (screens 1a-1f) in GPUI: pill bar, volume popover, Settings and Speed menus, spinner, error card with Try again, seek hover label, the design's icons and the Inter typeface. It replaces the interim look and removes `Action::CycleRate`. | 1 | **current** |

## Goals / Non-Goals

**Goals:**
- `Skin::Default` matches screens 1a-1f at half scale, apart from the backdrop blur.
- All the skin's state comes from `ui::View`, and all its input goes through `ControlsView`, as slice 1 set up. The skin owns no behaviour.
- Apps need no new setup: the font registers itself, and the icons come from the `Assets`/`Icons` they already register.

**Non-Goals:**
- No responsive layout or breakpoints. Sizes are fixed, as agreed.
- No public skin API. `SkinRenderer` stays private.
- No new Settings items. Speed is still the only one, so the root page has one row.
- No Video.js big play button, poster drawing or captions in this skin.
- No font subsetting. The Inter faces are embedded whole.

## Decisions

### 1. Measurements: the canvas halved, as named constants

`skins/default/` defines every size and colour as a constant, like `interim.rs` does. Each constant's comment names the artboard and the CSS value it halves. The main values, from the canvas:

| Part | Canvas | Skin |
|---|---|---|
| Pill | 87 tall, inset 25/24 at the sides and 24 at the bottom, radius 44, padding 18/17, gap 24 | 43.5 tall, inset 12, radius 22, padding 9/8.5, gap 12 |
| Buttons | 48, with an open highlight of 72 at `rgba(255,255,255,.18)` and a hover of `.14` | 24, open highlight 36 |
| Icons | play 30, volume 36, settings 34, fullscreen 32 | 15, 18, 17, 16 |
| Time text | Inter 500, 24 px, `rgba(255,255,255,.95)`, tabular figures | 12 px |
| Seek track | 6 tall, thumb 14, track `.2`, fill white | 3, thumb 7, hover: track 5 and thumb 10 |
| Volume popover | 64×192, radius 32, 17 above the pill, track 8, thumb 24, track `.3` | 32×96, radius 16, 8.5 above, track 4, thumb 12 |
| Settings root | 350×78, radius 20, row radius 14 at `.12`, 24 px text, value `.62` | 175×39, radius 10, row radius 7, 12 px |
| Speed page | 350×446, header 79 with a `.14` divider, rows 67 (checked `.18`, hover `.1`), scrollbar 6 at `.35` | 175×223, header 39.5, rows 33.5, scrollbar 3 |
| Spinner | 112 circle, a 60 px ring of stroke 5/48 (track `.22`), 0.9 s per turn | 56 circle, 30 px ring |
| Error card | 520 wide, radius 32, padding 40/40/36, badge 72 at `.14`, title 28/34 600, text 20/28 `.72`, button 56 tall, white, text `#18181b` 20 600 | 260 wide (at most the width minus 24), radius 16, badge 36, title 14/17, text 10/14, button 28 |
| Glass | `rgba(0,0,0,.5)`, a 1 px `rgba(255,255,255,.1)` ring, shadows `0 1 2 .2` and `0 8 24 .2`, `blur(16px)` | the same colours, shadows halved, no blur (see 6) |
| Disabled | opacity `.4`, time labels `.55` | the same |

The seek bar's buffered part is not drawn in the design. It uses `rgba(255,255,255,.35)` between the track and the fill. The hover label has no screen either. It is a glass pill with 12 px Inter 500, 4 px above the track.

Why constants and not a theme: the skin is one fixed design, and `interim.rs` already works this way. Alternative: GPUI-Kit's theme tokens. They would tie the look to the app's theme, which the design does not do.

### 2. File layout

- `skins/default/mod.rs`: the `SkinRenderer` impl, with the policy `{ before_start: true, on_error: true }`. Its root sets `font_family("Inter")` and draws the layers.
- `bar.rs`: the pill, its buttons and the seek bar.
- `popovers.rs`: the volume popover and the Settings menu.
- `overlays.rs`: the spinner, the error card and the scrim.

The helpers `interim.rs` defines are skin-independent, so they move to `skins/mod.rs` for every skin to use: `Activates`, `measure`, the `icon` builders and the glass style. `interim.rs` is deleted, and `skins::renderer` maps `Skin::Default` to the new skin.

Alternative: one `default.rs`. That would be about 900 lines; the split follows the canvas's own grouping.

### 3. Layers and the fade

From the bottom up, the skin's root holds:
1. the spinner and the error card
2. the chrome layer, holding the scrim, the pill and the popovers

The chrome layer is drawn while `bar_drawn()` is true, and the existing fade animates its opacity, so the scrim and the popovers fade with the bar, which they belong to. The core closes the menu when the bar hides. The volume popover follows hover, which the hidden bar no longer has.

The design's scrim has three stops: 0.12 at the top, 0.08 at 55 % and 0.3 at the bottom. GPUI's `linear_gradient` takes two stops, so the scrim is drawn from `rgba(0,0,0,.08)` at 55 % of the height to `.3` at the bottom. The difference above that is invisible over video.

### 4. Volume popover: a vertical bar measured on y

The popover is positioned absolutely in the skin's root, horizontally centred on the mute button, whose bounds are recorded with `measure` at prepaint (the play control may be hidden, so its x is not fixed). It shows when either of these holds, exactly as the collapsed horizontal bar expands today:
- `volume_hovered` (the mute button and the popover form one hover region), or a volume drag
- the volume bar has focus

In `ControlsView`, `volume_fraction` becomes vertical: `1 - ui::bars::fraction(y, top, height)`. `volume_pressed` and `window_mouse_moved` then pass `event.position` rather than `.x`. The core is unchanged: `volume_drag(fraction)` is axis-free. Up and Down already map to volume steps for `Focus::VolumeBar`.

### 5. Settings menu in GPUI

**Drawing.** The menu is drawn from `View.settings.menu` alone. Its page decides between the root and Speed layouts, and its entries come from the core:
- the label
- the value
- whether it is checked
- whether it has focus
- on the root page, the page it opens (`MenuItem::opens`), which picks its icon

It is anchored to the skin root's right edge, 8.5 above the pill, like the canvas, which places it at the bar's right end whichever buttons are hidden. Its height is capped at the space above the pill minus 12, so a short player does not clip it, and the list then scrolls sooner. It shrinks no further than its header and one row (about 77 px); below that it is clipped by the element.

**Scrolling.** The Speed list is a `div` with `overflow_y_scroll()` and a `ScrollHandle` held by `ControlsView`. When the focused entry changes, `scroll_to_item(focused)` keeps it in view. The design's thin white scrollbar is drawn as a thumb from the handle's offset and its content height. GPUI-Kit's `Scrollbar` is used instead if it can take the design's 3 px white thumb with no track. That is checked during implementation; the spec only asks for a visible scrollbar.

**Pointer.**
- Rows perform `Select(i)`, and Back performs `Back`, through `Activates`.
- The menu and the Settings button stop mouse-down propagation.
- The root's existing `capture_any_mouse_down` path gains one step: a press anywhere else while the menu is open performs `CloseSettings`. The click then carries on to the video and toggles play, as Video.js's menu closes on blur while the click still reaches the player.

**Focus.** `ControlsView` gains `settings_focus` (the Settings button) and `menu_focus` (the open menu's container). `key_down` reports `Focus::Menu` while `menu_focus` is focused. Opening the menu by Enter or Space on the Settings button moves focus to `menu_focus`; a click leaves focus where it is, as with any button. Each render checks two things:
- if the menu is closed while `menu_focus` holds focus, focus goes to `settings_focus`, through the same mechanism that moves focus off faded-out controls today
- if the Settings button is gone, focus goes to the player

The focused entry is drawn with the design's focus ring, `0 0 0 3px rgba(161,161,170,.5)`, halved to 1.5 px.

Why the menu is not GPUI-Kit's `Popover` or `DropdownMenu`: they own their open state and focus, while here the core owns them, so the hold-active rule, closing with the bar and the key map stay toolkit-agnostic. Wrapping a self-managed popover would mean syncing two sources of truth. Plain `div`s with absolute positioning cover the design: the popovers sit inside the player's bounds, so no `deferred` or `anchored` is needed.

### 6. No backdrop blur

GPUI has no backdrop-filter. `BoxShadow.blur_radius` blurs the shadow only, and window blur is the whole window's background. The pill and popovers use the design's `rgba(0,0,0,.5)` without blur. Contrast stays acceptable: white text on 50 % black over video is how the canvas reads too, where the blur only softens detail.

Alternative: render the video region behind each glass shape blurred, by sampling the surface. That would need a shader pass over the video texture, against the zero-copy rule's spirit and outside GPUI-Kit. Rejected. The gap is documented under the platform-coverage requirement and in the README.

### 7. Inter, embedded and self-registering

`crates/gpui/assets/fonts/` holds three static faces with `OFL.txt`: `Inter-Regular.ttf` (400), `Inter-Medium.ttf` (500) and `Inter-SemiBold.ttf` (600), the weights the design uses, from the official rsms/inter release. A `fonts` module embeds them with `include_bytes!`. `ControlsView::new` calls `fonts::register(cx)`, which runs once per `App`, guarded by a GPUI global marker. It calls `cx.text_system().add_fonts(...)` and logs a warning on failure, in which case GPUI falls back to its default font.

Choices and why:
- **Static faces over the variable font.** How a variable font's `wght` axis is picked differs across GPUI's text backends (CoreText, DirectWrite, cosmic-text). Static faces resolve by weight on all three. *I am not certain variable Inter would fail; static is the safe choice, at about 1.2 MB against about 0.9 MB.*
- **Self-registering over an app call.** An app call (`player_gpui::init`) adds setup that is easy to miss, and the crate docs already list two setup steps. Registering in `ControlsView::new` is idempotent and needs the `App`, which it has.
- **Not through `AssetSource`.** GPUI loads fonts through `add_fonts`, not through the asset source.
- **Packaging.** `cargo package` includes `assets/` by default, since the crate has no `include`/`exclude`, and task 2.1 checks that.

No new crate is introduced. `rust-sak` has nothing for fonts or icons, and none is needed.

### 8. Icons

The design's glyphs that are not stroke-only Lucide icons are bundled as SVGs in `crates/gpui/assets/icons/player/`:

| Glyph | Source |
|---|---|
| `play` | the design's filled polygon |
| `pause` | the design's two filled rounded bars (1e) |
| `volume-3` | the design's filled speaker with two waves |
| `volume-2`, `volume-1` | the same speaker with one wave, and with none |
| `volume-0` | the speaker with Lucide's `volume-x` cross |
| `fullscreen-enter` | the design's two filled triangles |
| `fullscreen-exit` | the same triangles pointing inwards |
| `spinner-track`, `spinner-arc` | the ring's full circle and its quarter arc |

GPUI tints SVGs with a single colour, so the spinner is two layers: the track at `.22` and the arc turning in white.

`assets.rs` serves them at `icons/player/<name>.svg` from the same `Assets` and `Icons` sources, and the controls draw them with `Icon::new(...).path(...)`. The Lucide icons the design uses are added to `icon_assets!`: `Settings`, `Gauge`, `ChevronLeft`, `ChevronRight`, `Check`, `CircleAlert` and `RotateCcw` (the last already there). Extras the skin no longer draws (`Volume`, `Volume1`, `Volume2`, `VolumeX`) are dropped. The asset tests' "every icon the controls draw" list follows.

The spinner turns its arc with GPUI's `with_animation` and a rotation transformation, as GPUI-Kit's `Spinner` does internally. Kit's `Spinner` draws one icon, not a track under an arc, so the two layers are composed by hand.

### 9. Removing `Action::CycleRate`

`Action::CycleRate`, its arm in `Controls::perform` and `bars::next_rate` are removed. Their tests either move to `SetRate`/`Select` or are deleted. `bars::rate_index` stays, because the Speed page's checked entry uses it. `Action` is `#[non_exhaustive]`, but removing a variant still breaks anyone who names it. That is acceptable in this release, which already breaks `controls(false)`.

### 10. Backend/UI separation

The core changes are the removal and one added field, `MenuItem::opens` (`MenuItem` is `#[non_exhaustive]`, so adding it breaks no one). Everything drawn here reads `ui::View`, and everything clicked goes through `Controls::perform`, `key` or the drag methods. An egui frontend's Default skin would reuse the same view, menu model, policy and Retry unchanged, and would bring its own Inter registration and SVGs.

### Threading, FFI and lifetimes

There is no new `unsafe`, FFI or thread. Fonts are registered on the GUI thread, once per `App`, and live as long as it does. The embedded bytes are `'static`. Retry goes through slice 1's `Player::reload` on its existing asynchronous path. Video frames are untouched, so the zero-copy path is unaffected: the skin only draws translucent shapes over the surface.

### GPUI-Kit vs raw GPUI

These come from GPUI-Kit:
- `Icon` for every glyph
- `Tooltip` for the control tooltips
- `FluentBuilder`
- possibly `Scrollbar`

These are raw GPUI, as `interim.rs` already uses them:
- `div` styling: rounded corners, `box_shadow`, `linear_gradient`, opacity
- `canvas`, for measuring bounds
- `with_animation`, for the fade and the spinner's turn
- `overflow_y_scroll` with `ScrollHandle`

GPUI-Kit has no glass popover or vertical slider matching the design whose state can be driven from outside.

## Risks / Trade-offs

- **No backdrop blur** → documented as a platform-wide gap. The colours are the design's own.
- **About 1.2 MB of fonts in every app using `player-gpui`** → acceptable for a video player. Subsetting is a later option and changes no spec.
- **Registering fonts implicitly could clash with an app that also loads Inter** → GPUI's text system accepts duplicate families, and the app's copy and ours render the same face. Registering once per `App` avoids repeats.
- **Short players clip popovers** → the menu's height is capped to the space above the pill (decision 5). The volume popover needs about 120 px above the pill's top. Below that it is clipped by the element's bounds, which is documented with the fixed-size choice.
- **Pixel-level look is not unit-testable** → tests assert structure and behaviour by element id: order, presence, highlights as state, text content and focus. The look is checked by hand against screens 1a-1f on each platform (tasks 5.1 and 5.2).
- **Derived glyphs** (volume 0-2, exit fullscreen) are not on the canvas → they are drawn in the design's style and checked by hand. Changing them later changes no spec.

## Migration Plan

- Apps performing `Action::CycleRate` switch to `Action::SetRate(rate)` or leave rate choice to the Speed page.
- Apps get the new look automatically under `Skin::Default`. Those that preferred the old big play button have no replacement in this skin. The README says so.
- Rollback is reverting the change. No data or settings persist.

## Open Questions

- Whether GPUI-Kit's `Scrollbar` can be styled to the design's thin white thumb, or a hand-drawn thumb is needed (decision 5). Either way meets the spec and the tasks.
