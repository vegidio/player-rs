## Purpose

The toolkit-agnostic layer that gives every frontend the Video.js player experience: UI options, actions, hotkeys, time labels, control visibility and seek/volume/rate control behaviour. Frontends only draw what it describes and forward user input to it.

## Requirements

### Requirement: UI options with Video.js defaults

The core crate SHALL provide UI options, built and validated before use, for:
- which parts of the controls are visible
- a poster image
- the playback rates offered
- an aspect ratio for the player
- the user-inactivity timeout
- whether a click on the video toggles playback
- whether a double-click toggles fullscreen
- the hotkeys

Unset values SHALL default as in Video.js:
- every part visible
- no poster
- no fixed aspect ratio
- an inactivity timeout of 2 seconds
- click and double-click enabled

There are two deliberate exceptions:
- The hotkeys SHALL be enabled by default.
- The playback rates SHALL default to 0.25 to 3 in steps of 0.25 (`0.25, 0.5, 0.75, ..., 3`), so the Speed setting is offered without configuration.

An explicitly empty list of playback rates SHALL mean no rates are offered. Building SHALL fail with an error naming the option in each of these cases:
- a playback rate is not finite or is outside `0.01..=100`
- an aspect ratio is not two positive numbers in `W:H` form

There SHALL be no option that hides every control; drawing no controls is a frontend's choice of skin.

#### Scenario: Defaults
- **WHEN** UI options are built without setting any value
- **THEN** every part is visible, the playback rates are the twelve values from 0.25 to 3 in steps of 0.25, the inactivity timeout is 2 seconds, click and double-click are enabled and hotkeys are enabled

#### Scenario: No rates
- **WHEN** UI options are built with an empty list of playback rates
- **THEN** no playback rates are offered

#### Scenario: Invalid playback rate
- **WHEN** UI options are built with the playback rates `[0.5, NaN]` or `[0.0, 1.0]`
- **THEN** building fails with an error naming the playback rates option

#### Scenario: Aspect ratio text
- **WHEN** UI options are built with the aspect ratio `"16:9"`, `"4:3"` or `"2.35:1"`
- **THEN** building succeeds

#### Scenario: Malformed aspect ratio
- **WHEN** UI options are built with the aspect ratio `"16x9"`, `"16:0"` or `"wide"`
- **THEN** building fails with an error naming the aspect ratio option

### Requirement: Player aspect ratio

The UI layer SHALL give the height a player of a given width should take, as in Video.js's fluid mode, using:
- the configured aspect ratio when one is set
- otherwise, the video's display size when it is known
- otherwise, 16:9

#### Scenario: Configured ratio
- **WHEN** the aspect ratio is `"4:3"` and the player is 800 wide
- **THEN** its height is 600, whatever the video's size

#### Scenario: Video's own ratio
- **WHEN** no aspect ratio is set, the video's display size is 160x120 and the player is 800 wide
- **THEN** its height is 600

#### Scenario: Nothing known yet
- **WHEN** no aspect ratio is set, no video size is known and the player is 1600 wide
- **THEN** its height is 900

### Requirement: Time labels

Times SHALL be written as in Video.js's `formatTime`:
- `m:ss`, with minutes zero-padded to two digits when the guide (the media's duration) is ten minutes or more
- `h:mm:ss` when the time or the guide reaches one hour
- an unknown time as dashes in the same number of fields: `-:-`, or `-:-:-` with hours

Negative times SHALL be written as zero. The remaining time SHALL be the whole seconds of the duration minus the whole seconds of the position, written with the same rules and a leading `-`.

#### Scenario: Short media
- **WHEN** 65 seconds are written with a guide of 90 seconds
- **THEN** the label is `1:05`

#### Scenario: Guide of ten minutes or more
- **WHEN** 65 seconds are written with a guide of 15 minutes
- **THEN** the label is `01:05`

#### Scenario: Guide of an hour or more
- **WHEN** 65 seconds are written with a guide of 2 hours
- **THEN** the label is `0:01:05`

#### Scenario: Unknown time
- **WHEN** an unknown time is written with a guide of 90 seconds
- **THEN** the label is `-:-`

#### Scenario: Remaining time
- **WHEN** the position is 12.7 seconds and the duration is 90.2 seconds
- **THEN** the remaining-time label is `-1:18`

### Requirement: User actions

The UI layer SHALL perform these actions on the player:
- play, pause, toggle play
- seek to a position, seek by an offset
- toggle mute
- set the volume, step the volume by an amount
- set the rate, cycle the rate
- toggle fullscreen
- retry after an error
- toggle the time display
- open, close and navigate the Settings menu

Toggle mute SHALL follow Video.js: at volume 0, it SHALL set the volume to the last non-zero volume (at least 0.1) and unmute. Otherwise it SHALL flip the mute state. Stepping or setting the volume while muted SHALL unmute. Cycling the rate SHALL select the playback rate after the current one in the configured list, wrapping around, or the first one if the current rate is not in the list. With no rates configured, it SHALL do nothing. Toggling fullscreen SHALL not touch the player; it SHALL be returned to the frontend as a request, because only the frontend controls its window.

#### Scenario: Toggle mute at volume zero
- **WHEN** the volume was 0.6, was then set to 0, and mute is toggled
- **THEN** the volume becomes 0.6 and the player is unmuted

#### Scenario: Toggle mute from a tiny volume
- **WHEN** the last non-zero volume was 0.05, the volume is 0, and mute is toggled
- **THEN** the volume becomes 0.1

#### Scenario: Cycle the rate
- **WHEN** the playback rates are `[0.5, 1, 1.5, 2]`, the rate is 2, and the rate is cycled
- **THEN** the rate becomes 0.5

#### Scenario: Cycle from a rate not in the list
- **WHEN** the playback rates are `[0.5, 1, 2]`, the rate is 1.25, and the rate is cycled
- **THEN** the rate becomes 0.5

#### Scenario: Fullscreen is the frontend's
- **WHEN** fullscreen is toggled
- **THEN** a fullscreen request is returned to the frontend and the player's state does not change

### Requirement: Global hotkeys

When hotkeys are enabled, these keys SHALL act wherever focus is inside the player, as in Video.js:
- `f` toggles fullscreen
- `m` toggles mute
- `k` or Space toggles play

Hotkeys SHALL be replaceable by other keys for each of the three actions, and SHALL be possible to disable entirely. A key pressed together with Ctrl, Alt or the platform command key SHALL be left unhandled, so application shortcuts keep working. Every key SHALL report whether it was handled, so the frontend can let unhandled keys through.

#### Scenario: Play-pause key
- **WHEN** media is playing, focus is in the player and `k` is pressed
- **THEN** playback pauses and the key is reported handled

#### Scenario: Modified key
- **WHEN** Ctrl+`f` or Cmd+`f` is pressed
- **THEN** nothing happens and the key is reported unhandled

#### Scenario: Hotkeys disabled
- **WHEN** hotkeys are disabled and `m` is pressed with focus in the player
- **THEN** the mute state does not change and the key is reported unhandled

#### Scenario: Remapped key
- **WHEN** the mute hotkey is remapped to `x`
- **THEN** `x` toggles mute and `m` no longer does

### Requirement: Slider keys

When the seek bar has focus, keys SHALL act on it as in Video.js, whether or not hotkeys are enabled:
- Left or Down: seek back 5 seconds
- Right or Up: seek forward 5 seconds
- PageDown / PageUp: seek back / forward 60 seconds
- Home: go to the start
- End: go to the end
- digits 0 to 9: go to 0 % to 90 % of the duration
- Space or Enter: toggle play

When the volume bar has focus, Left or Down SHALL lower and Right or Up SHALL raise the volume by 0.1, clamped to `0.0..=1.0`. A key with a slider focus that the slider does not handle SHALL be treated as a global hotkey.

#### Scenario: Arrow on the seek bar
- **WHEN** the seek bar has focus, the position is 30 seconds and Right is pressed
- **THEN** the player seeks to 35 seconds

#### Scenario: Digit on the seek bar
- **WHEN** the seek bar has focus, the duration is 200 seconds and `3` is pressed
- **THEN** the player seeks to 60 seconds

#### Scenario: Arrow on the volume bar
- **WHEN** the volume bar has focus, the volume is 0.5 and Down is pressed
- **THEN** the volume becomes 0.4

#### Scenario: Arrow without slider focus
- **WHEN** focus is in the player but on no slider, and Right is pressed
- **THEN** the position does not change and the key is reported unhandled

### Requirement: Clicks on the video

A click on the video SHALL toggle play. A double-click SHALL also request fullscreen, as in Video.js, where its two clicks still toggle play twice. When the click or double-click option is disabled, the corresponding action SHALL not happen. Both SHALL work the same whatever the frontend draws, including when it draws no controls.

#### Scenario: Click toggles play
- **WHEN** media is paused and the video is clicked
- **THEN** playback starts

#### Scenario: Double-click requests fullscreen
- **WHEN** the video is double-clicked
- **THEN** a fullscreen request is returned to the frontend

#### Scenario: Click disabled
- **WHEN** the click option is disabled and the video is clicked
- **THEN** the playback state does not change

#### Scenario: Controls hidden
- **WHEN** the frontend draws no controls, media is paused and the video is clicked
- **THEN** playback starts

### Requirement: User activity and control-bar visibility

The UI layer SHALL track user activity as Video.js does:
- **Becoming active**: the user SHALL be active on any pointer movement, pointer press, key press or other input.
- **Becoming inactive**: the user SHALL become inactive once the inactivity timeout has passed with no input.
- **Holding active**: the user SHALL stay active while a pointer button is held, while the pointer is over the control bar, or while the Settings menu is open.
- **Timeout of zero**: an inactivity timeout of zero SHALL mean the user never becomes inactive.

The frontend SHALL be able to choose a control-bar policy with two independent settings, both off by default:
- show the control bar before playback first starts
- show the control bar while the media's error is shown

The control bar SHALL be hidden in each of these cases:
- before playback first starts for the current media, unless the policy shows it then
- when the media failed to load, unless the policy shows it then
- once playback has started, the user is inactive and the media is playing

The control bar SHALL be visible in every other case, including whenever the media is paused. While the media's error is shown, the UI layer SHALL report the controls as disabled. Every action except toggling fullscreen and retrying SHALL then do nothing.

The pointer cursor SHALL be hidden over the player when, and only when, the user is inactive and the media is playing. When it changes visibility, the control bar SHALL fade in over 0.1 seconds and fade out over 1 second. The UI layer SHALL tell the frontend when it next needs to be updated for activity to time out.

#### Scenario: Controls hide while playing
- **WHEN** media is playing and there has been no input for 2 seconds
- **THEN** the control bar and the cursor are hidden

#### Scenario: Input shows them again
- **WHEN** the control bar is hidden while playing and the pointer moves
- **THEN** the control bar and the cursor are visible again, and hide again after another 2 seconds without input

#### Scenario: Paused media keeps the controls
- **WHEN** media is paused and there has been no input for 10 seconds
- **THEN** the control bar stays visible

#### Scenario: Hovering the control bar
- **WHEN** media is playing and the pointer rests over the control bar for 10 seconds
- **THEN** the control bar stays visible, and hides 2 seconds after the pointer leaves it

#### Scenario: Holding a pointer button
- **WHEN** media is playing and a pointer button is held without moving for 10 seconds
- **THEN** the control bar stays visible

#### Scenario: Open Settings menu
- **WHEN** media is playing and the Settings menu stays open for 10 seconds without input
- **THEN** the control bar stays visible

#### Scenario: Zero timeout
- **WHEN** the inactivity timeout is zero and media plays for 10 seconds without input
- **THEN** the control bar stays visible

#### Scenario: Before playback starts
- **WHEN** media is loaded with autoplay off and has not been played, under the default policy
- **THEN** the control bar is hidden

#### Scenario: Shown before start by policy
- **WHEN** the policy shows the bar before playback starts, and media is loaded with autoplay off and has not been played
- **THEN** the control bar is visible and does not hide while the media stays paused

#### Scenario: Shown on error by policy
- **WHEN** the policy shows the bar on error and a source that cannot be opened is loaded
- **THEN** the control bar is visible, the controls are reported disabled, and play does nothing while fullscreen is still requested

### Requirement: Start state, loading and errors

The UI layer SHALL show the big play button and, when a poster is set, the poster while the current media has not started playing, as Video.js does before `has-started`. The big play button SHALL not be shown when its part is hidden. Both SHALL be hidden once playback starts, and shown again when new media is loaded. Activating the big play button SHALL start playback.

A loading spinner SHALL be shown once media is loaded and playback has been waiting, for the network or for a seek, for 0.3 seconds, as Video.js delays it. It SHALL be hidden as soon as the wait ends, and never shown while its part is hidden.

When media fails to load, the UI layer SHALL report its error message, whether or not the error display part is visible. The error display SHALL be reported as shown only when its part is visible. The error SHALL be cleared when other media, or the same media again, is loaded.

The play control SHALL show:
- a play icon while paused
- a pause icon while playing
- a replay icon once the media has played to its end

#### Scenario: Before the first play
- **WHEN** media with a poster set is loaded with autoplay off
- **THEN** the big play button and the poster are shown and the play control shows the play icon

#### Scenario: Big play button hidden
- **WHEN** the big play button part is hidden and media is loaded with autoplay off
- **THEN** the big play button is not shown

#### Scenario: Started
- **WHEN** the big play button is activated
- **THEN** playback starts, the big play button and poster are hidden and the play control shows the pause icon

#### Scenario: Ended
- **WHEN** media plays to its end
- **THEN** the play control shows the replay icon, and activating it restarts playback from the beginning

#### Scenario: Spinner delay
- **WHEN** a seek starts and completes within 0.3 seconds
- **THEN** the spinner is never shown; when a wait lasts longer, the spinner is shown from 0.3 seconds after it began until it ends

#### Scenario: Load failure
- **WHEN** a source that cannot be opened is loaded under the default control-bar policy
- **THEN** the error message is reported and the control bar is hidden

### Requirement: Seek bar

The seek bar SHALL report:
- the played fraction (position over duration)
- the buffered fraction (buffered position over duration)
- while the pointer hovers it, the time label at the hovered point

A pointer position SHALL map to a fraction clamped to `0..=1`, and a fraction to a time that is that share of the duration. A time equal to the duration SHALL be moved back by 0.1 seconds, so scrubbing never ends the media.

Dragging SHALL follow Video.js:
- **Pressing**: pause the media if it was playing, then seek to the pressed point.
- **Moving**: seek quickly to each new point; the played fraction SHALL follow the pointer, not the decoded position.
- **Releasing**: seek exactly to the release point, then resume playback if it was playing when the drag began.

While the duration is unknown, such as for live streams, the seek bar SHALL ignore pointer input and report a played fraction of zero.

#### Scenario: Played and buffered fractions
- **WHEN** the position is 30 seconds, 60 seconds are buffered and the duration is 120 seconds
- **THEN** the played fraction is 0.25 and the buffered fraction is 0.5

#### Scenario: Hover label
- **WHEN** the pointer hovers at 50 % of a seek bar for media of 90 seconds
- **THEN** the hover label is `0:45`

#### Scenario: Drag while playing
- **WHEN** media is playing and the seek bar is pressed at 10 %, dragged to 50 % and released
- **THEN** playback is paused during the drag, the played fraction follows the pointer, and after release the position is within 0.1 seconds of 50 % of the duration and playback resumes

#### Scenario: Drag while paused
- **WHEN** media is paused and the seek bar is dragged and released
- **THEN** the position moves to the release point and the media stays paused

#### Scenario: Drag to the end
- **WHEN** the seek bar is dragged to 100 % of 2-second media and released
- **THEN** the media is sought to 1.9 seconds and does not end

#### Scenario: Unknown duration
- **WHEN** the duration is unknown and the seek bar is pressed
- **THEN** the position does not change

### Requirement: Volume control

The volume bar SHALL report the volume as its fraction, or zero while muted, as Video.js does. Pressing or dragging it SHALL set the volume to the pointer's fraction and unmute the player. The mute control SHALL report an icon level, as Video.js does:
- 0 when muted or at volume 0
- 1 below 0.33
- 2 below 0.67
- 3 otherwise

#### Scenario: Muted bar
- **WHEN** the volume is 0.6 and the player is muted
- **THEN** the volume bar fraction is 0 and the icon level is 0

#### Scenario: Drag unmutes
- **WHEN** the player is muted and the volume bar is dragged to 40 %
- **THEN** the volume is 0.4 and the player is unmuted

#### Scenario: Icon levels
- **WHEN** the volume is 0.2, 0.5 and 0.9 in turn, unmuted
- **THEN** the icon level is 1, 2 and 3 respectively

### Requirement: Playback rate control

The playback rate SHALL be offered as the Speed item of the Settings menu, only when playback rates are configured and the Speed part is visible. Its value label SHALL be the current rate followed by `x`, with no trailing zeros (`1x`, `1.5x`). Its page SHALL list every configured rate in order, with the current rate checked. Selecting one SHALL set that rate.

#### Scenario: No rates
- **WHEN** no playback rates are configured
- **THEN** the Speed item is not offered

#### Scenario: Speed hidden
- **WHEN** playback rates are configured but the Speed part is hidden
- **THEN** the Speed item is not offered

#### Scenario: Label and cycle
- **WHEN** the playback rates are `[1, 1.5, 2]`, the rate is 1 and the rate is cycled
- **THEN** the rate becomes 1.5 and the Speed item's value reads `1.5x`

#### Scenario: Label and selection
- **WHEN** the playback rates are `[1, 1.5, 2]`, the rate is 1, and `1.5x` is selected on the Speed page
- **THEN** the rate becomes 1.5, the Speed item's value reads `1.5x` and `1.5x` is checked

### Requirement: Part visibility and the Settings button

The UI options SHALL let each of these parts be shown or hidden independently, as Video.js's per-component options do:
- play control
- volume (mute control and volume bar)
- seek bar
- current time
- time display (duration or remaining time)
- fullscreen control
- each Settings item, which today is Speed
- big play button
- loading spinner
- error display
- seek-bar hover time

The UI layer SHALL report each part as shown only when it is both visible and available. The Settings button SHALL not be an option of its own: it SHALL be reported as shown when, and only when, at least one Settings item is shown.

#### Scenario: Hidden part
- **WHEN** the volume part is hidden
- **THEN** the UI layer reports the volume as not shown, while the mute hotkey still toggles mute

#### Scenario: Settings follows its only item
- **WHEN** the Speed part is hidden
- **THEN** the Settings button is reported as not shown

#### Scenario: Settings needs an available item
- **WHEN** the Speed part is visible but no playback rates are configured
- **THEN** the Settings button is reported as not shown

#### Scenario: Settings shown
- **WHEN** the Speed part is visible and playback rates are configured
- **THEN** the Settings button is reported as shown

### Requirement: Settings menu

The UI layer SHALL keep the Settings menu's state:
- whether it is open
- which page it shows: the root page listing the shown Settings items with their current values, or an item's own page
- which entry has keyboard focus

Activating the Settings button SHALL open the menu on its root page, or close it if it is open. Activating an item on the root page SHALL open that item's page. Going back SHALL return to the root page. Selecting an entry on an item's page SHALL apply it and return to the root page.

With focus in the menu, keys SHALL act as on Video.js menus:
- Down moves focus to the next entry, and Up to the previous one, wrapping
- Enter or Space activates the focused entry
- Escape closes the menu
- Left on an item's page goes back to the root page

The menu SHALL close when the control bar hides, when other media is loaded, and when the Settings button stops being shown.

#### Scenario: Open and navigate
- **WHEN** the Settings button is activated and then the Speed item is activated
- **THEN** the menu is open on the Speed page with the current rate checked

#### Scenario: Select a speed
- **WHEN** the Speed page is open, the rate is 1 and `1.5x` is selected
- **THEN** the rate becomes 1.5 and the menu shows the root page, where Speed reads `1.5x`

#### Scenario: Keys in the menu
- **WHEN** the menu is open on the root page with Speed focused, and Enter, Down and Enter are pressed
- **THEN** the Speed page opens and the rate after the current one in the list is selected

#### Scenario: Escape
- **WHEN** the menu is open and Escape is pressed with focus in it
- **THEN** the menu closes and the key is reported handled

#### Scenario: Closes with the bar
- **WHEN** the menu is open and the control bar hides
- **THEN** the menu is closed

### Requirement: Time display toggle

The time display SHALL show either the remaining time, as the remaining-time label is written, or the duration, as a time label is written. It SHALL start with the remaining time. Activating it SHALL switch between the two, and the choice SHALL be kept for the life of the player view, across loaded media. This deliberately departs from Video.js, which shows the duration and the remaining time as separate fixed components.

#### Scenario: Starts with remaining time
- **WHEN** 2-minute media is at 0:42
- **THEN** the time display reads `-1:18`

#### Scenario: Toggle
- **WHEN** the time display is activated while 2-minute media is at 0:42
- **THEN** it reads `2:00`, and activating it again makes it read `-1:18`

### Requirement: Retry after an error

Retrying SHALL load the last source given to the player again and SHALL start playback whatever the autoplay mode, because the user asked to play. Retrying with no source ever loaded SHALL do nothing and succeed. A successful load SHALL clear the error.

#### Scenario: Retry succeeds
- **WHEN** a source failed to load because it was briefly unavailable, it becomes available, and retry is performed
- **THEN** the media loads, the error is cleared and playback starts

#### Scenario: Retry fails again
- **WHEN** retry is performed for a source that still cannot be opened
- **THEN** the error is reported again

#### Scenario: Nothing to retry
- **WHEN** retry is performed on a player that never loaded a source
- **THEN** nothing happens and no error is returned

### Requirement: Consistent UI behaviour on all platforms

The UI layer SHALL behave identically on macOS, Windows and Linux, and SHALL work without a display, a GUI toolkit or a video output, so that every requirement in this capability can be checked headless.

#### Scenario: Headless on CI
- **WHEN** the UI layer drives a player with no video output and null audio on any supported platform
- **THEN** every behaviour in this capability can be observed through the player's state and the UI layer's view
