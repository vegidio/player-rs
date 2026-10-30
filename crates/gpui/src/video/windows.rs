//! Windows: BGRA Direct3D 11 textures shared through NT handles, which the patched GPUI opens on its own device.

use gpui_kit::*;
use player::Player;
use std::os::windows::io::BorrowedHandle;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use player::windows::{Frame, SurfaceRenderer};

pub(super) fn build(player: &Player, on_frame: impl Fn() + Send + Sync + 'static) -> player::Result<SurfaceRenderer> {
    // The default adapter is the one GPUI renders with.
    SurfaceRenderer::builder(player).on_frame(on_frame).build()
}

pub(super) fn paint(window: &mut Window, bounds: Bounds<Pixels>, frame: &Frame) {
    paint_handle(window, bounds, frame.shared_handle());
}

/// Paints the texture behind `handle`, which GPUI keeps open through a duplicate for as long as it draws it.
pub(super) fn paint_handle(window: &mut Window, bounds: Bounds<Pixels>, handle: BorrowedHandle<'_>) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    match SharedTexture::duplicate(handle) {
        Ok(texture) => window.paint_surface(bounds, texture),
        // Only when the process runs out of handles, or the like: the frame stays black.
        Err(error) if !WARNED.swap(true, Ordering::Relaxed) => {
            log::warn!("player_gpui: a video frame cannot be shown: {error}");
        }
        Err(_) => {}
    }
}

/// Pixels drawn by GPUI's Direct3D renderer offscreen, through the patched GPUI's headless renderer.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::fit;
    use crate::video::pixels::{assert_quadrants_letterboxed, assert_video_drawn, open, screenshot};
    use crate::video::tests::cannot_show_video;
    use ::windows::Win32::Foundation::{GENERIC_ALL, HMODULE};
    use ::windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
    };
    use ::windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_QUERY_DESC, D3D11_QUERY_EVENT,
        D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    };
    use ::windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use ::windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIFactory1, IDXGIResource1,
    };
    use ::windows::core::{BOOL, Interface, PCWSTR};
    use gpui_windows::DirectXHeadlessRenderer;
    use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
    // The glob above brings in GPUI's `test` macro; these are plain tests.
    use std::prelude::v1::test;

    /// A headless app whose windows GPUI's Direct3D renderer draws, or `None`, with a message, where it cannot start.
    fn headless() -> Option<HeadlessAppContext> {
        if let Err(error) = DirectXHeadlessRenderer::new() {
            eprintln!("skipped: GPUI's Direct3D renderer cannot start: {error:#}");
            return None;
        }
        let renderer = || Ok(Some(Box::new(DirectXHeadlessRenderer::new()?) as Box<dyn PlatformHeadlessRenderer>));
        Some(HeadlessAppContext::with_platform(
            std::sync::Arc::new(NoopTextSystem),
            std::sync::Arc::new(()),
            renderer,
        ))
    }

    /// A 64x48 BGRA texture whose quadrants are, from the top left, red, green, blue and yellow: made on a device of
    /// its own on the adapter GPUI picks, as the core's renderer does, and finished on the GPU before it is returned.
    /// The texture lives as long as the handle.
    fn quadrant_texture() -> OwnedHandle {
        const QUADRANTS: [[u8; 4]; 4] = [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255], [0, 255, 255, 255]];
        let (width, height) = (64usize, 48usize);
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|index| {
                let (x, y) = (index % width, index / width);
                QUADRANTS[usize::from(y >= height / 2) * 2 + usize::from(x >= width / 2)]
            })
            .collect();
        let device = gpui_device();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width as u32,
            Height: height as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32,
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: (width * 4) as u32,
            SysMemSlicePitch: 0,
        };
        let (mut texture, mut query) = (None, None);
        // SAFETY: the description and the initial data are complete and outlive the calls, the out-pointers are
        // valid, and the texture is created shareable through NT handles, which the `OwnedHandle` then owns.
        unsafe {
            device.CreateTexture2D(&desc, Some(&data), Some(&mut texture)).unwrap();
            let resource: IDXGIResource1 = texture.unwrap().cast().unwrap();
            let handle = resource.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()).unwrap();

            // Finished before it is painted, as GPUI requires.
            let context = device.GetImmediateContext().unwrap();
            device
                .CreateQuery(&D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 }, Some(&mut query))
                .unwrap();
            let query = query.unwrap();
            context.End(&query);
            context.Flush();
            let mut done = BOOL(0);
            while context.GetData(&query, Some((&raw mut done).cast()), size_of::<BOOL>() as u32, 0).is_err()
                || !done.as_bool()
            {
                std::thread::yield_now();
            }
            OwnedHandle::from_raw_handle(handle.0)
        }
    }

    /// A device on the adapter GPUI picks: the first that creates a Direct3D 11 device.
    fn gpui_device() -> ID3D11Device {
        // SAFETY: plain factory creation and enumeration; the out-pointers are valid, and each adapter is used with the
        // driver type it requires.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
            (0..)
                .map_while(|index| factory.EnumAdapters1(index).ok())
                .find_map(|adapter| {
                    let mut device = None;
                    D3D11CreateDevice(
                        &adapter,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1]),
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        None,
                    )
                    .ok()?;
                    device
                })
                .expect("an adapter with Direct3D 11")
        }
    }

    /// Paints a shared texture the way the video view does, letterboxed with `fit`.
    struct Painted {
        handle: std::sync::Arc<OwnedHandle>,
        size: (u32, u32),
    }

    impl Render for Painted {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (handle, (width, height)) = (self.handle.clone(), self.size);
            div().size_full().bg(black()).child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        paint_handle(window, fit(bounds, width, height), handle.as_handle());
                    },
                )
                .size_full(),
            )
        }
    }

    /// The CI test of the patched draw path: needs no OpenGL or mpv, so it runs on Mesa's leg too.
    #[test]
    fn shared_texture_is_drawn_upright_and_letterboxed() {
        let Some(mut cx) = headless() else {
            return;
        };
        let handle = std::sync::Arc::new(quadrant_texture());
        // 160x90 points at the test windows' scale of 2: 320x180 pixels, in which the 4:3 texture fills 240x180.
        let window = open(&mut cx, 160., 90., || Painted { handle, size: (64, 48) });
        for frame in 0..3 {
            // Each draw paints the texture through a new duplicate of its handle.
            assert_quadrants_letterboxed(&screenshot(&mut cx, window), &format!("frame {frame}"));
        }
    }

    /// The whole path on the local GPU: mpv's frames through the core's renderer and the element into GPUI's.
    #[test]
    fn video_is_drawn_upright_in_its_colours_and_letterboxed() {
        if cannot_show_video() {
            return;
        }
        let Some(mut cx) = headless() else {
            return;
        };
        assert_video_drawn(&mut cx);
    }
}
