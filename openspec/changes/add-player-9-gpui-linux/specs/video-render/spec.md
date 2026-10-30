## ADDED Requirements

### Requirement: Correct picture on software OpenGL

When a render context is created on Mesa's software OpenGL renderers (llvmpipe or softpipe), the core SHALL put mpv in its dumb rendering mode (`gpu-dumb-mode`), unless the application set that option or mpv's `scale` option itself, so that YUV video is drawn whole and in its own colours rather than black, discoloured or crossed by black lines. On other renderers, mpv's rendering options SHALL stay as mpv or the application set them.

#### Scenario: YUV video on llvmpipe
- **WHEN** a render context or a Linux surface renderer on llvmpipe draws YUV video, scaled up or down by any factor, with neither option set by the application
- **THEN** every frame shows the whole video in its own colours, in every run

#### Scenario: Options chosen by the application
- **WHEN** the application builds the player with its own `scale` or `gpu-dumb-mode` option and creates a render context on llvmpipe
- **THEN** mpv keeps the application's option, and dumb mode is not turned on for it
