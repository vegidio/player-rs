## ADDED Requirements

### Requirement: Correct picture on software OpenGL

When a render context is created on Mesa's software OpenGL renderers (llvmpipe or softpipe), the core SHALL make mpv draw with bilinear scaling, unless the application set mpv's `scale` option itself, so that YUV video is drawn in its own colours rather than black. On other renderers, mpv's scaling options SHALL stay as mpv or the application set them.

#### Scenario: YUV video on llvmpipe
- **WHEN** a render context or a Linux surface renderer on llvmpipe draws YUV video, scaled up or down, with no scaler set by the application
- **THEN** every frame shows the video in its own colours, in every run

#### Scenario: Scaler chosen by the application
- **WHEN** the application builds the player with its own `scale` option and creates a render context on llvmpipe
- **THEN** mpv keeps the application's scaler
