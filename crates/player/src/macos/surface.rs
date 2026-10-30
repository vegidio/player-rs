//! IOSurface-backed `CVPixelBuffer`s, and the GL framebuffers that draw into each of their planes.

use super::SurfaceFormat;
use super::cgl::Context;
use crate::error::{Error, Result};
use glow::HasContext;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_core_video::{
    CVAttachmentMode, CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetIOSurface,
    kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrixKey, kCVPixelBufferIOSurfacePropertiesKey,
    kCVPixelBufferOpenGLCompatibilityKey, kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_io_surface::IOSurfaceRef;
use std::ptr::{self, NonNull};

impl SurfaceFormat {
    /// The size of this format's surfaces for a requested size: NV12 needs even dimensions, so odd ones grow by one.
    pub(super) fn surface_size(self, width: u32, height: u32) -> (u32, u32) {
        match self {
            Self::Bgra => (width, height),
            Self::Nv12 => (width.saturating_add(width & 1), height.saturating_add(height & 1)),
        }
    }
}

/// A texture bound to one plane of a surface, attached to its own framebuffer.
pub(super) struct Plane {
    pub(super) fbo: glow::NativeFramebuffer,
    texture: glow::NativeTexture,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// A pixel buffer and the framebuffers drawing into its planes: one for BGRA; Y then CbCr for NV12.
pub(super) struct Surface {
    pub(super) buffer: CFRetained<CVPixelBuffer>,
    pub(super) planes: Vec<Plane>,
}

impl Surface {
    /// Creates a `width` x `height` surface (see [`SurfaceFormat::surface_size`]) and its framebuffers on `context`,
    /// which `gl` calls into.
    pub(super) fn new(
        context: &Context,
        gl: &glow::Context,
        format: SurfaceFormat,
        width: u32,
        height: u32,
    ) -> Result<Surface> {
        let buffer = pixel_buffer(format, width, height)?;
        let surface = CVPixelBufferGetIOSurface(Some(&buffer))
            .ok_or_else(|| Error::Render("the pixel buffer has no IOSurface".into()))?;
        // (plane, size, internal format, format, type), as GL sees each plane of the IOSurface.
        let layout = match format {
            SurfaceFormat::Bgra => {
                vec![(0, (width, height), (glow::RGBA8, glow::BGRA, glow::UNSIGNED_INT_8_8_8_8_REV))]
            }
            SurfaceFormat::Nv12 => vec![
                (0, (width, height), (glow::R8, glow::RED, glow::UNSIGNED_BYTE)),
                (1, (width / 2, height / 2), (glow::RG8, glow::RG, glow::UNSIGNED_BYTE)),
            ],
        };

        let mut planes = Vec::with_capacity(layout.len());
        for (index, size, formats) in layout {
            // SAFETY: `context` is current on this thread, as `gl`'s functions require, and the plane's size and
            // formats match the pixel buffer's layout.
            match unsafe { plane(context, gl, &surface, index, size, formats) } {
                Ok(plane) => planes.push(plane),
                Err(error) => {
                    // SAFETY: as above.
                    unsafe { delete_planes(gl, planes) };
                    return Err(error);
                }
            }
        }
        Ok(Surface { buffer, planes })
    }

    /// Deletes the framebuffers and textures. The pixel buffer stays alive while any [`Frame`](super::Frame) holds
    /// it.
    pub(super) fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on `gl`'s context, which is current on this thread.
        unsafe { delete_planes(gl, self.planes) };
    }
}

/// Binds `plane` of `surface` to a new rectangle texture and attaches it to a new, complete framebuffer.
///
/// # Safety
///
/// `context` is current on this thread, `gl` calls into it, and the size and formats describe the plane.
unsafe fn plane(
    context: &Context,
    gl: &glow::Context,
    surface: &IOSurfaceRef,
    plane: u32,
    (width, height): (u32, u32),
    formats: (u32, u32, u32),
) -> Result<Plane> {
    // SAFETY: as the caller guarantees; every object is created and bound here, and deleted again on failure.
    unsafe {
        let texture = gl.create_texture().map_err(Error::Render)?;
        gl.bind_texture(glow::TEXTURE_RECTANGLE, Some(texture));
        let bound = context.bind_io_surface(surface, plane, (width, height), formats);
        gl.bind_texture(glow::TEXTURE_RECTANGLE, None);
        match bound.and_then(|()| crate::gl::framebuffer(gl, glow::TEXTURE_RECTANGLE, texture, "surface")) {
            Ok(fbo) => Ok(Plane { fbo, texture, width, height }),
            Err(error) => {
                gl.delete_texture(texture);
                Err(error)
            }
        }
    }
}

/// # Safety
///
/// The objects belong to `gl`'s context, which is current on this thread.
unsafe fn delete_plane(gl: &glow::Context, plane: Plane) {
    // SAFETY: as the caller guarantees.
    unsafe {
        gl.delete_framebuffer(plane.fbo);
        gl.delete_texture(plane.texture);
    }
}

/// # Safety
///
/// As [`delete_plane`].
unsafe fn delete_planes(gl: &glow::Context, planes: Vec<Plane>) {
    for plane in planes {
        // SAFETY: as the caller guarantees.
        unsafe { delete_plane(gl, plane) };
    }
}

/// Creates an IOSurface-backed pixel buffer GL can draw into. NV12 buffers are tagged with the full-range BT.601
/// matrix their contents are encoded with.
fn pixel_buffer(format: SurfaceFormat, width: u32, height: u32) -> Result<CFRetained<CVPixelBuffer>> {
    let pixel_format = match format {
        SurfaceFormat::Bgra => kCVPixelFormatType_32BGRA,
        SurfaceFormat::Nv12 => kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    };
    let io_surface_properties = CFDictionary::<CFString, CFType>::empty();
    // SAFETY: CoreVideo's constant keys are valid for the life of the process.
    let keys = unsafe { [kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferOpenGLCompatibilityKey] };
    let values: [&CFType; 2] = [&io_surface_properties, CFBoolean::new(true)];
    let attributes = CFDictionary::from_slices(&keys, &values);

    let mut buffer = ptr::null_mut();
    // SAFETY: the attributes map CoreVideo's keys to values of the documented types; the out-pointer is valid.
    let code = unsafe {
        CVPixelBufferCreate(
            None,
            width as usize,
            height as usize,
            pixel_format,
            Some(attributes.as_opaque()),
            NonNull::from(&mut buffer),
        )
    };
    let buffer = NonNull::new(buffer).filter(|_| code == 0).ok_or_else(|| {
        Error::Render(format!("CVPixelBufferCreate failed for a {width}x{height} {format:?} surface (error {code})"))
    })?;
    // SAFETY: CVPixelBufferCreate returned a new buffer with a +1 retain count, which this value now owns.
    let buffer = unsafe { CFRetained::from_raw(buffer) };

    if format == SurfaceFormat::Nv12 {
        // SAFETY: the key and value are CoreVideo's constant strings, and the matrix key takes one of the matrix
        // constants.
        unsafe {
            buffer.set_attachment(
                kCVImageBufferYCbCrMatrixKey,
                kCVImageBufferYCbCrMatrix_ITU_R_601_4,
                CVAttachmentMode::ShouldPropagate,
            );
        }
    }
    Ok(buffer)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::macos::GlRenderer;
    use crate::test_gl;
    use objc2_core_video::{
        CVBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
        CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidthOfPlane, CVPixelBufferLockBaseAddress,
        CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };

    /// The GL renderer for tests: software, as on CI, unless `PLAYER_TEST_GL=hardware`.
    pub(in crate::macos) fn renderer() -> GlRenderer {
        if std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware") {
            GlRenderer::Hardware
        } else {
            GlRenderer::Software
        }
    }

    /// A context current on this thread, with glow calling into it.
    pub(in crate::macos) fn gl() -> (Context, glow::Context) {
        let context = Context::new(renderer()).expect("a CGL context is created");
        let loader = crate::macos::cgl::Loader::new().unwrap();
        // SAFETY: `context` is current on this thread, and the loader resolves its functions.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| loader.get(name)) };
        (context, gl)
    }

    /// Reads a plane's rows of `bytes_per_pixel`-byte pixels, top row (row 0 of the buffer's memory) first.
    pub(in crate::macos) fn read_plane(buffer: &CVPixelBuffer, plane: usize, bytes_per_pixel: usize) -> Vec<Vec<u8>> {
        // SAFETY: the buffer is locked for reading around the accesses, which stay within the plane's rows.
        unsafe {
            assert_eq!(CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly), 0);
            let base = CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast::<u8>();
            let stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, plane);
            let (width, height) =
                (CVPixelBufferGetWidthOfPlane(buffer, plane), CVPixelBufferGetHeightOfPlane(buffer, plane));
            let bytes = width * bytes_per_pixel;
            let rows = (0..height)
                .map(|row| std::slice::from_raw_parts(base.add(row * stride), bytes).to_vec())
                .collect();
            CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
            rows
        }
    }

    fn clear(gl: &glow::Context, plane: &Plane, [r, g, b, a]: [f32; 4]) {
        // SAFETY: the framebuffer belongs to `gl`'s context, current on this thread.
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(plane.fbo));
            gl.viewport(0, 0, plane.width as i32, plane.height as i32);
            gl.clear_color(r, g, b, a);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.finish();
        }
    }

    fn assert_rows(rows: &[Vec<u8>], pixel: &[u8], what: &str) {
        for (y, row) in rows.iter().enumerate() {
            for (x, actual) in row.chunks_exact(pixel.len()).enumerate() {
                let close = actual.iter().zip(pixel).all(|(a, e)| a.abs_diff(*e) <= 1);
                assert!(close, "{what}: pixel ({x}, {y}) is {actual:?}, expected {pixel:?}");
            }
        }
    }

    #[test]
    fn nv12_rounds_odd_sizes_up() {
        assert_eq!(SurfaceFormat::Nv12.surface_size(321, 181), (322, 182));
        assert_eq!(SurfaceFormat::Nv12.surface_size(320, 180), (320, 180));
        assert_eq!(SurfaceFormat::Bgra.surface_size(321, 181), (321, 181));
    }

    #[test]
    fn bgra_surface_reads_back_a_cleared_colour() {
        if test_gl::skipped() {
            return;
        }
        let (context, gl) = gl();
        let surface = Surface::new(&context, &gl, SurfaceFormat::Bgra, 64, 48).unwrap();
        assert_eq!(surface.planes.len(), 1);
        clear(&gl, &surface.planes[0], [1.0, 0.5, 0.0, 1.0]);

        let rows = read_plane(&surface.buffer, 0, 4);
        assert_eq!((rows.len(), rows[0].len()), (48, 64 * 4));
        assert_rows(&rows, &[0, 128, 255, 255], "BGRA");
        surface.delete(&gl);
    }

    #[test]
    fn nv12_surface_reads_back_a_cleared_colour_per_plane() {
        if test_gl::skipped() {
            return;
        }
        let (context, gl) = gl();
        let (width, height) = SurfaceFormat::Nv12.surface_size(65, 49);
        let surface = Surface::new(&context, &gl, SurfaceFormat::Nv12, width, height).unwrap();
        let sizes: Vec<_> = surface.planes.iter().map(|plane| (plane.width, plane.height)).collect();
        assert_eq!(sizes, [(66, 50), (33, 25)]);
        clear(&gl, &surface.planes[0], [0.25, 0.0, 0.0, 1.0]);
        clear(&gl, &surface.planes[1], [0.5, 0.75, 0.0, 1.0]);

        let luma = read_plane(&surface.buffer, 0, 1);
        assert_eq!((luma.len(), luma[0].len()), (50, 66));
        assert_rows(&luma, &[64], "Y");
        let chroma = read_plane(&surface.buffer, 1, 2);
        assert_eq!((chroma.len(), chroma[0].len()), (25, 33 * 2));
        assert_rows(&chroma, &[128, 191], "CbCr");
        surface.delete(&gl);
    }

    #[test]
    fn nv12_surface_is_tagged_with_its_matrix() {
        if test_gl::skipped() {
            return;
        }
        let buffer = pixel_buffer(SurfaceFormat::Nv12, 16, 16).unwrap();
        let buffer: &CVBuffer = &buffer;
        // SAFETY: the key is CoreVideo's constant string; the mode out-pointer may be null.
        let matrix = unsafe { buffer.attachment(kCVImageBufferYCbCrMatrixKey, ptr::null_mut()) }.unwrap();
        // SAFETY: as above.
        let expected: &CFType = unsafe { kCVImageBufferYCbCrMatrix_ITU_R_601_4 };
        assert_eq!(&*matrix, expected);
    }
}
