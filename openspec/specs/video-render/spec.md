## Purpose

Lets an application draw a player's video into an OpenGL framebuffer it owns, with all decoding, scaling, colour conversion and subtitle rendering done by mpv on the GPU, so that every frontend shows video through the same toolkit-agnostic path without copying frames through the CPU.

## Requirements

### Requirement: Render context creation

The core crate SHALL let an application create a render context for a player built with the render video output, given a function that resolves OpenGL function names for a GL context that is current on the calling thread, and a frame-ready notification. Creation SHALL fail with a descriptive render error, and leave the player usable, when the player was not built with the render video output, when a render context already exists for that player, or when mpv cannot initialise rendering with the given GL context.

#### Scenario: Create for a render player
- **WHEN** a render context is created for a player built with the render video output, with a valid current GL context
- **THEN** creation succeeds

#### Scenario: Player without render output
- **WHEN** a render context is created for a player built with no video output
- **THEN** creation fails with a render error saying the player was not built for rendering, and the player keeps working

#### Scenario: Second render context
- **WHEN** a render context is created for a player that already has a live render context
- **THEN** creation fails with a render error, and the existing render context keeps working

#### Scenario: Recreate after drop
- **WHEN** a player's render context is dropped and a new one is created for the same player
- **THEN** creation succeeds

### Requirement: Frame-ready notification

A render context SHALL notify the application, from any thread, whenever a new video frame is ready or the picture must be redrawn. After a notification, the application SHALL be able to ask the render context, on its render thread, whether a new frame is ready to be drawn. No notification SHALL be delivered after the render context has been dropped.

#### Scenario: Notification while playing
- **WHEN** a player with a render context plays a video
- **THEN** the frame-ready notification is delivered repeatedly while the video plays, and asking after a notification reports a new frame at least once

#### Scenario: No notification after drop
- **WHEN** a render context is dropped while its player keeps playing
- **THEN** its frame-ready notification is not delivered again

### Requirement: Render into a host framebuffer

A render context SHALL draw the current video frame into an OpenGL framebuffer the application names, at the width and height the application gives, scaled to fit with the video's display aspect ratio kept and the rest filled black, and optionally flipped vertically. Drawing SHALL happen entirely on the GPU, into the application's framebuffer, without copying frames through CPU memory. Drawing with a width or height of zero SHALL fail with a render error and draw nothing.

#### Scenario: Frame drawn into an offscreen framebuffer
- **WHEN** a player with a render context plays mpv's generated colour test pattern and the application draws a ready frame into a 160x120 offscreen framebuffer
- **THEN** the framebuffer contains the test pattern, not a blank or single-colour image

#### Scenario: Aspect ratio kept
- **WHEN** a 160x120 video is drawn into a 320x120 framebuffer
- **THEN** the video fills the middle 160x120 area and the bars to its left and right are black

#### Scenario: Vertical flip
- **WHEN** the same frame is drawn once without and once with the vertical flip
- **THEN** the second image is the first mirrored top to bottom

#### Scenario: Zero-sized target
- **WHEN** drawing is requested with a width or height of zero
- **THEN** a render error is returned and the framebuffer is not touched

#### Scenario: Nothing loaded
- **WHEN** drawing is requested while the player has no media loaded
- **THEN** no error is returned and the framebuffer is filled black

### Requirement: Frame timing feedback

A render context SHALL let the application report that a drawn frame was presented on screen, so mpv can time later frames. Not reporting SHALL not prevent playback.

#### Scenario: Playback without reports
- **WHEN** an application draws frames but never reports presentation
- **THEN** playback still advances and frames keep being drawn

### Requirement: Render context lifetime

A render context SHALL keep the player's mpv instance alive for as long as it exists, even after the last player handle is dropped, and SHALL release the instance on drop when it is the last owner. Dropping a render context while media plays SHALL stop video output without stopping playback, and the player SHALL keep responding to control and reporting state. Media loaded while the player has no render context SHALL play without video until it is loaded again after a render context exists.

#### Scenario: Drop render context while playing
- **WHEN** a render context is dropped while its player is playing a video
- **THEN** the drop returns within one second, playback continues, and the player can still be paused and queried

#### Scenario: Drop player before render context
- **WHEN** the last player handle is dropped while its render context is alive, and the render context is dropped afterwards
- **THEN** neither drop hangs, drawing with the render context between the two drops returns no error, and the mpv instance is released after the second drop

#### Scenario: Load before creating a render context
- **WHEN** a player built with the render video output loads a video before any render context exists
- **THEN** loading succeeds without error, audio plays, and no video is drawn for that media

### Requirement: Render support on all platforms

Render context creation, frame-ready notification, drawing and the lifetime rules SHALL behave identically on macOS, Windows and Linux with any OpenGL context of version 2.1 or later (or OpenGL ES 2.0 or later) that the application provides, including software-rendered contexts with no GPU.

#### Scenario: Software OpenGL on CI
- **WHEN** the render tests run on any supported platform using a software-rendered offscreen OpenGL context
- **THEN** the context is created, frames are notified and drawn, and the drawn pattern matches the scenarios above

#### Scenario: macOS OpenGL
- **WHEN** the render tests run on macOS, where OpenGL is deprecated but present
- **THEN** they pass with the system OpenGL framework and no extra installation
