## MODIFIED Requirements

### Requirement: Player creation with validated options

The core crate SHALL create a player from a builder that accepts the initial autoplay mode, volume, mute state, video output and raw mpv options. The video output SHALL be either none, or rendering by the application through a render context (see the `video-render` capability) or a surface renderer (see the `video-surface` capability). Building SHALL fail with a descriptive error, and create no player, when a value is invalid or mpv cannot be initialised. Unset values SHALL default to autoplay off, volume 1.0, unmuted and the render video output. On macOS, a player built with the render video output, whether set or left as the default, SHALL use VideoToolbox hardware decoding unless a raw mpv option sets the hardware decoding mode. On Windows, such a player SHALL use mpv's safe automatic hardware decoding mode (`auto-safe`), and on Linux VA-API or NVDEC hardware decoding, in that order (`vaapi,nvdec`), unless a raw mpv option sets the hardware decoding mode. With no video output on any platform, hardware decoding SHALL stay off unless a raw mpv option turns it on.

#### Scenario: Default player
- **WHEN** a player is built without setting any option
- **THEN** building succeeds, the player is idle with no media, the volume is 1.0, it is unmuted, the rate is 1.0 and its video output is the render video output

#### Scenario: Default video output without a render context
- **WHEN** a player built without setting a video output loads media with audio, and no render context or surface renderer is ever created for it
- **THEN** the audio plays, no video is shown, mpv opens no window of its own, and the player reports no video size

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
- **WHEN** a player is built with the render video output, set explicitly or left as the default
- **THEN** building succeeds, mpv opens no window of its own, and video is shown only through a render context or surface renderer the application creates

#### Scenario: No video output chosen explicitly
- **WHEN** a player is built with no video output and loads media without audio
- **THEN** the media plays through to its end, as when no video output was the default

#### Scenario: Hardware decoding default on macOS
- **WHEN** a player is built on macOS with the render video output, set explicitly or left as the default, and no raw hardware decoding option
- **THEN** mpv's hardware decoding mode is VideoToolbox

#### Scenario: Hardware decoding default on Windows
- **WHEN** a player is built on Windows with the render video output, set explicitly or left as the default, and no raw hardware decoding option
- **THEN** mpv's hardware decoding mode is `auto-safe`

#### Scenario: Hardware decoding default on Linux
- **WHEN** a player is built on Linux with the render video output, set explicitly or left as the default, and no raw hardware decoding option
- **THEN** mpv's hardware decoding mode is `vaapi,nvdec`

#### Scenario: Hardware decoding overridden
- **WHEN** a player is built with the render video output and the raw mpv option `hwdec` set to `no`
- **THEN** mpv's hardware decoding mode is `no` on every platform

#### Scenario: Hardware decoding elsewhere
- **WHEN** a player is built with no video output on any platform, without a raw hardware decoding option
- **THEN** mpv's hardware decoding mode is `no`
