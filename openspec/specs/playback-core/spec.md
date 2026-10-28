## Purpose

Defines the toolkit-agnostic player that every frontend drives: how it is created and configured, how media is loaded and controlled, and how its state and events are observed, so that an app changing frontends keeps all of its playback code unchanged.

## Requirements

### Requirement: Player creation with validated options

The core crate SHALL create a player from a builder that accepts the initial autoplay mode, volume, mute state, video output and raw mpv options. The video output SHALL be either none, or rendering by the application through a render context (see the `video-render` capability) or a surface renderer (see the `video-surface` capability). Building SHALL fail with a descriptive error, and create no player, when a value is invalid or mpv cannot be initialised. Unset values SHALL default to autoplay off, volume 1.0, unmuted and no video output. On macOS, a player built with the render video output SHALL use VideoToolbox hardware decoding unless a raw mpv option sets the hardware decoding mode; on other platforms, and with no video output, hardware decoding SHALL stay off unless a raw mpv option turns it on.

#### Scenario: Default player
- **WHEN** a player is built without setting any option
- **THEN** building succeeds, the player is idle with no media, the volume is 1.0, it is unmuted and the rate is 1.0

#### Scenario: Volume outside the accepted range
- **WHEN** a player is built with a volume below 0.0, above 1.0, or NaN
- **THEN** building fails with an error naming the volume option and the accepted range `0.0..=1.0`

#### Scenario: Invalid raw mpv option
- **WHEN** a player is built with a raw mpv option whose name mpv does not know, or whose value mpv rejects
- **THEN** building fails with an error naming that option and mpv's reason

#### Scenario: Embedding defaults
- **WHEN** a player is built without raw options
- **THEN** mpv does not handle keyboard or mouse input itself and draws no on-screen controller, so the embedding frontend owns all user interaction

#### Scenario: Render video output
- **WHEN** a player is built with the render video output
- **THEN** building succeeds, mpv opens no window of its own, and video is shown only through a render context or surface renderer the application creates

#### Scenario: Hardware decoding default on macOS
- **WHEN** a player is built on macOS with the render video output and no raw hardware decoding option
- **THEN** mpv's hardware decoding mode is VideoToolbox

#### Scenario: Hardware decoding overridden
- **WHEN** a player is built with the render video output and the raw mpv option `hwdec` set to `no`
- **THEN** mpv's hardware decoding mode is `no` on every platform

#### Scenario: Hardware decoding elsewhere
- **WHEN** a player is built with no video output on any platform, or with the render video output on Windows or Linux, without a raw hardware decoding option
- **THEN** mpv's hardware decoding mode is `no`

### Requirement: Shareable player handle

A player SHALL be usable from any thread. Copies of a player handle SHALL control the same mpv instance, and the application SHALL be able to tell whether two handles are copies of the same player. Playback SHALL stop when the last copy is dropped, also when media is playing, and dropping SHALL not hang. The mpv instance SHALL be released when the last copy is dropped, or, if a render context for the player is still alive at that moment, when that render context is dropped.

#### Scenario: Copies control the same instance
- **WHEN** a handle is copied and one copy pauses playback
- **THEN** the other copy reports the player as paused

#### Scenario: Same-player check
- **WHEN** a handle is compared with its copy, and with a handle of a separately built player
- **THEN** the copy is reported as the same player and the other as a different one

#### Scenario: Control from another thread
- **WHEN** a handle is moved to another thread and used there to change the volume
- **THEN** the change takes effect and every copy reports the new volume

#### Scenario: Drop while playing
- **WHEN** the last copy of a handle is dropped while media is playing and no render context exists
- **THEN** playback stops, the mpv instance is released and the drop returns within one second

#### Scenario: Drop while a render context is alive
- **WHEN** the last copy of a handle is dropped while media is playing and a render context for that player is still alive
- **THEN** the drop returns within one second and playback stops, and the mpv instance is released once the render context is dropped

### Requirement: Media sources

The core crate SHALL describe media to load as either a URL or a local file path. A URL SHALL be accepted only if it starts with a scheme followed by `://` or is an mpv input-device URL such as `av://lavfi:...`. A file path SHALL be rejected if it is not valid UTF-8. A file path SHALL always be played as a local file, even if its name looks like a URL.

#### Scenario: Valid URL
- **WHEN** a source is created from `https://example.com/movie.mp4`
- **THEN** creation succeeds

#### Scenario: Invalid URL
- **WHEN** a source is created as a URL from an empty string or from a string with no scheme, such as `movie.mp4`
- **THEN** creation fails with an error that includes the rejected string

#### Scenario: File path that looks like a URL
- **WHEN** on macOS or Linux, a source is created from the relative file path `http://clip.mp4` (a file `clip.mp4` inside a directory named `http:`) and loaded
- **THEN** the player opens that local file, not a network resource

### Requirement: Loading media

Loading a source SHALL replace whatever is currently loaded. Loading SHALL return as soon as the request is accepted. Success or failure SHALL be reported later through events and state. The autoplay mode SHALL decide what happens once media is loaded: `Off` leaves it paused, `On` starts playback, `Muted` mutes the player and starts playback.

#### Scenario: Successful load with autoplay off
- **WHEN** a player with autoplay off loads a playable source
- **THEN** a loaded event is emitted, the state reports the media duration, and the player is paused at the start

#### Scenario: Autoplay on
- **WHEN** a player with autoplay on loads a playable source
- **THEN** playback starts without any further call and the position advances

#### Scenario: Autoplay muted
- **WHEN** a player with autoplay muted loads a playable source
- **THEN** the player is muted and playback starts without any further call

#### Scenario: Unplayable source
- **WHEN** a player loads a source that does not exist or cannot be decoded
- **THEN** an ended event with an error reason is emitted, carrying mpv's message, and the player returns to idle

#### Scenario: Load replaces current media
- **WHEN** a player that is playing one source loads another
- **THEN** the first source ends with a reason saying it was replaced, and the second source is loaded

### Requirement: Playback control

A player SHALL support play, pause, toggling pause, stopping, and seeking either to an absolute position or by a relative offset. A seek SHALL be clamped by mpv to the media's bounds. A player SHALL also support a fast seek to an absolute position, for scrubbing, that may land on the nearest keyframe instead of the exact position. A control call made while no media is loaded SHALL have no effect and return no error.

#### Scenario: Pause and resume
- **WHEN** playing media is paused and later resumed
- **THEN** the position does not advance while paused and advances again after resuming

#### Scenario: Absolute seek
- **WHEN** a player with loaded media seeks to 1.0 s
- **THEN** a seek-completed event is emitted and the reported position is within 0.1 s of 1.0 s

#### Scenario: Fast seek
- **WHEN** a player with loaded media fast-seeks to 1.0 s
- **THEN** a seek-completed event is emitted and the reported position is within 1.0 s of 1.0 s

#### Scenario: Relative seek past the end
- **WHEN** a player seeks forward by more than the remaining duration
- **THEN** playback reaches the end of the media and does not fail

#### Scenario: Stop
- **WHEN** playing media is stopped
- **THEN** an ended event with a stopped reason is emitted and the player becomes idle with no media

#### Scenario: Control with no media
- **WHEN** a player with no media loaded is asked to play, pause, seek or fast-seek
- **THEN** the call returns no error and the player stays idle

### Requirement: Volume, mute and rate clamping at runtime

At runtime, a player SHALL accept any volume or playback rate and clamp it to the supported range, as Video.js does: volume to `0.0..=1.0`, rate to `0.01..=100.0` (mpv's speed range). Mute SHALL be independent of volume.

#### Scenario: Volume above the range
- **WHEN** the volume is set to 1.3 at runtime
- **THEN** no error is returned and the reported volume is 1.0

#### Scenario: Rate below the range
- **WHEN** the playback rate is set to 0.0 at runtime
- **THEN** no error is returned and the reported rate is 0.01

#### Scenario: Mute keeps the volume
- **WHEN** a player with volume 0.6 is muted and then unmuted
- **THEN** the reported volume stays 0.6 throughout and the player reports muted only between the two calls

### Requirement: Playback state snapshot

A player SHALL provide, at any time and from any thread, a snapshot of its current state, covering:
- whether it is idle
- the position and the duration
- paused and buffering
- the volume, mute and the rate
- the video's display size
- the buffered position

Position, duration, video display size and buffered position SHALL be absent while they are unknown or there is no video. The video display size SHALL be the size at which the video is meant to be shown, with the pixel aspect ratio and any rotation already applied. The buffered position SHALL be the media time up to which data is cached ahead of playback, and SHALL be absent when the player keeps no cache for the media. The snapshot SHALL reflect a change no later than the event announcing that change is delivered.

#### Scenario: Idle player
- **WHEN** a player with no media loaded is queried
- **THEN** it reports idle, no position, no duration, no video display size and no buffered position

#### Scenario: State follows playback
- **WHEN** a player is playing media and a position-change event is received
- **THEN** a snapshot taken right after reports a position no earlier than the one in that event

#### Scenario: Video display size
- **WHEN** a player with the render video output and a render context loads a 160x120 video with square pixels
- **THEN** a video-size-changed event is emitted and the snapshot reports a video display size of 160x120

#### Scenario: Audio-only media
- **WHEN** a player loads media that has no video track
- **THEN** the snapshot reports no video display size

#### Scenario: Buffered position
- **WHEN** a player with its cache enabled loads a two-second local file and has read it to the end
- **THEN** a buffered-changed event is emitted and the snapshot reports a buffered position within 0.1 s of the duration

### Requirement: Event subscription

A player SHALL let any number of subscribers receive its events, each from the moment it subscribed. A subscriber SHALL be able to wait for events either by blocking a thread or by awaiting them in any async executor. A subscriber that is dropped SHALL not affect the others. The events SHALL cover at least:
- media loaded
- media ended, with the reason: end of file, stopped, replaced, or error with a message
- position changed, duration changed, buffered position changed
- paused changed, buffering changed
- volume changed, mute changed, rate changed
- video display size changed
- seek started, seek completed
- player idle

#### Scenario: Two subscribers
- **WHEN** two subscribers exist and media is loaded
- **THEN** both receive the loaded event

#### Scenario: Late subscriber
- **WHEN** a subscriber is created after media has loaded
- **THEN** it does not receive the earlier loaded event and receives events emitted after it subscribed

#### Scenario: Dropped subscriber
- **WHEN** one of two subscribers is dropped and playback continues
- **THEN** the other keeps receiving events

#### Scenario: Async subscriber
- **WHEN** a subscriber awaits the next event inside an async task
- **THEN** the task resumes when the next event is emitted, without polling

#### Scenario: Media plays to the end
- **WHEN** media plays until its end
- **THEN** an ended event with the end-of-file reason is emitted, and the media stays loaded and paused at its end (the player does not become idle), as in Video.js

#### Scenario: Play after the end
- **WHEN** media has played to its end and play is requested
- **THEN** playback restarts from the beginning, as in Video.js

### Requirement: Raw property and command access

A player SHALL let callers read and write any mpv property by name, as a boolean, integer, floating-point number or string, and run any mpv command given as a list of arguments, without the frontend calling libmpv. Failures SHALL return an error carrying mpv's message and the property or command name.

#### Scenario: Read a typed property
- **WHEN** the `speed` property is read as a floating-point number
- **THEN** the current playback rate is returned

#### Scenario: Unknown property
- **WHEN** a property name mpv does not know is read
- **THEN** an error is returned naming the property and carrying mpv's message

#### Scenario: Run a command
- **WHEN** the command `["set", "volume", "50"]` is run
- **THEN** it succeeds and the reported volume is 0.5

### Requirement: Consistent behaviour on all platforms

The player SHALL behave identically on macOS, Windows and Linux for every requirement in this capability, and SHALL run with no audio device when the null audio output is selected through a raw mpv option.

#### Scenario: Headless playback on CI
- **WHEN** the player loads mpv's generated test source with null audio and no video output on any supported platform
- **THEN** the media loads, plays, seeks and ends with the same events and state as described above
