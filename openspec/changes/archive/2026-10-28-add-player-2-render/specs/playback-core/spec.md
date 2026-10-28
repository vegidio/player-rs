## MODIFIED Requirements

### Requirement: Player creation with validated options

The core crate SHALL create a player from a builder that accepts the initial autoplay mode, volume, mute state, video output and raw mpv options. The video output SHALL be either none, or rendering by the application through a render context (see the `video-render` capability). Building SHALL fail with a descriptive error, and create no player, when a value is invalid or mpv cannot be initialised. Unset values SHALL default to autoplay off, volume 1.0, unmuted and no video output.

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
- **THEN** building succeeds, mpv opens no window of its own, and video is shown only through a render context the application creates

### Requirement: Shareable player handle

A player SHALL be usable from any thread. Copies of a player handle SHALL control the same mpv instance. Playback SHALL stop when the last copy is dropped, also when media is playing, and dropping SHALL not hang. The mpv instance SHALL be released when the last copy is dropped, or, if a render context for the player is still alive at that moment, when that render context is dropped.

#### Scenario: Copies control the same instance
- **WHEN** a handle is copied and one copy pauses playback
- **THEN** the other copy reports the player as paused

#### Scenario: Control from another thread
- **WHEN** a handle is moved to another thread and used there to change the volume
- **THEN** the change takes effect and every copy reports the new volume

#### Scenario: Drop while playing
- **WHEN** the last copy of a handle is dropped while media is playing and no render context exists
- **THEN** playback stops, the mpv instance is released and the drop returns within one second

#### Scenario: Drop while a render context is alive
- **WHEN** the last copy of a handle is dropped while media is playing and a render context for that player is still alive
- **THEN** the drop returns within one second and playback stops, and the mpv instance is released once the render context is dropped

### Requirement: Playback state snapshot

A player SHALL provide, at any time and from any thread, a snapshot of its current state: whether it is idle, the position, the duration, paused, buffering, volume, muted, rate and the video's display size. Position, duration and video display size SHALL be absent while they are unknown or there is no video. The video display size SHALL be the size at which the video is meant to be shown, with the pixel aspect ratio and any rotation already applied. The snapshot SHALL reflect a change no later than the event announcing that change is delivered.

#### Scenario: Idle player
- **WHEN** a player with no media loaded is queried
- **THEN** it reports idle, no position, no duration and no video display size

#### Scenario: State follows playback
- **WHEN** a player is playing media and a position-change event is received
- **THEN** a snapshot taken right after reports a position no earlier than the one in that event

#### Scenario: Video display size
- **WHEN** a player with the render video output and a render context loads a 160x120 video with square pixels
- **THEN** a video-size-changed event is emitted and the snapshot reports a video display size of 160x120

#### Scenario: Audio-only media
- **WHEN** a player loads media that has no video track
- **THEN** the snapshot reports no video display size

### Requirement: Event subscription

A player SHALL let any number of subscribers receive its events, each from the moment it subscribed. A subscriber SHALL be able to wait for events either by blocking a thread or by awaiting them in any async executor. A subscriber that is dropped SHALL not affect the others. The events SHALL cover at least: media loaded, media ended (with the reason: end of file, stopped, replaced, or error with a message), position changed, duration changed, paused changed, buffering changed, volume changed, mute changed, rate changed, video display size changed, seek started, seek completed, and player idle.

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
