## Purpose

Lets a GPUI application show a player's video inside its window as an ordinary element that it can lay out and style, with the video kept upright, letterboxed on black and refreshed as frames arrive, as a Video.js player shows its video element.

## Requirements

### Requirement: Video element

The GPUI crate SHALL provide a video element that an application creates from an element id and a player built with the render video output, and that it can size and position like any other GPUI element. On macOS and Windows, the element SHALL show the player's current video frame over its whole bounds, with the video's display aspect ratio kept and the rest filled black, as the `<video>` area of a Video.js player does. Before the first frame, and while no media is loaded, the element SHALL show black.

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

### Requirement: Sharp video at the element's size

The element SHALL have the player render frames at its bounds in device pixels, taking the window's scale factor into account, and SHALL follow changes of its bounds or scale factor, so that the video is never upscaled by the window compositor from a smaller picture.

#### Scenario: Retina display
- **WHEN** a 640x360-point element is shown on a display with scale factor 2
- **THEN** the player renders frames of 1280x720 pixels

#### Scenario: Window resized
- **WHEN** the window is resized so the element's bounds change while a video is paused
- **THEN** the element shows the paused picture at the new size without the application doing anything

### Requirement: Repaint on new frames

The element SHALL repaint its window whenever the player publishes a new frame, without the application scheduling redraws, and SHALL not keep the window repainting while no new frames are published.

#### Scenario: Playing video
- **WHEN** a video plays in the element
- **THEN** the element shows new frames as they are published, at the video's frame rate up to the display's refresh rate

#### Scenario: Paused video
- **WHEN** playback is paused and nothing else in the window changes
- **THEN** the window stops repainting for the video

### Requirement: Element state keyed by id

The element's state, including its connection to the player's video, SHALL be kept per element id for as long as an element with that id is rendered in consecutive frames, so that re-rendering the parent view does not restart video. Rendering the element with the same id but a different player SHALL switch it to the new player's video. When no element with that id is rendered in a frame, the state SHALL be released and the player SHALL keep playing without video. Two elements with different ids SHALL be able to show two different players in the same window.

#### Scenario: Parent re-renders
- **WHEN** the parent view re-renders many times while a video plays in an element with a fixed id
- **THEN** the video keeps playing without restarting, flicker or black frames

#### Scenario: Different player under the same id
- **WHEN** an element is rendered with one player and then with another under the same id
- **THEN** the element shows the second player's video, and the first player keeps playing without video

#### Scenario: Element removed
- **WHEN** a view stops rendering the element
- **THEN** the element's state is released, and the player keeps playing and responding to control

#### Scenario: Two players
- **WHEN** a window contains two video elements with different ids and different players
- **THEN** each element shows its own player's video

### Requirement: Loading before the element is shown

A video loaded into the player before its element is first rendered SHALL appear in the element once the element is rendered, without the application reloading it.

#### Scenario: Load in the view constructor
- **WHEN** an application loads a video into a player and only then first renders a video element for it
- **THEN** the element shows the video

### Requirement: GPUI platform coverage

The video element SHALL be available on macOS, Windows and Linux, so application code using it builds on all three. On macOS and Windows it SHALL show video. On Linux it SHALL show black. Wherever it shows black, the player's audio and control SHALL keep working. These gaps SHALL be documented.

#### Scenario: macOS
- **WHEN** an application with a video element runs on macOS
- **THEN** the element shows the video

#### Scenario: Windows
- **WHEN** the same application, set up for Windows as documented, runs on Windows on a graphics driver that can share OpenGL textures with Direct3D 11
- **THEN** the element shows the video

#### Scenario: Linux
- **WHEN** the same application runs on Linux
- **THEN** it builds against the unmodified GPUI and runs, the element shows black, and the player's audio plays

### Requirement: Windows video setup

An application that builds for Windows SHALL use a patched GPUI in place of GPUI-Kit's default backend crates. Without the patch, the Windows build SHALL fail with an error that points at the missing patch. On macOS and Linux, the application SHALL build against the unmodified GPUI. The setup SHALL be documented, including that GPUI-Kit's alternative backend is not covered.

#### Scenario: Windows with the patched GPUI
- **WHEN** an application patches GPUI as documented and is built for Windows
- **THEN** it builds, and the video element shows video on Windows

#### Scenario: Windows without the patched GPUI
- **WHEN** an application does not patch GPUI and is built for Windows
- **THEN** the build fails, and the first error names an item whose name and documentation say that the patched GPUI is required

#### Scenario: Other platforms
- **WHEN** an application is built for macOS or Linux without patching GPUI
- **THEN** it builds, and the element behaves as on those platforms without the patch

### Requirement: Windows video unavailable at run time

On Windows, when the player's video cannot be shared with GPUI's renderer, the element SHALL show black, log one warning that names the reason, and keep the player's audio and controls working. This covers a graphics driver that cannot share OpenGL textures with Direct3D 11, OpenGL and GPUI on different graphics adapters, and a frame GPUI cannot open.

#### Scenario: Driver without sharing
- **WHEN** a video element is shown on Windows, and the graphics driver cannot share OpenGL textures with Direct3D 11
- **THEN** the element shows black, one warning saying the driver cannot share with Direct3D 11 is logged, and the player's audio and controls keep working

#### Scenario: Frame GPUI cannot open
- **WHEN** GPUI is given a frame whose texture it cannot open on its own device
- **THEN** that video is drawn black, the rest of the window draws normally, and a warning is logged once, not once per frame
