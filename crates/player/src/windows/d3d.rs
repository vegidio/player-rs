//! DXGI adapters, the render thread's Direct3D 11 device, the textures frames are drawn and published in, and the
//! wait for the GPU.

use super::AdapterLuid;
use crate::error::{Error, Result};
use ::windows::Win32::Foundation::{GENERIC_ALL, HMODULE};
use ::windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS, D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS, D3D11_QUERY_DESC,
    D3D11_QUERY_EVENT, D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Query,
    ID3D11Texture2D,
};
use ::windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use ::windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter1, IDXGIFactory1, IDXGIResource1,
};
use ::windows::core::{BOOL, Interface, PCWSTR};
use std::ffi::c_void;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long to wait for the GPU to finish a frame before giving up on it.
const FINISH_TIMEOUT: Duration = Duration::from_secs(1);

/// A DXGI adapter: a GPU, or WARP, Windows' software renderer.
pub(super) struct Adapter {
    pub(super) luid: AdapterLuid,
    /// The PCI vendor ID.
    pub(super) vendor: u32,
    pub(super) name: String,
}

/// Creates a device on the adapter with `luid`, or by default on the first adapter that supports Direct3D 11 as GPUI
/// uses it, which is the adapter GPUI picks.
pub(super) fn device(luid: Option<AdapterLuid>) -> Result<(Adapter, Device)> {
    // SAFETY: no flags; the factory is released when dropped.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
        .map_err(|error| Error::Render(format!("no DXGI factory: {error}")))?;
    let mut failures = vec![];
    // SAFETY: enumeration ends with an error past the last adapter.
    for adapter in (0..).map_while(|index| unsafe { factory.EnumAdapters1(index) }.ok()) {
        // SAFETY: `adapter` is valid.
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        let found = Adapter {
            luid: AdapterLuid { low: desc.AdapterLuid.LowPart, high: desc.AdapterLuid.HighPart },
            vendor: desc.VendorId,
            name: String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').to_owned(),
        };
        match luid {
            Some(luid) if luid != found.luid => {}
            Some(_) => {
                return match Device::new(&adapter) {
                    Ok(device) => Ok((found, device)),
                    Err(error) => Err(Error::Render(format!(
                        "the adapter {} ({}) has no Direct3D 11 device: {error}",
                        found.name, found.luid
                    ))),
                };
            }
            None => match Device::new(&adapter) {
                Ok(device) => return Ok((found, device)),
                Err(error) => failures.push(format!("{}: {error}", found.name)),
            },
        }
    }
    Err(Error::Render(match luid {
        Some(luid) => format!("no graphics adapter has the LUID {luid}"),
        None => format!("no graphics adapter supports Direct3D 11: {failures:?}"),
    }))
}

/// A Direct3D 11 device and its immediate context, used on the render thread only.
pub(super) struct Device {
    pub(super) raw: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Signalled once the GPU has finished the work before it.
    query: ID3D11Query,
}

impl Device {
    /// A device on `adapter` with the requirements GPUI's renderer has, so an adapter GPUI uses is never skipped.
    fn new(adapter: &IDXGIAdapter1) -> std::result::Result<Device, ::windows::core::Error> {
        let (mut raw, mut context) = (None, None);
        // SAFETY: the out-pointers are valid; the adapter is used with the driver type it requires.
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1]),
                D3D11_SDK_VERSION,
                Some(&mut raw),
                None,
                Some(&mut context),
            )?;
        }
        let (Some(raw), Some(context)) = (raw, context) else {
            return Err(::windows::core::Error::empty());
        };
        let mut options = D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS::default();
        // SAFETY: the data pointer and size describe `options`, the structure this feature fills.
        unsafe {
            raw.CheckFeatureSupport(
                D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS,
                (&raw mut options).cast::<c_void>(),
                size_of_val(&options) as u32,
            )?;
        }
        if !options.ComputeShaders_Plus_RawAndStructuredBuffers_Via_Shader_4_x.as_bool() {
            return Err(::windows::core::Error::new(::windows::Win32::Foundation::E_NOTIMPL, "no compute shaders"));
        }
        let mut query = None;
        // SAFETY: the description is complete and the out-pointer valid.
        unsafe { raw.CreateQuery(&D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 }, Some(&mut query))? };
        let query = query.ok_or_else(::windows::core::Error::empty)?;
        Ok(Device { raw, context, query })
    }

    /// A `width` x `height` BGRA texture GL and Direct3D can draw into, shareable through NT handles if `shared`.
    pub(super) fn texture(&self, width: u32, height: u32, shared: bool) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: if shared {
                (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32
            } else {
                0
            },
        };
        let mut texture = None;
        // SAFETY: the description is complete, there is no initial data, and the out-pointer is valid.
        unsafe { self.raw.CreateTexture2D(&desc, None, Some(&mut texture)) }
            .map_err(|error| Error::Render(format!("no {width}x{height} Direct3D 11 texture: {error}")))?;
        texture.ok_or_else(|| Error::Render(format!("no {width}x{height} Direct3D 11 texture")))
    }

    /// Copies `source` into `destination`, of the same size and format, on the GPU.
    pub(super) fn copy(&self, destination: &ID3D11Texture2D, source: &ID3D11Texture2D) {
        // SAFETY: both textures belong to this device and match; the context is used on this thread only.
        unsafe { self.context.CopyResource(destination, source) };
    }

    /// Waits until the GPU has finished all work submitted so far, so another device can read its results.
    pub(super) fn finish(&self) -> Result<()> {
        // SAFETY: the query and context belong to this device; the data pointer and size describe `done`.
        unsafe {
            self.context.End(&self.query);
            self.context.Flush();
            let start = Instant::now();
            loop {
                let mut done = BOOL(0);
                self.context
                    .GetData(&self.query, Some((&raw mut done).cast()), size_of::<BOOL>() as u32, 0)
                    .map_err(|error| Error::Render(format!("the GPU's progress cannot be read: {error}")))?;
                if done.as_bool() {
                    return Ok(());
                }
                if start.elapsed() > FINISH_TIMEOUT {
                    return Err(Error::Render("the GPU did not finish the frame".into()));
                }
                std::thread::yield_now();
            }
        }
    }
}

/// A texture of the ring frames are published in, and its NT handle, which [`Frame`](super::Frame)s share. An open
/// handle keeps the texture alive, also after the device is released.
pub(super) struct Slot {
    pub(super) texture: ID3D11Texture2D,
    pub(super) handle: Arc<OwnedHandle>,
}

impl Slot {
    pub(super) fn new(device: &Device, width: u32, height: u32) -> Result<Slot> {
        let texture = device.texture(width, height, true)?;
        let fail = |error: ::windows::core::Error| Error::Render(format!("the texture cannot be shared: {error}"));
        let resource: IDXGIResource1 = texture.cast().map_err(fail)?;
        // SAFETY: the texture was created with MISC_SHARED_NTHANDLE; no security attributes or name are given.
        let handle = unsafe { resource.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }.map_err(fail)?;
        // SAFETY: `CreateSharedHandle` returned a new NT handle, which the `OwnedHandle` now owns and closes.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        Ok(Slot { texture, handle: Arc::new(handle) })
    }
}

#[cfg(test)]
pub(in crate::windows) mod tests {
    use super::*;
    use ::windows::Win32::Foundation::HANDLE;
    use ::windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING, ID3D11Device1,
        ID3D11RenderTargetView,
    };
    use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle};

    /// Opens a shared texture on a new device on the default adapter and reads it as rows of BGRA bytes, top row
    /// first, as an application reading a frame would.
    pub(in crate::windows) fn read_back(handle: BorrowedHandle<'_>) -> (u32, u32, Vec<Vec<u8>>) {
        let (_, device) = super::device(None).expect("a second device is created");
        let device1: ID3D11Device1 = device.raw.cast().unwrap();
        // SAFETY: the handle is a valid NT handle to a shared texture, and the out-pointers below are valid.
        unsafe {
            let texture: ID3D11Texture2D =
                device1.OpenSharedResource1(HANDLE(handle.as_raw_handle())).expect("the handle opens");
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            assert_eq!(desc.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
            let staging = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..desc
            };
            let mut copy = None;
            device.raw.CreateTexture2D(&staging, None, Some(&mut copy)).unwrap();
            let copy = copy.unwrap();
            device.copy(&copy, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            device.context.Map(&copy, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
            let rows = (0..desc.Height as usize)
                .map(|row| {
                    let start = mapped.pData.cast::<u8>().add(row * mapped.RowPitch as usize);
                    std::slice::from_raw_parts(start, desc.Width as usize * 4).to_vec()
                })
                .collect();
            device.context.Unmap(&copy, 0);
            (desc.Width, desc.Height, rows)
        }
    }

    fn clear(device: &Device, texture: &ID3D11Texture2D, colour: [f32; 4]) {
        let mut view: Option<ID3D11RenderTargetView> = None;
        // SAFETY: the texture is a render target of this device; the out-pointer is valid.
        unsafe {
            device.raw.CreateRenderTargetView(texture, None, Some(&mut view)).unwrap();
            device.context.ClearRenderTargetView(&view.unwrap(), &colour);
        }
    }

    #[test]
    fn default_adapter_has_a_device() {
        let (adapter, _device) = device(None).expect("an adapter supports Direct3D 11");
        assert!(!adapter.name.is_empty());
        let (again, _) = device(Some(adapter.luid)).expect("the adapter is found by its LUID");
        assert_eq!(again.luid, adapter.luid);
    }

    #[test]
    fn unknown_luid_is_an_error() {
        let luid = AdapterLuid { low: 0xdead_beef, high: 0x7654_3210 };
        let Err(Error::Render(message)) = device(Some(luid)) else { panic!("an adapter was found") };
        assert!(message.contains(&luid.to_string()), "{message}");
    }

    #[test]
    fn slot_opens_on_a_second_device() {
        let (_, device) = device(None).unwrap();
        let slot = Slot::new(&device, 64, 48).unwrap();
        clear(&device, &slot.texture, [1.0, 0.5, 0.0, 1.0]);
        device.finish().unwrap();

        let (width, height, rows) = read_back(slot.handle.as_handle());
        assert_eq!((width, height), (64, 48));
        for (y, row) in rows.iter().enumerate() {
            for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
                let close = pixel.iter().zip([0u8, 128, 255, 255]).all(|(a, e)| a.abs_diff(e) <= 1);
                assert!(close, "pixel ({x}, {y}) is {pixel:?}");
            }
        }
    }

    #[test]
    fn slot_outlives_its_device() {
        let handle = {
            let (_, device) = device(None).unwrap();
            let slot = Slot::new(&device, 64, 48).unwrap();
            clear(&device, &slot.texture, [0.0, 0.0, 1.0, 1.0]);
            device.finish().unwrap();
            Arc::clone(&slot.handle)
        };
        let (_, _, rows) = read_back(handle.as_handle());
        assert_eq!(&rows[47][..4], [255, 0, 0, 255], "the texture outlives the device and its slot");
    }
}
