//! The GPU pass that encodes mpv's RGBA picture as NV12: full-range BT.601 YCbCr, chroma at half size.
//!
//! The encode is the exact inverse of the decode GPUI's Metal renderer applies (`R = Y + 1.402(Cr−½)`,
//! `G = Y − 0.3441(Cb−½) − 0.7141(Cr−½)`, `B = Y + 1.772(Cb−½)`), done on gamma-encoded RGB as video YCbCr is.

use super::surface::Surface;
use crate::error::{Error, Result};
use glow::HasContext;

/// A full-screen triangle with no vertex buffer.
const VERTEX: &str = r"#version 150
void main() {
    vec2 corner = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
}
";

/// Y, one texel per pixel; rows map 1:1, so the picture keeps its orientation.
const LUMA: &str = r"#version 150
uniform sampler2D source;
out vec4 colour;
void main() {
    vec3 rgb = texelFetch(source, ivec2(gl_FragCoord.xy), 0).rgb;
    colour = vec4(dot(rgb, vec3(0.299, 0.587, 0.114)), 0.0, 0.0, 1.0);
}
";

/// CbCr at half size: sampling linearly at the centre of each 2x2 block averages exactly its four texels.
const CHROMA: &str = r"#version 150
uniform sampler2D source;
out vec4 colour;
void main() {
    vec3 rgb = texture(source, gl_FragCoord.xy * 2.0 / vec2(textureSize(source, 0))).rgb;
    float y = dot(rgb, vec3(0.299, 0.587, 0.114));
    colour = vec4((rgb.b - y) / 1.772 + 0.5, (rgb.r - y) / 1.402 + 0.5, 0.0, 1.0);
}
";

/// The RGBA texture mpv draws into before the conversion, sampled linearly.
pub(super) struct Scratch {
    pub(super) fbo: glow::NativeFramebuffer,
    texture: glow::NativeTexture,
}

impl Scratch {
    pub(super) fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Scratch> {
        let size = |value: u32| i32::try_from(value).map_err(|_| Error::Render(format!("surface size {value}")));
        let (w, h) = (size(width)?, size(height)?);
        // SAFETY: `gl`'s context is current on this thread (see the module docs); every object is created and bound
        // here, and deleted again on failure.
        unsafe {
            let texture = gl.create_texture().map_err(Error::Render)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            for (parameter, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, value as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);

            let fbo = match gl.create_framebuffer() {
                Ok(fbo) => fbo,
                Err(error) => {
                    gl.delete_texture(texture);
                    return Err(Error::Render(error));
                }
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            let scratch = Scratch { fbo, texture };
            if status != glow::FRAMEBUFFER_COMPLETE {
                scratch.delete(gl);
                return Err(Error::Render(format!("the scratch framebuffer is incomplete (status {status:#x})")));
            }
            Ok(scratch)
        }
    }

    pub(super) fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on `gl`'s context, which is current on this thread.
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }
}

/// The compiled Y and CbCr passes.
pub(super) struct Converter {
    luma: glow::NativeProgram,
    chroma: glow::NativeProgram,
    /// Core profiles draw only with a vertex array bound, even an empty one.
    vertex_array: glow::NativeVertexArray,
}

impl Converter {
    pub(super) fn new(gl: &glow::Context) -> Result<Converter> {
        let luma = program(gl, LUMA)?;
        let chroma = program(gl, CHROMA).inspect_err(|_| {
            // SAFETY: the program was created on `gl`'s context, which is current on this thread.
            unsafe { gl.delete_program(luma) };
        })?;
        // SAFETY: as above.
        let vertex_array = unsafe { gl.create_vertex_array() }.map_err(Error::Render).inspect_err(|_| {
            // SAFETY: as above.
            unsafe {
                gl.delete_program(luma);
                gl.delete_program(chroma);
            }
        })?;
        Ok(Converter { luma, chroma, vertex_array })
    }

    /// Encodes `scratch` into `target`, an NV12 surface of the same size.
    pub(super) fn convert(&self, gl: &glow::Context, scratch: &Scratch, target: &Surface) {
        let [luma, chroma] = &target.planes[..] else { unreachable!("an NV12 surface has two planes") };
        // SAFETY: every object belongs to `gl`'s context, which is current on this thread. The state mpv may have
        // left that would affect the draws is reset first.
        unsafe {
            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::STENCIL_TEST);
            gl.disable(glow::CULL_FACE);
            gl.color_mask(true, true, true, true);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(scratch.texture));
            gl.bind_vertex_array(Some(self.vertex_array));

            for (program, plane) in [(self.luma, luma), (self.chroma, chroma)] {
                gl.use_program(Some(program));
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(plane.fbo));
                gl.viewport(0, 0, plane.width as i32, plane.height as i32);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.use_program(None);
            gl.bind_vertex_array(None);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    pub(super) fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on `gl`'s context, which is current on this thread.
        unsafe {
            gl.delete_program(self.luma);
            gl.delete_program(self.chroma);
            gl.delete_vertex_array(self.vertex_array);
        }
    }
}

/// Compiles and links the full-screen vertex shader with `fragment`. The sampler stays on texture unit 0.
fn program(gl: &glow::Context, fragment: &str) -> Result<glow::NativeProgram> {
    // SAFETY: `gl`'s context is current on this thread; the shaders are deleted once linked or on failure.
    unsafe {
        let program = gl.create_program().map_err(Error::Render)?;
        let mut shaders = vec![];
        let mut result = Ok(());
        for (kind, source) in [(glow::VERTEX_SHADER, VERTEX), (glow::FRAGMENT_SHADER, fragment)] {
            let shader = match gl.create_shader(kind) {
                Ok(shader) => shader,
                Err(error) => {
                    result = Err(error);
                    break;
                }
            };
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            gl.attach_shader(program, shader);
            shaders.push(shader);
            if !gl.get_shader_compile_status(shader) {
                result = Err(format!("shader compilation failed: {}", gl.get_shader_info_log(shader)));
                break;
            }
        }
        if result.is_ok() {
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                result = Err(format!("shader linking failed: {}", gl.get_program_info_log(program)));
            }
        }
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        match result {
            Ok(()) => Ok(program),
            Err(message) => {
                gl.delete_program(program);
                Err(Error::Render(format!("the NV12 conversion: {message}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::SurfaceFormat;
    use crate::macos::surface::tests::{gl, read_plane};
    use crate::test_gl;

    /// GPUI's decode (see the module docs), from 8-bit Y, Cb and Cr to RGB in `0.0..=1.0`.
    fn decode(y: u8, cb: u8, cr: u8) -> [f64; 3] {
        let (y, cb, cr) = (f64::from(y) / 255.0, f64::from(cb) / 255.0 - 0.5, f64::from(cr) / 255.0 - 0.5);
        [y + 1.402 * cr, y - 0.3441 * cb - 0.7141 * cr, y + 1.772 * cb]
    }

    /// Converts a 16x8 texture of eight solid 4x4 fields, red, green, blue and white in rows 0-3, then grey, black,
    /// yellow and cyan in rows 4-7, and checks every pixel decodes back to its field's colour within 3% per channel,
    /// in the same row.
    #[test]
    fn converts_solid_colours() {
        if test_gl::skipped() {
            return;
        }
        let (context, gl) = gl();
        const FIELDS: [[[u8; 3]; 4]; 2] = [
            [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]],
            [[128, 128, 128], [0, 0, 0], [255, 255, 0], [0, 255, 255]],
        ];
        let field = |x: usize, y: usize| FIELDS[y / 4][x / 4];
        let (width, height) = (16, 8);
        let pixels: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    let [r, g, b] = field(x, y);
                    [r, g, b, 255]
                })
            })
            .collect();

        let scratch = Scratch::new(&gl, width as u32, height as u32).unwrap();
        // SAFETY: the texture belongs to `gl`'s context, current on this thread; `pixels` holds the whole image.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(scratch.texture));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&pixels)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        let surface = Surface::new(&context, &gl, SurfaceFormat::Nv12, width as u32, height as u32).unwrap();
        let converter = Converter::new(&gl).unwrap();
        converter.convert(&gl, &scratch, &surface);
        // SAFETY: the context is current on this thread.
        unsafe { gl.finish() };

        let (luma, chroma) = (read_plane(&surface.buffer, 0, 1), read_plane(&surface.buffer, 1, 2));
        for y in 0..height {
            for x in 0..width {
                let (cb, cr) = (chroma[y / 2][x / 2 * 2], chroma[y / 2][x / 2 * 2 + 1]);
                let decoded = decode(luma[y][x], cb, cr);
                let expected = field(x, y).map(|channel| f64::from(channel) / 255.0);
                let close = decoded.iter().zip(expected).all(|(d, e)| (d.clamp(0.0, 1.0) - e).abs() <= 0.03);
                assert!(close, "pixel ({x}, {y}) decodes to {decoded:?}, expected {expected:?}");
            }
        }

        converter.delete(&gl);
        surface.delete(&gl);
        scratch.delete(&gl);
    }
}
