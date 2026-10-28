## MODIFIED Requirements

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
