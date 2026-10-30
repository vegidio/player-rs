//! DRM device nodes, the GBM device on one, and the ring's slots: single-plane RGBA buffers allocated by GBM, shared
//! as dma-bufs and drawn into by GL through an EGLImage.

use super::DrmDevice;
use super::egl::{Context, Display, Image, Plane};
use crate::error::{Error, Result};
use glow::HasContext;
use std::cmp::Reverse;
use std::ffi::{c_int, c_void};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

/// `DRM_FORMAT_ABGR8888` (and `GBM_FORMAT_ABGR8888`): R, G, B, A bytes in memory order.
pub(super) const ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
/// `DRM_FORMAT_MOD_LINEAR`.
pub(super) const LINEAR: u64 = 0;
/// `DRM_FORMAT_MOD_INVALID`: what GBM reports when a buffer has no explicit modifier.
const INVALID: u64 = 0x00ff_ffff_ffff_ffff;

const GBM_BO_USE_RENDERING: u32 = 1 << 2;
const GBM_BO_USE_LINEAR: u32 = 1 << 4;

unsafe extern "C" {
    fn gbm_create_device(fd: c_int) -> *mut c_void;
    fn gbm_device_destroy(gbm: *mut c_void);
    fn gbm_bo_create(gbm: *mut c_void, width: u32, height: u32, format: u32, flags: u32) -> *mut c_void;
    fn gbm_bo_create_with_modifiers2(
        gbm: *mut c_void,
        width: u32,
        height: u32,
        format: u32,
        modifiers: *const u64,
        count: u32,
        flags: u32,
    ) -> *mut c_void;
    fn gbm_bo_get_plane_count(bo: *mut c_void) -> c_int;
    fn gbm_bo_get_fd_for_plane(bo: *mut c_void, plane: c_int) -> c_int;
    fn gbm_bo_get_stride_for_plane(bo: *mut c_void, plane: c_int) -> u32;
    fn gbm_bo_get_offset(bo: *mut c_void, plane: c_int) -> u32;
    fn gbm_bo_get_modifier(bo: *mut c_void) -> u64;
    fn gbm_bo_destroy(bo: *mut c_void);
}

/// A DRM device node, such as `/dev/dri/renderD128`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) path: PathBuf,
    pub(super) device: DrmDevice,
}

/// The DRM nodes in `/dev/dri`: render nodes by device number, then primary (card) nodes by device number.
///
/// Card nodes come last because they are only needed where a software driver allocates through dumb buffers, which
/// the kernel refuses on render nodes, as Mesa's llvmpipe does on `vgem`.
pub(super) fn nodes() -> Vec<Node> {
    nodes_in(Path::new("/dev/dri"))
}

fn nodes_in(directory: &Path) -> Vec<Node> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return vec![];
    };
    let mut nodes: Vec<(bool, Node)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let render = name.starts_with("renderD");
            if !render && !name.starts_with("card") {
                return None;
            }
            let path = entry.path();
            let metadata = std::fs::metadata(&path).ok()?;
            metadata
                .file_type()
                .is_char_device()
                .then(|| (render, Node { path, device: split(metadata.rdev()) }))
        })
        .collect();
    nodes.sort_by_key(|(render, node)| (Reverse(*render), node.device.minor));
    nodes.into_iter().map(|(_, node)| node).collect()
}

/// glibc's `gnu_dev_major` and `gnu_dev_minor`.
fn split(rdev: u64) -> DrmDevice {
    DrmDevice {
        major: (((rdev >> 32) & 0xffff_f000) | ((rdev >> 8) & 0x0000_0fff)) as u32,
        minor: (((rdev >> 12) & 0xffff_ff00) | (rdev & 0x0000_00ff)) as u32,
    }
}

/// A GBM device on an open DRM node, destroyed when dropped, before the node is closed.
pub(super) struct Device {
    raw: NonNull<c_void>,
    node: File,
}

impl Device {
    pub(super) fn open(node: &Node) -> Result<Rc<Device>> {
        let path = node.path.display();
        // std opens with O_CLOEXEC.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node.path)
            .map_err(|error| Error::Render(format!("{path} cannot be opened: {error}")))?;
        // SAFETY: the fd is an open DRM node, which outlives the device (`Drop` destroys it first).
        let raw = unsafe { gbm_create_device(file.as_raw_fd()) };
        let raw = NonNull::new(raw).ok_or_else(|| Error::Render(format!("no GBM device on {path}")))?;
        Ok(Rc::new(Device { raw, node: file }))
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }

    /// The open node, for mpv's hardware decoders.
    pub(super) fn node_fd(&self) -> c_int {
        self.node.as_raw_fd()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: the device is valid, and every buffer and display on it is gone, since they hold it. The node is
        // closed after this, with the fields.
        unsafe { gbm_device_destroy(self.raw.as_ptr()) };
    }
}

/// A single-plane RGBA buffer and its dma-buf. The bo is destroyed when dropped; the dma-buf's memory lives on for
/// as long as any of its fds is open.
pub(super) struct Buffer {
    bo: NonNull<c_void>,
    pub(super) fd: Arc<OwnedFd>,
    pub(super) stride: u32,
    pub(super) offset: u32,
    pub(super) modifier: u64,
    _device: Rc<Device>,
}

impl Buffer {
    /// Allocates a `width` x `height` buffer with one of `modifiers`, in order of preference.
    pub(super) fn new(device: &Rc<Device>, width: u32, height: u32, modifiers: &[u64]) -> Result<Buffer> {
        let count = u32::try_from(modifiers.len()).map_err(|_| Error::Render("too many modifiers".into()))?;
        let unusable = |reason: String| {
            Error::Render(format!(
                "the graphics driver cannot allocate a single-plane {width}x{height} RGBA buffer with any of the \
                 modifiers {}: {reason}",
                list(modifiers)
            ))
        };
        // SAFETY: the device is valid; `modifiers` has `count` elements and outlives the call.
        let mut bo = unsafe {
            gbm_bo_create_with_modifiers2(
                device.as_ptr(),
                width,
                height,
                ABGR8888,
                modifiers.as_ptr(),
                count,
                GBM_BO_USE_RENDERING,
            )
        };
        let mut fallback = false;
        if bo.is_null() && modifiers.contains(&LINEAR) {
            // Drivers without modifier support allocate linear buffers through the older entry point.
            // SAFETY: as above.
            bo = unsafe {
                gbm_bo_create(device.as_ptr(), width, height, ABGR8888, GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR)
            };
            fallback = true;
        }
        let bo = NonNull::new(bo).ok_or_else(|| unusable("GBM refused the allocation".into()))?;
        // From here on the bo is destroyed by `Buffer`'s `Drop`, or below.
        let destroy = |reason: String| {
            // SAFETY: the bo is valid and not used afterwards.
            unsafe { gbm_bo_destroy(bo.as_ptr()) };
            unusable(reason)
        };

        // SAFETY: the bo is valid.
        let planes = unsafe { gbm_bo_get_plane_count(bo.as_ptr()) };
        if planes != 1 {
            return Err(destroy(format!("its layout has {planes} planes")));
        }
        // SAFETY: the bo is valid and has plane 0.
        let (raw_fd, stride, offset, modifier) = unsafe {
            (
                gbm_bo_get_fd_for_plane(bo.as_ptr(), 0),
                gbm_bo_get_stride_for_plane(bo.as_ptr(), 0),
                gbm_bo_get_offset(bo.as_ptr(), 0),
                gbm_bo_get_modifier(bo.as_ptr()),
            )
        };
        if raw_fd < 0 {
            return Err(destroy("its dma-buf cannot be exported".into()));
        }
        // SAFETY: GBM returns a new fd that the caller owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let modifier = if fallback && modifier == INVALID { LINEAR } else { modifier };
        if !modifiers.contains(&modifier) {
            drop(fd);
            return Err(destroy(format!("it was allocated with the modifier {modifier:#x}")));
        }
        Ok(Buffer { bo, fd: Arc::new(fd), stride, offset, modifier, _device: Rc::clone(device) })
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: the bo is valid and destroyed only here; its exported fds keep the memory.
        unsafe { gbm_bo_destroy(self.bo.as_ptr()) };
    }
}

/// Modifiers as a list for messages, such as `[0x0, 0x100000000000001]`.
pub(super) fn list(modifiers: &[u64]) -> String {
    let items: Vec<String> = modifiers.iter().map(|modifier| format!("{modifier:#x}")).collect();
    format!("[{}]", items.join(", "))
}

/// A buffer of the ring, imported into GL as the colour attachment of a framebuffer mpv draws into.
pub(super) struct Slot {
    pub(super) fbo: glow::NativeFramebuffer,
    texture: glow::NativeTexture,
    // Destroyed after the texture using it, then the bo.
    _image: Image,
    pub(super) buffer: Buffer,
}

impl Slot {
    /// Allocates a buffer on `device` and makes it drawable on `context`, which is current on this thread.
    pub(super) fn new(
        device: &Rc<Device>,
        display: &Rc<Display>,
        context: &Context,
        gl: &glow::Context,
        (width, height): (u32, u32),
        modifiers: &[u64],
    ) -> Result<Slot> {
        let buffer = Buffer::new(device, width, height, modifiers)?;
        let image = display.import(&Plane {
            fd: buffer.fd.as_fd(),
            width,
            height,
            fourcc: ABGR8888,
            stride: buffer.stride,
            offset: buffer.offset,
            modifier: buffer.modifier,
        })?;
        // SAFETY: `context` is current on this thread, and `gl` was loaded for it; objects created here are deleted
        // again on failure.
        unsafe {
            let texture = gl.create_texture().map_err(Error::Render)?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            image.bind_texture(context);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
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
            let slot = Slot { fbo, texture, _image: image, buffer };
            if status != glow::FRAMEBUFFER_COMPLETE {
                slot.delete(gl);
                return Err(Error::Render(format!(
                    "the dma-buf cannot be drawn into: its framebuffer is incomplete (status {status:#x})"
                )));
            }
            Ok(slot)
        }
    }

    /// Deletes the GL objects, then destroys the image and the bo. Frames holding the fd keep the memory.
    pub(super) fn delete(self, gl: &glow::Context) {
        // SAFETY: the objects were created on `gl`'s context, which is current on this thread.
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[test]
    fn device_numbers_split_like_glibc() {
        assert_eq!(split(0xe280), DrmDevice { major: 226, minor: 128 });
        assert_eq!(split(0xe200), DrmDevice { major: 226, minor: 0 });
        // Large numbers use the high bits, as glibc's `makedev` puts them.
        let (major, minor) = (0x0001_2345_u64, 0x0006_789a_u64);
        let rdev = ((major & 0xfff) << 8) | ((major & !0xfff) << 32) | (minor & 0xff) | ((minor & !0xff) << 12);
        assert_eq!(split(rdev), DrmDevice { major: 0x0001_2345, minor: 0x0006_789a });
    }

    #[test]
    fn no_nodes_without_a_directory() {
        assert!(nodes_in(Path::new("/nonexistent/dri")).is_empty());
    }

    #[test]
    fn render_nodes_come_before_card_nodes() {
        let nodes = nodes();
        let render = nodes.iter().take_while(|node| node.path.to_string_lossy().contains("renderD")).count();
        assert!(nodes[render..].iter().all(|node| node.path.to_string_lossy().contains("card")), "{nodes:?}");
        for group in [&nodes[..render], &nodes[render..]] {
            assert!(group.is_sorted_by_key(|node| node.device.minor), "{nodes:?}");
        }
    }

    #[test]
    fn modifiers_are_listed_in_hex() {
        assert_eq!(list(&[LINEAR, 0x0100_0000_0000_0001]), "[0x0, 0x100000000000001]");
    }
}
