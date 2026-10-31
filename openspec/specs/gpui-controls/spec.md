## Purpose

Gives the GPUI video element the controls and interaction of a Video.js player: the control bar, the big play button, the spinner and error display, pointer and keyboard input, and fullscreen. All of it follows the toolkit-agnostic UI layer's rules.

## Requirements

### Requirement: Player options on the element

The video element SHALL accept the player UI options and SHALL default to the default options, under which every part is visible. The options SHALL take effect when the element's state is created and whenever the element is given a different player; changing them on a later render with the same id and player SHALL have no effect, as Video.js reads its options once at construction. Clicks, double-clicks and keys meant for the player SHALL follow the click, double-click and hotkey options under every skin, including a skin that draws no controls.

#### Scenario: Default options
- **WHEN** an application shows a video element without passing options or a skin
- **THEN** the element shows the Default skin's controls with every part visible

#### Scenario: Controls disabled
- **WHEN** an application wants bare video, chooses the None skin with the click option disabled, and the video is clicked
- **THEN** no control is drawn and playback does not change

#### Scenario: Options changed later
- **WHEN** the element is rendered again with the same id and player but different options
- **THEN** the element keeps behaving as the first options said

### Requirement: Fluid sizing from the aspect ratio

When the options set an aspect ratio, the element's height SHALL follow its width at that ratio, as Video.js's fluid mode does. Without one, the element SHALL take the size the application gives it.

#### Scenario: Aspect ratio set
- **WHEN** the options set the aspect ratio `16:9` and the element is laid out 800 points wide with no height given
- **THEN** the element is 450 points tall

#### Scenario: No aspect ratio
- **WHEN** no aspect ratio is set and the application sizes the element 640x480
- **THEN** the element is 640x480 whatever the video's size

### Requirement: Control bar contents and look

The Default skin's control bar SHALL be the designed floating pill (canvas "Video Player Skins", screen 1a):
- a fully rounded bar, inset from the element's left, right and bottom edges, over the video
- a translucent black background, a faint light outline and a soft shadow
- white icons and text in the skin's typeface
- fixed sizes, half the design canvas's measurements (a pill about 44 pixels tall, 12-pixel text, 24-pixel buttons), independent of the element's size

It SHALL contain, from left to right, each only while the UI layer reports its part as shown:
1. the play control, whose icon is play, pause or replay as the UI layer says
2. the mute control, whose icon shows the volume level (0 muted, 1, 2, 3 loud)
3. the current time, such as `0:42`
4. the seek bar, taking the remaining width
5. the time display, such as `-1:18`, which switches between the remaining time and the duration when clicked
6. the Settings button, while the UI layer reports it shown
7. the fullscreen control, whose icon is enter fullscreen, or exit fullscreen while the player is fullscreen

A control under the pointer SHALL show a light round highlight. The mute control and the Settings button SHALL keep a stronger highlight while their popover is open. Each control SHALL show a tooltip naming what it does: Play, Pause, Replay, Mute, Unmute, Settings, Fullscreen or Exit Fullscreen. All labels SHALL stay current while media plays.

While the UI layer reports the controls disabled, every control but fullscreen SHALL be drawn dimmed and SHALL ignore the pointer, and the time labels SHALL be drawn dimmed.

#### Scenario: Bar while paused
- **WHEN** 2-minute media is paused at 0:42 with volume 0.5 and the default playback rates
- **THEN** the control bar shows, in order, a play icon, a level-2 volume icon, `0:42`, the seek bar a little over a third played, `-1:18`, the Settings button and the enter-fullscreen icon

#### Scenario: Hidden parts
- **WHEN** the options hide the volume and fullscreen parts
- **THEN** the control bar shows no mute control or fullscreen control, and the seek bar takes the freed width

#### Scenario: Rate control
- **WHEN** playback rates `[1, 1.5, 2]` are configured and the rate is 1
- **THEN** the control bar has no rate control of its own, and the rate is chosen on the Settings menu's Speed page

#### Scenario: No Settings without rates
- **WHEN** the playback rates are configured empty
- **THEN** the control bar shows no Settings button

#### Scenario: Fixed size
- **WHEN** the element is 640 pixels wide and then 1920 pixels wide
- **THEN** the pill, its controls and its text keep the same height and sizes, and only the seek bar's width changes

#### Scenario: Time display toggle
- **WHEN** 2-minute media is paused at 0:42 and the time display is clicked
- **THEN** it reads `2:00` instead of `-1:18`, and playback does not change

#### Scenario: Ended
- **WHEN** media has played to its end
- **THEN** the play control shows the replay icon, and clicking it restarts playback

#### Scenario: Remaining time follows playback
- **WHEN** 2-minute media plays from 0:10 to 0:20
- **THEN** the time display changes from `-1:50` to `-1:40`

#### Scenario: Shown before the first play
- **WHEN** media is loaded with autoplay off and has not been played
- **THEN** the control bar is shown with the play icon, and no big play button is drawn

#### Scenario: Disabled on error
- **WHEN** a source that cannot be opened is loaded and the play control is clicked
- **THEN** the control bar is shown with its controls dimmed, playback does not change, and the fullscreen control still toggles fullscreen

### Requirement: Control bar visibility and fading

The control bar SHALL be shown and hidden as the UI layer decides, fading in over 0.1 seconds and out over 1 second, as Video.js does. It SHALL hide after the inactivity timeout with no input and no application involvement, and a hidden, faded-out bar SHALL not take pointer input.

#### Scenario: Hides while playing
- **WHEN** media plays in the element and the pointer rests over the video without moving for the inactivity timeout
- **THEN** the control bar fades out over 1 second

#### Scenario: Pointer brings it back
- **WHEN** the control bar is hidden while playing and the pointer moves over the element
- **THEN** the control bar fades in over 0.1 seconds

#### Scenario: Paused
- **WHEN** media is paused
- **THEN** the control bar stays visible

### Requirement: Seek bar interaction

The seek bar SHALL be a thin, rounded track. It SHALL draw the buffered fraction behind the played fraction, as Video.js's load-progress and play-progress bars do, with a round white thumb at the end of the played part. While the pointer is over it, the track SHALL grow taller, and SHALL show the hovered time above the pointer in a small rounded translucent label styled like the skin's popovers. Pressing, dragging and releasing the pointer on it SHALL scrub as the UI layer defines:
- pressing pauses playing media and seeks to the pressed point
- dragging seeks to each point, with the played fraction following the pointer
- releasing seeks exactly and resumes playback if it was playing

A drag that leaves the seek bar, or the element, SHALL keep scrubbing until the pointer button is released. While the duration is unknown, the seek bar SHALL ignore the pointer.

#### Scenario: Hover label
- **WHEN** the pointer is over the middle of the seek bar for 90-second media
- **THEN** a label reading `0:45` is shown above the pointer

#### Scenario: Drag while playing
- **WHEN** media is playing and the seek bar is pressed at 10 %, dragged to 50 % and released
- **THEN** playback pauses during the drag, the played bar follows the pointer, and after release playback resumes near 50 % of the duration

#### Scenario: Drag outside the bar
- **WHEN** a seek bar drag moves above the control bar and is released there
- **THEN** scrubbing continues to the pointer's horizontal position until release, and ends there

#### Scenario: Live stream
- **WHEN** the duration is unknown and the seek bar is clicked
- **THEN** the position does not change

### Requirement: Volume panel interaction

Clicking the mute control SHALL toggle mute as the UI layer defines, including restoring an audible volume when unmuting from volume 0. The volume bar SHALL be a vertical slider in a rounded translucent popover above the mute control (design screen 1b), as Video.js's vertical volume panel places it. The popover SHALL be shown while the pointer is over the mute control or the popover, or while the volume bar has keyboard focus, and hidden otherwise. Pressing or dragging the volume bar SHALL set the volume to the pointer's height along it, from 0 at the bottom to 1 at the top, and unmute. A drag SHALL continue until release even outside the bar, and SHALL keep the popover shown.

#### Scenario: Expand on hover
- **WHEN** the pointer moves over the mute control
- **THEN** the volume popover opens above it, and closes after the pointer leaves both the mute control and the popover

#### Scenario: Drag sets volume
- **WHEN** the player is muted and the volume bar is dragged to 40 % of its height from the bottom
- **THEN** the volume is 0.4, the player is unmuted and the mute icon shows level 2

#### Scenario: Mute toggle
- **WHEN** the volume is 0.8 and the mute control is clicked twice
- **THEN** the player is muted after the first click and unmuted at 0.8 after the second

#### Scenario: Keyboard
- **WHEN** the volume bar is reached with Tab and the up arrow is pressed
- **THEN** the volume popover is shown and the volume rises by 0.1

### Requirement: Overlays

The element's Default skin SHALL show the following, each as the UI layer reports it and only while its part is shown:
- **Big play button**: none. The Default skin's design has no big play button, so it SHALL never be drawn, whatever the options say. The control bar is shown before playback starts instead.
- **Loading spinner** (design screen 1e): a ring, light along a quarter of its length over a faint full circle, turning continuously inside a translucent black circle centred over the video.
- **Error display** (design screen 1f): a rounded translucent card centred over the video, holding an alert icon in a round badge, the title "This video can't be played", mpv's error message beneath it, and a white "Try again" button. Activating Try again SHALL retry as the UI layer defines. The button SHALL take keyboard focus and be activated by Space or Enter. The control bar SHALL stay shown under the card, disabled.

While the control bar is drawn, a faint dark gradient SHALL be drawn over the video beneath it, as the design does, fading with the bar.

#### Scenario: Big play button
- **WHEN** media is loaded with autoplay off and has not been played
- **THEN** no big play button is drawn, and clicking the video or the play control starts playback

#### Scenario: Spinner
- **WHEN** playback waits for the network for longer than 0.3 seconds
- **THEN** a spinner is shown in the centre of the video until playback resumes

#### Scenario: Spinner hidden
- **WHEN** the spinner part is hidden and playback waits for the network for longer than 0.3 seconds
- **THEN** no spinner is shown

#### Scenario: Error
- **WHEN** a source that cannot be opened is loaded
- **THEN** the error card is shown with the title "This video can't be played" and mpv's message, over the disabled control bar

#### Scenario: Try again
- **WHEN** the error card is shown, the source has become readable, and Try again is clicked
- **THEN** the media loads, the card disappears and playback starts

#### Scenario: Error display hidden
- **WHEN** the error display part is hidden and a source that cannot be opened is loaded
- **THEN** no error card is drawn, and the control bar is shown disabled

### Requirement: Pointer input on the video

The element SHALL report pointer movement, presses, releases and leaving, and the pointer entering and leaving the control bar, so that user activity follows Video.js. A click on the video area, outside every control, SHALL toggle play, and a double-click SHALL toggle fullscreen, each unless disabled in the options. Clicks on controls SHALL not count as clicks on the video.

#### Scenario: Click toggles play
- **WHEN** media is paused and the video area is clicked
- **THEN** playback starts

#### Scenario: Click on a control
- **WHEN** the volume bar is clicked while media is playing
- **THEN** the volume changes and playback keeps playing

#### Scenario: Double-click
- **WHEN** the video area is double-clicked
- **THEN** the player enters fullscreen

#### Scenario: Click disabled
- **WHEN** the options disable click and the video area is clicked
- **THEN** playback does not change

### Requirement: Keyboard input and focus

Clicking the element SHALL focus it. The seek bar and the volume bar SHALL each take keyboard focus, and Tab SHALL move focus through the controls in control-bar order. While focus is in the element, a key press SHALL do what the UI layer maps it to for the focused part:
- seek bar keys while the seek bar is focused
- volume bar keys while the volume bar is focused
- Settings menu keys while focus is in the open Settings menu
- the configured hotkeys otherwise

Opening the Settings menu from the keyboard SHALL move focus into the menu, onto its focused entry. When the menu closes while focus is in it, focus SHALL return to the Settings button.

Keys the UI layer does not handle, including every key held with Ctrl, Alt or the platform key, SHALL reach the application unchanged. A focused control SHALL also be activated by Space or Enter, as a button is. The element SHALL offer play/pause, mute and fullscreen as actions that an application can bind or dispatch, for example from its menus, to the focused player.

#### Scenario: Hotkey
- **WHEN** the element is focused, media is playing and `k` is pressed
- **THEN** playback pauses

#### Scenario: Seek bar keys
- **WHEN** the seek bar is focused at 0:30 and the right arrow is pressed
- **THEN** the position becomes 0:35

#### Scenario: Remapped hotkeys
- **WHEN** the options disable hotkeys and `k` is pressed with the element focused
- **THEN** playback does not change and the key reaches the application

#### Scenario: Application shortcut
- **WHEN** the element is focused and Cmd+F (Ctrl+F on Windows and Linux) is pressed
- **THEN** the key reaches the application and the player does not enter fullscreen

#### Scenario: Action from a menu
- **WHEN** the application dispatches the mute action while the element is focused
- **THEN** the player is muted

#### Scenario: Menu from the keyboard
- **WHEN** the Settings button is focused, the rate is 1, and Enter, Enter, Down and Enter are pressed
- **THEN** the menu opens, the Speed page opens, the rate after 1 in the list is selected, and the menu shows its root page with focus still in it

#### Scenario: Escape returns focus
- **WHEN** focus is in the open Settings menu and Escape is pressed
- **THEN** the menu closes, focus is on the Settings button, and the player stays in or out of fullscreen as it was

### Requirement: Fullscreen

The player's fullscreen SHALL be its window's fullscreen, because a GPUI element cannot fill the screen alone. The fullscreen control, the fullscreen hotkey, a double-click and the fullscreen action SHALL enter fullscreen, or leave it while fullscreen. While the player is fullscreen, Escape SHALL also leave it, as a browser does. The element SHALL tell the application each time it enters or leaves fullscreen, so the application can show the player alone, and SHALL also report leaving when the window leaves fullscreen by other means. Fullscreen SHALL work the same on macOS, Windows and Linux.

#### Scenario: Enter and leave
- **WHEN** the fullscreen control is clicked, and then Escape is pressed
- **THEN** the window enters fullscreen and the application is told so, then the window leaves fullscreen and the application is told so

#### Scenario: Left by the window
- **WHEN** the player is fullscreen and the user leaves fullscreen through the window's own control
- **THEN** the application is told the player left fullscreen, and the fullscreen control shows enter again

### Requirement: Icon assets

The crate SHALL provide an asset source that serves every icon its controls draw and falls back to GPUI-Kit's default icon bundle, so an application registers one asset source for both. It SHALL also provide the player's extra icons alone, for applications that compose their own asset source. The required application setup (GPUI-Kit initialisation and the asset source) SHALL be documented.

#### Scenario: One asset source
- **WHEN** an application registers the crate's asset source and initialises GPUI-Kit
- **THEN** every control icon and every GPUI-Kit component icon is drawn

#### Scenario: Composed source
- **WHEN** an application composes the crate's extra icons with its own asset source and GPUI-Kit's default bundle
- **THEN** every control icon is drawn

### Requirement: Skins

The video element SHALL accept a skin, which decides how the controls look, chosen from the skins the crate provides: Default and None. Without one, the element SHALL use Default. Like the options, the skin SHALL take effect when the element's state is created and whenever the element is given a different player. Whatever the skin, the element SHALL keep the same input, keyboard, focus, actions and fullscreen behaviour.

#### Scenario: Default skin
- **WHEN** an application shows a video element without choosing a skin
- **THEN** the Default skin's controls are drawn

#### Scenario: Skin changed later
- **WHEN** the element is rendered again with the same id and player but a different skin
- **THEN** the element keeps the first skin

### Requirement: None skin

The None skin SHALL draw no control, overlay, button, spinner, error display or poster over the video. Clicks, double-clicks, hotkeys and the application's play/pause, mute and fullscreen actions SHALL keep working as the options say.

#### Scenario: Nothing drawn
- **WHEN** the element uses the None skin and media is loaded with autoplay off
- **THEN** only the video area is drawn

#### Scenario: Clicks still work
- **WHEN** the element uses the None skin, the click option is enabled, media is paused and the video is clicked
- **THEN** playback starts

#### Scenario: Clicks disabled
- **WHEN** the element uses the None skin, the click option is disabled and the video is clicked
- **THEN** playback does not change

#### Scenario: Hotkeys
- **WHEN** the element uses the None skin, is focused, and `m` is pressed
- **THEN** the player is muted

### Requirement: Controls platform coverage

The controls, pointer and keyboard input, and fullscreen SHALL work on macOS, Windows and Linux. Unlike Video.js, the pointer cursor SHALL stay visible while the user is inactive, on every platform, until GPUI offers a way to hide it on demand. The Default skin's design blurs the video behind its pill and popovers. GPUI cannot blur what lies behind an element, so on every platform they SHALL be drawn with the design's translucent black and no blur. Both gaps SHALL be documented.

#### Scenario: Windows and Linux
- **WHEN** an application with a video element runs on Windows or Linux and its play control is clicked
- **THEN** the Default skin's control bar is drawn as on macOS, and the media starts playing

#### Scenario: Inactive cursor
- **WHEN** media plays and the user is inactive until the control bar has hidden
- **THEN** the cursor stays visible over the element

#### Scenario: No backdrop blur
- **WHEN** the control bar is shown over detailed video
- **THEN** the video shows through the bar darkened but not blurred

### Requirement: Settings menu

The Default skin SHALL draw the Settings menu while the UI layer reports it open, as a rounded translucent popover above the control bar at its right end, over the Settings button:
- **Root page** (design screen 1c): one row per entry the UI layer lists, each with the item's icon (a gauge for Speed), its label, its current value dimmed, and a chevron pointing right.
- **Item page** (design screen 1d): a header with a Back button (a chevron pointing left) and the page's title, a divider, and the page's entries as a list. The checked entry SHALL be highlighted and SHALL show a check mark. When the entries do not fit the popover's fixed height, the list SHALL scroll and show a scrollbar, and SHALL keep the focused entry in view.

Rows SHALL be highlighted under the pointer, and the entry with keyboard focus SHALL show a focus ring while focus is in the menu. Clicking a row SHALL activate it, and clicking Back SHALL go back, as the UI layer defines. Clicking the Settings button SHALL open or close the menu. A press outside the menu and the Settings button SHALL close the menu, and a click on the video SHALL still do what a click on the video does.

#### Scenario: Open the menu
- **WHEN** the default playback rates are configured, the rate is 1, and the Settings button is clicked
- **THEN** the menu shows one row reading Speed with the value `1x`, above the Settings button, and the Settings button is highlighted

#### Scenario: Pick a speed
- **WHEN** the menu is open, the Speed row is clicked, and then the `1.5x` row is clicked
- **THEN** the Speed page lists every configured rate with `1x` checked, and after the second click the rate is 1.5 and the menu shows its root page with Speed reading `1.5x`

#### Scenario: Long list scrolls
- **WHEN** the Speed page lists the twelve default rates
- **THEN** the list scrolls within the popover with a scrollbar, and moving keyboard focus to `3x` scrolls it into view

#### Scenario: Back
- **WHEN** the Speed page is open and Back is clicked
- **THEN** the menu shows its root page

#### Scenario: Click outside
- **WHEN** the menu is open, media is paused, and the video is clicked away from the menu
- **THEN** the menu closes and playback starts

### Requirement: Default skin typeface

The Default skin SHALL draw all its text in Inter, as the design does, at the weights the design uses, on every platform, whether or not Inter is installed. The crate SHALL embed Inter and make it available without any setup by the application, and SHALL ship Inter's licence (SIL Open Font License 1.1) with it.

#### Scenario: No setup
- **WHEN** an application that registers no fonts shows a video element with the Default skin
- **THEN** the time labels and the menu are drawn in Inter

#### Scenario: Same on every platform
- **WHEN** the same player is shown on macOS, Windows and Linux
- **THEN** its text is drawn in Inter on each
