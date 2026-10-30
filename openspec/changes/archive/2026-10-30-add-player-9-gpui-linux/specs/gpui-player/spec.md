## RENAMED Requirements

- FROM: `### Requirement: Windows video setup`
- TO: `### Requirement: Patched GPUI setup`

## MODIFIED Requirements

### Requirement: Video element

The GPUI crate SHALL provide a video element that an application creates from an element id and a player built with the render video output, and that it can size and position like any other GPUI element. On macOS, Windows and Linux, the element SHALL show the player's current video frame over its whole bounds, with the video's display aspect ratio kept and the rest filled black, as the `<video>` area of a Video.js player does. Before the first frame, and while no media is loaded, the element SHALL show black.

#### Scenario: Video shown
- **WHEN** a window contains a video element for a player that plays a video
- **THEN** the element shows the video, upright and in its own colours

#### Scenario: Letterboxed in a wider element
- **WHEN** a 4:3 video plays in a 16:9 element
- **THEN** the video fills the element's height, centred, with black bars on the left and right

#### Scenario: No media
- **WHEN** a video element's player has no media loaded
- **THEN** the element shows black

#### Scenario: Controls over the video on Windows
- **WHEN** a video plays in an element with controls on Windows
- **THEN** the control bar and the other Video.js overlays are drawn over the video, as on macOS

#### Scenario: Controls over the video on Linux
- **WHEN** a video plays in an element with controls on Linux
- **THEN** the control bar and the other Video.js overlays are drawn over the video, as on macOS

### Requirement: GPUI platform coverage

The video element SHALL be available on macOS, Windows and Linux, so application code using it builds on all three, and SHALL show video on all three. Wherever it shows black instead, because the platform's graphics stack cannot share the video with GPUI, the player's audio and control SHALL keep working. These gaps, and the graphics stacks on which showing video has not been verified, SHALL be documented.

#### Scenario: macOS
- **WHEN** an application with a video element runs on macOS
- **THEN** the element shows the video

#### Scenario: Windows
- **WHEN** the same application, set up for Windows as documented, runs on Windows on a graphics driver that can share OpenGL textures with Direct3D 11
- **THEN** the element shows the video

#### Scenario: Linux
- **WHEN** the same application, set up for Linux as documented, runs on Linux with a DRM device the player can render on, and GPUI renders with a graphics driver that can import dma-bufs
- **THEN** the element shows the video

#### Scenario: Linux on GPUI's OpenGL fallback
- **WHEN** the same application runs on Linux and GPUI renders through OpenGL instead of Vulkan, on a driver that can import dma-bufs into EGL
- **THEN** the element shows the video

#### Scenario: Linux software Vulkan without layout modifiers
- **WHEN** GPUI renders on Linux through a Vulkan driver that imports dma-bufs but lacks explicit memory layouts (DRM format modifiers), as Mesa's software Vulkan driver does, and the player's buffers are linear
- **THEN** the element shows the video

### Requirement: Patched GPUI setup

An application that builds for Windows or Linux SHALL use a patched GPUI in place of GPUI-Kit's default backend crates: on Windows the patched core and Windows crates, and on Linux the patched core and wgpu renderer crates. Without the patch, the Windows or Linux build SHALL fail with an error that points at the missing patch. On macOS, the application SHALL build against the unmodified GPUI. The setup SHALL be documented, including that GPUI-Kit's alternative backend is not covered.

#### Scenario: Windows with the patched GPUI
- **WHEN** an application patches GPUI as documented and is built for Windows
- **THEN** it builds, and the video element shows video on Windows

#### Scenario: Linux with the patched GPUI
- **WHEN** an application patches GPUI as documented and is built for Linux
- **THEN** it builds, and the video element shows video on Linux

#### Scenario: Windows without the patched GPUI
- **WHEN** an application does not patch GPUI and is built for Windows
- **THEN** the build fails, and the first error names an item whose name and documentation say that the patched GPUI is required

#### Scenario: Linux without the patched GPUI
- **WHEN** an application does not patch GPUI and is built for Linux
- **THEN** the build fails, and the first error names an item whose name and documentation say that the patched GPUI is required

#### Scenario: Other platforms
- **WHEN** an application is built for macOS without patching GPUI
- **THEN** it builds, and the element shows video as it does on macOS

## ADDED Requirements

### Requirement: Linux video unavailable at run time

On Linux, when the player's video cannot be shared with GPUI's renderer, the element SHALL show black, log one warning that names the reason, and keep the player's audio and controls working. This covers a machine with no DRM device the player can render on, and a GPUI driver that cannot import the player's buffers.

#### Scenario: No DRM device
- **WHEN** a video element is shown on Linux on a machine with no usable DRM render node, as WSL2 without the `vgem` module
- **THEN** the element shows black, one warning saying no render node is available is logged, and the player's audio and controls keep working

#### Scenario: GPUI driver without dma-buf import
- **WHEN** GPUI renders on Linux through a driver that cannot import dma-bufs, as Mesa's Direct3D 12 drivers on WSL2
- **THEN** that video is drawn black, the rest of the window draws normally, and a warning is logged once, not once per frame

#### Scenario: Buffer GPUI cannot import
- **WHEN** GPUI is given a frame whose dma-buf it cannot import on its own device
- **THEN** that video is drawn black, the rest of the window draws normally, and a warning is logged once, not once per frame

### Requirement: Render device and layout from GPUI

On Linux, the element SHALL have the player render on the DRM device GPUI renders with, using a memory layout GPUI can import, whenever GPUI's renderer reports them. When GPUI reports neither, or the player cannot render on GPUI's device, the element SHALL fall back to the player's default device and the linear layout, and SHALL still show video wherever GPUI can import that.

#### Scenario: GPUI reports its device
- **WHEN** GPUI renders through a Vulkan driver that reports its DRM render node and the layouts it imports
- **THEN** the player's frames are rendered on that render node, in one of those layouts

#### Scenario: GPUI reports nothing
- **WHEN** GPUI renders through a driver that reports neither its render node nor its layouts, as Mesa's software Vulkan driver
- **THEN** the player's frames are rendered on the player's default render node in the linear layout, and the element shows them

#### Scenario: Player cannot render on GPUI's device
- **WHEN** GPUI reports a render node on which the player's renderer cannot be built
- **THEN** the element falls back to the player's default render node and the linear layout, and shows video if GPUI can import it
