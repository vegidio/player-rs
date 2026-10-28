## Purpose

Gives the GPUI video element the controls and interaction of a Video.js player: the control bar, the big play button, the spinner and error display, pointer and keyboard input, and fullscreen. All of it follows the toolkit-agnostic UI layer's rules.

## Requirements

### Requirement: Player options on the element

The video element SHALL accept the player UI options and SHALL default to the default options, under which controls are shown. The options SHALL take effect when the element's state is created and whenever the element is given a different player; changing them on a later render with the same id and player SHALL have no effect, as Video.js reads its options once at construction. With controls disabled, the element SHALL draw no control, overlay or button, and SHALL ignore clicks, double-clicks and keys meant for the player.

#### Scenario: Default options
- **WHEN** an application shows a video element without passing options
- **THEN** the element shows Video.js controls

#### Scenario: Controls disabled
- **WHEN** the element is given options with controls disabled and the video is clicked
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

The control bar SHALL run along the bottom of the element over the video, with Video.js's default-skin look:
- a translucent dark background
- white icons and text
- a fixed height, independent of the element's size

It SHALL contain, from left to right:
1. the play control, whose icon is play, pause or replay as the UI layer says
2. the volume panel: a mute control, whose icon shows the volume level (0 muted, 1, 2, 3 loud), followed by a horizontal volume bar
3. the seek bar, taking the remaining width
4. the remaining-time label, such as `-1:18`
5. the playback-rate control, labelled with the current rate such as `1.5x`, only when playback rates are configured
6. the fullscreen control, whose icon is enter fullscreen, or exit fullscreen while the player is fullscreen

Each control SHALL show a tooltip naming what it does: Play, Pause, Replay, Mute, Unmute, Playback Rate, Fullscreen or Exit Fullscreen. As in Video.js's default skin, the current time and duration SHALL not be shown. All labels SHALL stay current while media plays.

#### Scenario: Bar while paused
- **WHEN** 2-minute media is paused at 0:42 with volume 0.5 and no playback rates configured
- **THEN** the control bar shows, in order, a play icon, a level-2 volume icon and the volume bar, the seek bar a little over a third played, `-1:18`, and the enter-fullscreen icon, with no rate control

#### Scenario: Rate control
- **WHEN** playback rates `[1, 1.5, 2]` are configured, the rate is 1 and the rate control is clicked
- **THEN** the rate becomes 1.5 and the control reads `1.5x`

#### Scenario: Ended
- **WHEN** media has played to its end
- **THEN** the play control shows the replay icon, and clicking it restarts playback

#### Scenario: Remaining time follows playback
- **WHEN** 2-minute media plays from 0:10 to 0:20
- **THEN** the remaining-time label changes from `-1:50` to `-1:40`

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

The seek bar SHALL draw the buffered fraction behind the played fraction, as Video.js's load-progress and play-progress bars do. While the pointer is over it, the bar SHALL grow taller, and SHALL show the hovered time in a label above the pointer. Pressing, dragging and releasing the pointer on it SHALL scrub as the UI layer defines:
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

Clicking the mute control SHALL toggle mute as the UI layer defines, including restoring an audible volume when unmuting from volume 0. The volume bar SHALL be collapsed until the pointer is over the volume panel or the volume bar has keyboard focus, and SHALL then expand, as Video.js's horizontal volume panel does. Pressing or dragging the volume bar SHALL set the volume to the pointer's fraction and unmute, and a drag SHALL continue until release even outside the bar.

#### Scenario: Expand on hover
- **WHEN** the pointer moves over the mute control
- **THEN** the volume bar expands beside it, and collapses after the pointer leaves the volume panel

#### Scenario: Drag sets volume
- **WHEN** the player is muted and the volume bar is dragged to 40 %
- **THEN** the volume is 0.4, the player is unmuted and the mute icon shows level 2

#### Scenario: Mute toggle
- **WHEN** the volume is 0.8 and the mute control is clicked twice
- **THEN** the player is muted after the first click and unmuted at 0.8 after the second

### Requirement: Overlays

The element SHALL show the following, each as the UI layer reports it:
- **Big play button**: while it is shown, a Video.js big play button, a play icon on a translucent rounded box, centred over the video as in Video.js 8's default skin. Clicking it SHALL start playback.
- **Loading spinner**: a spinning indicator centred over the video while the spinner is shown.
- **Error display**: when media failed to load, a dark layer over the video with the error message centred on it.

#### Scenario: Big play button
- **WHEN** media is loaded with autoplay off and has not been played
- **THEN** the big play button is shown in the centre of the element, and clicking it starts playback and hides it

#### Scenario: Spinner
- **WHEN** playback waits for the network for longer than 0.3 seconds
- **THEN** a spinner is shown in the centre of the video until playback resumes

#### Scenario: Error
- **WHEN** a source that cannot be opened is loaded
- **THEN** the error message is shown over the video, and the control bar is hidden

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
- the configured hotkeys otherwise

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

### Requirement: Controls platform coverage

The controls, pointer and keyboard input, and fullscreen SHALL work on macOS, Windows and Linux. On Windows and Linux they SHALL be drawn over the black video area until those platforms get video surfaces, and SHALL control playback fully. Unlike Video.js, the pointer cursor SHALL stay visible while the user is inactive, on every platform, until GPUI offers a way to hide it on demand. This gap SHALL be documented.

#### Scenario: Windows and Linux
- **WHEN** an application with a video element runs on Windows or Linux and its play control is clicked
- **THEN** the control bar is drawn over black, and the media's audio starts playing

#### Scenario: Inactive cursor
- **WHEN** media plays and the user is inactive until the control bar has hidden
- **THEN** the cursor stays visible over the element
