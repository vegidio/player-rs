## MODIFIED Requirements

### Requirement: Video element

The GPUI crate SHALL provide a video element that an application creates from an element id and a player built with the render video output, and that it can size and position like any other GPUI element. On macOS, and on Windows when the application has turned Windows video on (see "Windows video setup"), the element SHALL show the player's current video frame over its whole bounds, with the video's display aspect ratio kept and the rest filled black, as the `<video>` area of a Video.js player does. Before the first frame, and while no media is loaded, the element SHALL show black.

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
- **WHEN** a video plays in an element with controls on Windows with Windows video turned on
- **THEN** the control bar and the other Video.js overlays are drawn over the video, as on macOS

### Requirement: GPUI platform coverage

The video element SHALL be available on macOS, Windows and Linux, so application code using it builds on all three. On Windows it SHALL show video when the application has turned Windows video on, and black otherwise. On Linux it SHALL show black. Wherever it shows black, the player's audio and control SHALL keep working. These gaps SHALL be documented.

#### Scenario: macOS
- **WHEN** an application with a video element runs on macOS
- **THEN** the element shows the video

#### Scenario: Windows with Windows video
- **WHEN** the same application runs on Windows, with Windows video turned on, on a graphics driver that can share OpenGL textures with Direct3D 11
- **THEN** the element shows the video

#### Scenario: Windows and Linux
- **WHEN** the same application runs on Linux, or is built for Windows without turning Windows video on
- **THEN** it builds against the unmodified GPUI and runs, the element shows black, and the player's audio plays

## ADDED Requirements

### Requirement: Windows video setup

Windows video SHALL be opt-in. The application turns on a feature of the GPUI crate and uses a patched GPUI in place of GPUI-Kit's default backend crates. With the feature but without the patched GPUI, the build SHALL fail with an error that points at the missing patch. The setup SHALL be documented, including that GPUI-Kit's alternative backend is not covered.

#### Scenario: Feature with the patched GPUI
- **WHEN** an application turns the feature on and patches GPUI as documented
- **THEN** it builds, and the video element shows video on Windows

#### Scenario: Feature without the patched GPUI
- **WHEN** an application turns the feature on but does not patch GPUI
- **THEN** the build fails, and the first error names an item whose name and documentation say that the patched GPUI is required

#### Scenario: Other platforms
- **WHEN** an application turns the feature on and is built for macOS or Linux
- **THEN** the feature has no effect, and the element behaves as without it

### Requirement: Windows video unavailable at run time

On Windows with Windows video turned on, when the player's video cannot be shared with GPUI's renderer, the element SHALL show black, log one warning that names the reason, and keep the player's audio and controls working. This covers a graphics driver that cannot share OpenGL textures with Direct3D 11, OpenGL and GPUI on different graphics adapters, and a frame GPUI cannot open.

#### Scenario: Driver without sharing
- **WHEN** a video element is shown on Windows with Windows video turned on, and the graphics driver cannot share OpenGL textures with Direct3D 11
- **THEN** the element shows black, one warning saying the driver cannot share with Direct3D 11 is logged, and the player's audio and controls keep working

#### Scenario: Frame GPUI cannot open
- **WHEN** GPUI is given a frame whose texture it cannot open on its own device
- **THEN** that video is drawn black, the rest of the window draws normally, and a warning is logged once, not once per frame
