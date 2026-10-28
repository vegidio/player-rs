## MODIFIED Requirements

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
