## MODIFIED Requirements

### Requirement: Surface renderer creation

On macOS, Windows and Linux, the core crate SHALL let an application create a surface renderer for a player built with the render video output, giving a notification to call after each published frame. On macOS the application SHALL be able to choose the surface format (BGRA, or NV12 bi-planar YCbCr) and whether to use the system's hardware or software OpenGL renderer. On Windows the surfaces SHALL be BGRA Direct3D 11 textures on a graphics adapter the application can choose by its locally unique identifier (LUID), defaulting to the first adapter that supports Direct3D 11. On Linux the surfaces SHALL be single-plane RGBA dma-bufs allocated on a DRM render node the application can choose by its device number, defaulting to the first render node on which a renderer can be built, with a memory layout (DRM format modifier) taken from a list the application can give, defaulting to linear. Creating one SHALL need no unsafe code from the application. Creation SHALL fail with a descriptive render error, and leave the player usable, when the player was not built with the render video output, when the player already has a render context or surface renderer, or when no OpenGL context can be created; on Windows also when the OpenGL driver cannot share textures with Direct3D 11, or when no adapter has the requested LUID or the OpenGL driver cannot render on it; on Linux also when no usable render node exists, when no render node has the requested device number, or when the graphics driver cannot allocate or import a dma-buf with any of the allowed layouts.

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

#### Scenario: Linux default render node
- **WHEN** a surface renderer is created on Linux without choosing a render node, on a machine with at least one usable render node
- **THEN** its frames report the device number of the first render node, in order of device number, on which a renderer can be built

#### Scenario: Linux unknown render node
- **WHEN** a surface renderer is created on Linux with a device number that no render node has
- **THEN** creation fails with a render error naming the device number, and the player keeps working

#### Scenario: Linux without a render node
- **WHEN** a surface renderer is created on Linux on a machine with no usable DRM render node
- **THEN** creation fails with a render error saying no DRM render node is available, and the player keeps working

#### Scenario: Linux default layout
- **WHEN** a surface renderer is created on Linux without giving a list of layouts
- **THEN** its frames report the linear modifier

#### Scenario: Linux layouts the driver cannot use
- **WHEN** a surface renderer is created on Linux with a list of layouts none of which the driver can allocate as a single-plane buffer
- **THEN** creation fails with a render error naming the layouts, and the player keeps working

### Requirement: Surface content and encoding

Published frames SHALL show the video scaled to fit the frame with its display aspect ratio kept and the rest filled black, upright when the surface's first row is read as the top of the picture. BGRA frames, on macOS and on Windows, and RGBA frames on Linux, SHALL hold the RGB picture mpv renders. NV12 frames SHALL encode that picture as full-range BT.601 YCbCr, with chroma at half the width and height, and SHALL be tagged with that matrix and range, so that decoding them with the full-range BT.601 matrix gives back the rendered colours within the error of 8-bit 4:2:0 encoding.

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

#### Scenario: Linux test pattern
- **WHEN** a Linux renderer at 160x120 plays mpv's generated colour test pattern
- **THEN** the frame's dma-buf, read with its first row as the top, contains the test pattern upright, not a blank or single-colour image

#### Scenario: Linux solid colours
- **WHEN** a Linux renderer plays a video of solid colour fields
- **THEN** the centre of each field in the frame, read as R, G, B, A bytes in memory order, is within 3% of the field's colour

### Requirement: Hardware decoding through surfaces

On macOS, when the player uses VideoToolbox hardware decoding (its default for the render video output) and the surface renderer uses the hardware OpenGL renderer, decoded frames SHALL reach the surfaces without being copied through CPU memory. On Windows and on Linux, when the player uses its default hardware decoding mode and the graphics driver supports a hardware decoder for the media whose frames OpenGL can use directly, decoded frames SHALL reach the surfaces without being copied through CPU memory. When hardware decoding is not available (for example with a software OpenGL renderer, or a codec the hardware decoder does not support), playback SHALL fall back to software decoding and frames SHALL still be published.

#### Scenario: VideoToolbox on hardware
- **WHEN** a surface renderer using the hardware OpenGL renderer plays an H.264 video on a Mac with a GPU
- **THEN** mpv reports VideoToolbox as the active hardware decoder and frames are published

#### Scenario: Software renderer fallback
- **WHEN** a surface renderer using the software OpenGL renderer plays an H.264 video
- **THEN** frames are published, whichever decoder mpv reports as active

#### Scenario: Windows hardware decoding on a GPU
- **WHEN** a surface renderer on a Windows machine with a GPU whose driver supports hardware H.264 decoding plays an H.264 video
- **THEN** mpv reports an active hardware decoder that is not a copy-back variant, and frames are published

#### Scenario: Linux hardware decoding on a GPU
- **WHEN** a surface renderer on a Linux machine with a GPU whose driver supports hardware H.264 decoding plays an H.264 video
- **THEN** mpv reports an active hardware decoder that is not a copy-back variant, and frames are published

#### Scenario: Linux software rendering
- **WHEN** a surface renderer on Linux whose render node has only a software OpenGL driver plays an H.264 video
- **THEN** frames are published, with mpv decoding in software

### Requirement: Surface platform coverage

Surface renderers SHALL be available on macOS, on both Apple Silicon and Intel, on Windows, on x64 and arm64, and on Linux (glibc), on x64 and arm64. Hardware configurations on which a platform's surface renderer has not been verified SHALL be documented.

#### Scenario: macOS
- **WHEN** the surface tests run on macOS arm64 or x64 with the software OpenGL renderer
- **THEN** they pass with the system frameworks and no extra installation

#### Scenario: Windows
- **WHEN** the Windows surface tests run on a Windows machine whose graphics driver supports OpenGL sharing with Direct3D 11
- **THEN** they pass with the system's graphics driver and no extra installation

#### Scenario: Linux
- **WHEN** the Linux surface tests run on a Linux machine with a DRM render node whose driver allocates and imports dma-bufs, including a software one
- **THEN** they pass with the system's graphics driver and the development packages the README lists

#### Scenario: Windows and Linux
- **WHEN** the core crate is built for Windows or Linux
- **THEN** it builds with that platform's surface renderer, and its documentation lists the hardware on which each renderer is unverified

## ADDED Requirements

### Requirement: Frame sharing through a dma-buf

On Linux, each published frame SHALL provide a file descriptor of its dma-buf, together with its DRM fourcc, modifier, row stride, plane offset and the device number of its render node, so another graphics API on the same machine can import it and sample it without a copy. The descriptor SHALL stay valid while the application holds its frame, also after the renderer is dropped.

#### Scenario: Import into another context
- **WHEN** an application imports a frame's dma-buf with the frame's fourcc, modifier, stride and offset into its own OpenGL context
- **THEN** it gets an RGBA texture of the frame's size whose pixels match the published picture

#### Scenario: Descriptor outlives renderer
- **WHEN** the application holds a published frame, drops the surface renderer, and then imports the frame's dma-buf
- **THEN** the import succeeds and holds the frame's picture

#### Scenario: Single plane
- **WHEN** any frame is published on Linux
- **THEN** it describes exactly one memory plane
