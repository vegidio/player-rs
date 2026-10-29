//! macOS: NV12 IOSurfaces, painted as CoreVideo pixel buffers.

use gpui_kit::*;
use player::Player;
use player::macos::{GlRenderer, SurfaceFormat};

pub(crate) use player::macos::{Frame, SurfaceRenderer};

pub(super) fn build(player: &Player, on_frame: impl Fn() + Send + Sync + 'static) -> player::Result<SurfaceRenderer> {
    SurfaceRenderer::builder(player)
        .format(SurfaceFormat::Nv12)
        .gl_renderer(gl_renderer())
        .on_frame(on_frame)
        .build()
}

pub(super) fn paint(window: &mut Window, bounds: Bounds<Pixels>, frame: &Frame) {
    window.paint_surface(bounds, frame.pixel_buffer().clone());
}

/// The OpenGL renderer: the GPU where there is one. The tests follow the crate's `PLAYER_TEST_GL` convention instead:
/// Apple's software renderer, as on CI, unless `PLAYER_TEST_GL=hardware`.
fn gl_renderer() -> GlRenderer {
    if !cfg!(test) {
        GlRenderer::Auto
    } else if std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware") {
        GlRenderer::Hardware
    } else {
        GlRenderer::Software
    }
}
