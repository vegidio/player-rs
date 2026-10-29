## MODIFIED Requirements

### Requirement: Surface renderer creation

On macOS and Windows, the core crate SHALL let an application create a surface renderer for a player built with the render video output, giving a notification to call after each published frame. On macOS the application SHALL be able to choose the surface format (BGRA, or NV12 bi-planar YCbCr) and whether to use the system's hardware or software OpenGL renderer. On Windows the surfaces SHALL be BGRA Direct3D 11 textures on a graphics adapter the application can choose by its locally unique identifier (LUID), defaulting to the first adapter that supports Direct3D 11. Creating one SHALL need no unsafe code from the application. Creation SHALL fail with a descriptive render error, and leave the player usable, when the player was not built with the render video output, when the player already has a render context or surface renderer, or when no OpenGL context can be created; on Windows also when the OpenGL driver cannot share textures with Direct3D 11, or when no adapter has the requested LUID or the OpenGL driver cannot render on it.

#### Scenario: Create for a render player
- **WHEN** a surface renderer is created for a player built with the render video output
- **THEN** creation succeeds

#### Scenario: Player without render output
- **WHEN** a surface renderer is created for a player built with no video output
- **THEN** creation fails with a render error saying the player was not built for rendering, and the player keeps working

#### Scenario: Second renderer for the same player
- **WHEN** a surface renderer is created for a player that already has a live surface renderer or render context
- **THEN** creation fails with a render error, and the existing one keeps working

#### Scenario: Recreate after drop
- **WHEN** a player's surface renderer is dropped and a new one is created for the same player
- **THEN** creation succeeds and the new renderer publishes frames

#### Scenario: Windows default adapter
- **WHEN** a surface renderer is created on Windows without choosing an adapter
- **THEN** its frames report the LUID of the first DXGI adapter that supports Direct3D 11

#### Scenario: Windows unknown adapter
- **WHEN** a surface renderer is created on Windows with an adapter LUID that no adapter has
- **THEN** creation fails with a render error naming the LUID, and the player keeps working

#### Scenario: Windows driver without Direct3D sharing
- **WHEN** a surface renderer is created on Windows and the OpenGL driver cannot share textures with Direct3D 11
- **THEN** creation fails with a render error saying the OpenGL driver cannot share with Direct3D 11, and the player keeps working

### Requirement: Surface content and encoding

Published frames SHALL show the video scaled to fit the frame with its display aspect ratio kept and the rest filled black, upright when the surface's first row is read as the top of the picture. BGRA frames, on macOS and on Windows, SHALL hold the RGB picture mpv renders. NV12 frames SHALL encode that picture as full-range BT.601 YCbCr, with chroma at half the width and height, and SHALL be tagged with that matrix and range, so that decoding them with the full-range BT.601 matrix gives back the rendered colours within the error of 8-bit 4:2:0 encoding.

#### Scenario: BGRA test pattern
- **WHEN** a BGRA renderer at 160x120 plays mpv's generated colour test pattern
- **THEN** the frame contains the test pattern, upright, not a blank or single-colour image

#### Scenario: NV12 colours
- **WHEN** an NV12 renderer plays a video of solid colour fields and a frame is decoded with the full-range BT.601 matrix
- **THEN** the centre of each field is within 3% of the colour a BGRA renderer produces for the same field

#### Scenario: NV12 orientation
- **WHEN** an NV12 frame of mpv's colour test pattern is decoded
- **THEN** it matches the upright BGRA frame of the same media, within the encoding tolerance

#### Scenario: Letterboxing
- **WHEN** a 160x120 video is rendered into 320x120 surfaces
- **THEN** the video fills the middle 160x120 area and the bars to its left and right are black

#### Scenario: Windows solid colours
- **WHEN** a Windows renderer plays a video of solid colour fields
- **THEN** the centre of each field in the frame is within 3% of the field's colour

### Requirement: Hardware decoding through surfaces

On macOS, when the player uses VideoToolbox hardware decoding (its default for the render video output) and the surface renderer uses the hardware OpenGL renderer, decoded frames SHALL reach the surfaces without being copied through CPU memory. On Windows, when the player uses its default hardware decoding mode and the graphics driver supports a hardware decoder for the media whose frames OpenGL can use directly, decoded frames SHALL reach the surfaces without being copied through CPU memory. When hardware decoding is not available (for example with the software OpenGL renderer, or a codec the hardware decoder does not support), playback SHALL fall back to software decoding and frames SHALL still be published.

#### Scenario: VideoToolbox on hardware
- **WHEN** a surface renderer using the hardware OpenGL renderer plays an H.264 video on a Mac with a GPU
- **THEN** mpv reports VideoToolbox as the active hardware decoder and frames are published

#### Scenario: Software renderer fallback
- **WHEN** a surface renderer using the software OpenGL renderer plays an H.264 video
- **THEN** frames are published, whichever decoder mpv reports as active

#### Scenario: Windows hardware decoding on a GPU
- **WHEN** a surface renderer on a Windows machine with a GPU whose driver supports hardware H.264 decoding plays an H.264 video
- **THEN** mpv reports an active hardware decoder that is not a copy-back variant, and frames are published

### Requirement: Surface platform coverage

Surface renderers SHALL be available on macOS, on both Apple Silicon and Intel, and on Windows, on x64 and arm64. They SHALL be absent from the API on Linux until it gets its own surface sharing, and the absence SHALL be documented.

#### Scenario: macOS
- **WHEN** the surface tests run on macOS arm64 or x64 with the software OpenGL renderer
- **THEN** they pass with the system frameworks and no extra installation

#### Scenario: Windows
- **WHEN** the Windows surface tests run on a Windows machine whose graphics driver supports OpenGL sharing with Direct3D 11
- **THEN** they pass with the system's graphics driver and no extra installation

#### Scenario: Windows and Linux
- **WHEN** the core crate is built for Windows or Linux
- **THEN** on Windows it builds with the surface renderer, and on Linux it builds without it, with its documentation stating that surfaces are available on macOS and Windows only for now

## ADDED Requirements

### Requirement: Frame sharing with another Direct3D device

On Windows, each published frame SHALL provide a shared handle to its texture and the LUID of its adapter, so that another Direct3D 11 device on the same adapter, in the same process, can open the texture and sample it, without the frame being copied. A handle SHALL stay valid while the application holds its frame, also after the renderer is dropped.

#### Scenario: Open on another device
- **WHEN** an application creates its own Direct3D 11 device on a frame's adapter and opens the frame's shared handle
- **THEN** it gets a BGRA texture of the frame's size whose pixels match the published picture

#### Scenario: Handle outlives renderer
- **WHEN** the application holds a published frame, drops the surface renderer, and then opens the frame's handle on another device
- **THEN** the texture opens and holds the frame's picture
