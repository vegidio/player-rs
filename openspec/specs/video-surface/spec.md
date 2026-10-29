## Purpose

Lets an application show a player's video through its own native graphics API by receiving each frame as a GPU surface that the core crate renders on a thread it owns, so that frontends whose renderer is not OpenGL can display video without copying frames through the CPU and without handling OpenGL themselves.

## Requirements

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

### Requirement: Surface size

A surface renderer SHALL render frames at the size in pixels the application last set, and SHALL publish no frame before a size is set. NV12 surfaces SHALL have even width and height, rounding an odd requested dimension up by one. A size change SHALL take effect for the next published frame, also while playback is paused. A width or height of zero SHALL stop frames from being published until a non-zero size is set again.

#### Scenario: No size yet
- **WHEN** a surface renderer's player plays a video and no size has been set
- **THEN** no frame is published

#### Scenario: Frames at the set size
- **WHEN** the size is set to 320x180 while a video plays
- **THEN** each published frame is 320x180

#### Scenario: Odd size with NV12
- **WHEN** an NV12 renderer's size is set to 321x181
- **THEN** published frames are 322x182

#### Scenario: Resize while paused
- **WHEN** the size is changed while playback is paused
- **THEN** a frame at the new size showing the paused picture is published

### Requirement: Frame publication and notification

A surface renderer SHALL render each frame mpv announces, and each redraw mpv requests, into a surface, and SHALL publish it as the latest frame only after the GPU has finished drawing it. Any thread SHALL be able to read the latest frame without waiting on mpv or the render thread. The renderer SHALL call the application's notification after each publication, from a thread that is not one of mpv's, and SHALL not call it after the renderer has been dropped. A published frame SHALL keep its contents unchanged until at least two newer frames have been published.

#### Scenario: Frames while playing
- **WHEN** a surface renderer with a size set plays a video
- **THEN** the notification is called repeatedly, and the latest frame changes between notifications

#### Scenario: Latest frame from another thread
- **WHEN** a thread other than the render thread reads the latest frame while a video plays
- **THEN** it gets a complete frame of the set size without blocking

#### Scenario: Nothing loaded
- **WHEN** a size is set on a surface renderer whose player has no media loaded
- **THEN** a frame filled black is published

#### Scenario: No notification after drop
- **WHEN** a surface renderer is dropped while its player keeps playing
- **THEN** its notification is not called again

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

### Requirement: Video when created after loading

A surface renderer created while its player already has media with a video track loaded SHALL make that video appear in its frames without the application reloading the media, and without interrupting audio playback or, for seekable media, changing the playback position by more than one second. Media without audio, whose load ended with an error because its video could not start without a render context, SHALL be loaded again once, by the core, when a surface renderer exists, whether the load ended before or after the renderer was created; the player SHALL announce the failed load's end with an error, then the media loaded again.

#### Scenario: Renderer created after load
- **WHEN** a player built with the render video output loads a seekable video file with audio, and a surface renderer with a size set is created afterwards
- **THEN** published frames show the video, not black, and playback continues within one second of where it would have been

#### Scenario: Media without audio loaded before the renderer
- **WHEN** a player built with the render video output loads a video without audio, its load ends with an error, and a surface renderer with a size set is created afterwards
- **THEN** the player announces the media loaded again, and published frames show the video, not black

#### Scenario: Media without audio loaded just before the renderer
- **WHEN** a player built with the render video output loads a video without audio and a surface renderer with a size set is created right after, before the load has ended
- **THEN** published frames show the video, not black

### Requirement: Surface renderer lifetime

Dropping a surface renderer SHALL stop its render thread and release its OpenGL resources, return within one second, and leave the player playing without video and responding to control. A surface renderer SHALL keep the player's mpv instance alive while it exists, so the player and the renderer can be dropped in either order. Frames the application still holds SHALL stay valid to read after the renderer is dropped.

#### Scenario: Drop renderer while playing
- **WHEN** a surface renderer is dropped while its player is playing a video
- **THEN** the drop returns within one second, playback continues, and the player can still be paused and queried

#### Scenario: Drop player before renderer
- **WHEN** the last player handle is dropped while its surface renderer is alive, and the renderer is dropped afterwards
- **THEN** neither drop hangs and the mpv instance is released after the second drop

#### Scenario: Frame outlives renderer
- **WHEN** the application holds a published frame and then drops the surface renderer
- **THEN** the frame's size and pixels can still be read

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

### Requirement: Frame sharing with another Direct3D device

On Windows, each published frame SHALL provide a shared handle to its texture and the LUID of its adapter, so that another Direct3D 11 device on the same adapter, in the same process, can open the texture and sample it, without the frame being copied. A handle SHALL stay valid while the application holds its frame, also after the renderer is dropped.

#### Scenario: Open on another device
- **WHEN** an application creates its own Direct3D 11 device on a frame's adapter and opens the frame's shared handle
- **THEN** it gets a BGRA texture of the frame's size whose pixels match the published picture

#### Scenario: Handle outlives renderer
- **WHEN** the application holds a published frame, drops the surface renderer, and then opens the frame's handle on another device
- **THEN** the texture opens and holds the frame's picture
