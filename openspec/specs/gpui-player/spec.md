## Purpose

Lets a GPUI application show a player's video inside its window as an ordinary element that it can lay out and style, with the video kept upright, letterboxed on black and refreshed as frames arrive, as a Video.js player shows its video element.

## Requirements

### Requirement: Video element

The GPUI crate SHALL provide a video element that an application creates from an element id and a player built with the render video output, and that it can size and position like any other GPUI element. On macOS, the element SHALL show the player's current video frame over its whole bounds, with the video's display aspect ratio kept and the rest filled black, as the `<video>` area of a Video.js player does. Before the first frame, and while no media is loaded, the element SHALL show black.

#### Scenario: Video shown
- **WHEN** a window contains a video element for a player that plays a video
- **THEN** the element shows the video, upright and in its own colours

#### Scenario: Letterboxed in a wider element
- **WHEN** a 4:3 video plays in a 16:9 element
- **THEN** the video fills the element's height, centred, with black bars on the left and right

#### Scenario: No media
- **WHEN** a video element's player has no media loaded
- **THEN** the element shows black

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

The video element SHALL be available on macOS, Windows and Linux, so application code using it builds on all three. On Windows and Linux it SHALL show black, while the player's audio and control keep working, until those platforms get video surfaces. This gap SHALL be documented.

#### Scenario: macOS
- **WHEN** an application with a video element runs on macOS
- **THEN** the element shows the video

#### Scenario: Windows and Linux
- **WHEN** the same application runs on Windows or Linux
- **THEN** it builds and runs, the element shows black, and the player's audio plays
