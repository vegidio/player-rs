## MODIFIED Requirements

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

### Requirement: Control bar contents and look

Until the designed Default skin replaces it, the Default skin's control bar SHALL run along the bottom of the element over the video, with Video.js's default-skin look:
- a translucent dark background
- white icons and text
- a fixed height, independent of the element's size

It SHALL contain, from left to right, each only while the UI layer reports its part as shown:
1. the play control, whose icon is play, pause or replay as the UI layer says
2. the volume panel: a mute control, whose icon shows the volume level (0 muted, 1, 2, 3 loud), followed by a horizontal volume bar
3. the seek bar, taking the remaining width
4. the time display, such as `-1:18`, which switches between the remaining time and the duration when clicked
5. the playback-rate control, labelled with the current rate such as `1.5x`, while the Speed item is offered
6. the fullscreen control, whose icon is enter fullscreen, or exit fullscreen while the player is fullscreen

Each control SHALL show a tooltip naming what it does: Play, Pause, Replay, Mute, Unmute, Playback Rate, Fullscreen or Exit Fullscreen. All labels SHALL stay current while media plays.

#### Scenario: Bar while paused
- **WHEN** 2-minute media is paused at 0:42 with volume 0.5 and no playback rates configured
- **THEN** the control bar shows, in order, a play icon, a level-2 volume icon and the volume bar, the seek bar a little over a third played, `-1:18`, and the enter-fullscreen icon, with no rate control

#### Scenario: Hidden parts
- **WHEN** the options hide the volume and fullscreen parts
- **THEN** the control bar shows no mute control, volume bar or fullscreen control, and the seek bar takes the freed width

#### Scenario: Rate control
- **WHEN** playback rates `[1, 1.5, 2]` are configured, the rate is 1 and the rate control is clicked
- **THEN** the rate becomes 1.5 and the control reads `1.5x`

#### Scenario: Time display toggle
- **WHEN** 2-minute media is paused at 0:42 and the time display is clicked
- **THEN** it reads `2:00` instead of `-1:18`, and playback does not change

#### Scenario: Ended
- **WHEN** media has played to its end
- **THEN** the play control shows the replay icon, and clicking it restarts playback

#### Scenario: Remaining time follows playback
- **WHEN** 2-minute media plays from 0:10 to 0:20
- **THEN** the time display changes from `-1:50` to `-1:40`

### Requirement: Overlays

The element's Default skin SHALL show the following, each as the UI layer reports it and only while its part is shown:
- **Big play button**: a Video.js big play button, a play icon on a translucent rounded box, centred over the video as in Video.js 8's default skin. Clicking it SHALL start playback.
- **Loading spinner**: a spinning indicator centred over the video.
- **Error display**: when media failed to load, a dark layer over the video with the error message centred on it.

#### Scenario: Big play button
- **WHEN** media is loaded with autoplay off and has not been played
- **THEN** the big play button is shown in the centre of the element, and clicking it starts playback and hides it

#### Scenario: Spinner
- **WHEN** playback waits for the network for longer than 0.3 seconds
- **THEN** a spinner is shown in the centre of the video until playback resumes

#### Scenario: Spinner hidden
- **WHEN** the spinner part is hidden and playback waits for the network for longer than 0.3 seconds
- **THEN** no spinner is shown

#### Scenario: Error
- **WHEN** a source that cannot be opened is loaded
- **THEN** the error message is shown over the video, and the control bar is hidden

## ADDED Requirements

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
