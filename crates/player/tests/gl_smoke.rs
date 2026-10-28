//! Proves the render tests' GL setup works on this machine: a headless context, an offscreen framebuffer, and pixel
//! readback. When this fails, the render tests fail for the same reason, not because of the player.

mod common {
    pub mod gl;
}

use common::gl::{self, Gl};

#[test]
fn clears_and_reads_back_a_framebuffer() {
    if gl::skipped() {
        return;
    }
    let gl = Gl::new();
    eprintln!("GL renderer: {}", gl.renderer());

    let framebuffer = gl.framebuffer(64, 32, [0, 0, 0, 255]);
    gl.fill(&framebuffer, [255, 0, 255, 255]);
    let image = gl.read(&framebuffer);
    assert!(
        image.pixels.as_chunks::<4>().0.iter().all(|pixel| *pixel == [255, 0, 255, 255]),
        "the framebuffer is magenta, first pixel: {:?}",
        image.pixel(0, 0)
    );
    gl.delete(framebuffer);
}

#[test]
fn loader_resolves_gl_functions() {
    if gl::skipped() {
        return;
    }
    let gl = Gl::new();
    let mut loader = gl.loader();
    assert!(!loader(c"glGetString").is_null());
    assert!(!loader(c"glBindFramebuffer").is_null());
}
