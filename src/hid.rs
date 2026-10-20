use crate::error::{Error, Result};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidD_GetProductString, HidD_GetSerialNumberString, HidD_SetFeature,
    HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

pub const VENDOR_ID: u16 = 0x1E7D;
pub const PRODUCT_ID: u16 = 0x2F09;
pub const USAGE_PAGE_CONTROL: u16 = 0xFF01;
pub const USAGE_PAGE_LED: u16 = 0xFF00;

#[derive(Clone, Debug)]
pub struct HidInterface {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub input_len: u16,
    pub output_len: u16,
    pub feature_len: u16,
    pub product: String,
    pub serial: String,
    pub open_error: Option<String>,
}

impl HidInterface {
    pub fn enumerate() -> Result<Vec<Self>> {
        let mut guid = windows_sys::core::GUID::default();
        unsafe { HidD_GetHidGuid(&mut guid) };

        let set = unsafe {
            SetupDiGetClassDevsW(
                &guid,
                null(),
                null_mut(),
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )
        };
        if set == INVALID_HANDLE_VALUE as isize || set == 0 {
            return Err(Error::last_os("SetupDiGetClassDevsW"));
        }

        let mut out = Vec::new();
        let mut index = 0u32;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            let ok = unsafe { SetupDiEnumDeviceInterfaces(set, null(), &guid, index, &mut iface) };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                if err == windows_sys::Win32::Foundation::ERROR_NO_MORE_ITEMS {
                    break;
                }
                unsafe { SetupDiDestroyDeviceInfoList(set) };
                return Err(Error::last_os("SetupDiEnumDeviceInterfaces"));
            }
            index += 1;

            let mut required = 0u32;
            unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &iface,
                    null_mut(),
                    0,
                    &mut required,
                    null_mut(),
                )
            };
            if required == 0 {
                continue;
            }

            let mut buf = vec![0u8; required as usize];
            // SP_DEVICE_INTERFACE_DETAIL_DATA_W.cbSize is the struct size, not the
            // path buffer size. 8 on 64-bit Unicode, 6 on 32-bit Unicode.
            let cb_size: u32 = if cfg!(target_pointer_width = "64") {
                8
            } else {
                6
            };
            unsafe { *(buf.as_mut_ptr() as *mut u32) = cb_size };
            let ok = unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &iface,
                    buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W,
                    required,
                    null_mut(),
                    null_mut(),
                )
            };
            if ok == 0 {
                continue;
            }

            let path = unsafe {
                let ptr = buf.as_ptr().add(4) as *const u16;
                // Stay inside the buffer returned by SetupAPI. The path is
                // NUL-terminated, but never trust a driver boundary blindly.
                let max_units = (required as usize).saturating_sub(4) / 2;
                let mut len = 0usize;
                while len < max_units && *ptr.add(len) != 0 {
                    len += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
            };
            if path.is_empty() {
                continue;
            }

            let (handle, open_error) = match open_path(&path, GENERIC_READ | GENERIC_WRITE) {
                Ok(h) => (h, None),
                Err(e) => match open_path(&path, 0) {
                    Ok(h) => (h, Some(e.to_string())),
                    Err(_) => (INVALID_HANDLE_VALUE, Some(e.to_string())),
                },
            };

            if is_invalid_handle(handle) {
                out.push(HidInterface {
                    path,
                    vendor_id: 0,
                    product_id: 0,
                    usage_page: 0,
                    usage: 0,
                    input_len: 0,
                    output_len: 0,
                    feature_len: 0,
                    product: String::new(),
                    serial: String::new(),
                    open_error,
                });
                continue;
            }

            let mut attr = HIDD_ATTRIBUTES {
                Size: size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };
            let got_attr = unsafe { HidD_GetAttributes(handle, &mut attr) };

            let caps = read_caps(handle);
            let product = read_wide_string(handle, false);
            let serial = read_wide_string(handle, true);

            unsafe { CloseHandle(handle) };

            out.push(HidInterface {
                path,
                vendor_id: if got_attr { attr.VendorID } else { 0 },
                product_id: if got_attr { attr.ProductID } else { 0 },
                usage_page: caps.map(|c| c.UsagePage).unwrap_or(0),
                usage: caps.map(|c| c.Usage).unwrap_or(0),
                input_len: caps.map(|c| c.InputReportByteLength).unwrap_or(0),
                output_len: caps.map(|c| c.OutputReportByteLength).unwrap_or(0),
                feature_len: caps.map(|c| c.FeatureReportByteLength).unwrap_or(0),
                product,
                serial,
                open_error,
            });
        }

        unsafe { SetupDiDestroyDeviceInfoList(set) };
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    pub fn is_target(&self) -> bool {
        self.vendor_id == VENDOR_ID && self.product_id == PRODUCT_ID
    }
}

fn read_caps(handle: HANDLE) -> Option<HIDP_CAPS> {
    let mut preparsed: PHIDP_PREPARSED_DATA = 0;
    let ok = unsafe { HidD_GetPreparsedData(handle, &mut preparsed) };
    if !ok {
        return None;
    }
    let mut caps = HIDP_CAPS::default();
    let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };
    unsafe { HidD_FreePreparsedData(preparsed) };
    if status == HIDP_STATUS_SUCCESS {
        Some(caps)
    } else {
        None
    }
}

fn read_wide_string(handle: HANDLE, serial: bool) -> String {
    let mut buf = [0u16; 256];
    let ok = unsafe {
        if serial {
            HidD_GetSerialNumberString(handle, buf.as_mut_ptr() as *mut c_void, 512)
        } else {
            HidD_GetProductString(handle, buf.as_mut_ptr() as *mut c_void, 512)
        }
    };
    if !ok {
        return String::new();
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn is_invalid_handle(h: HANDLE) -> bool {
    h.is_null() || h == INVALID_HANDLE_VALUE
}

fn open_path(path: &str, access: u32) -> Result<HANDLE> {
    let w = wide(path);
    let handle = unsafe {
        CreateFileW(
            w.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if is_invalid_handle(handle) {
        Err(Error::last_os(&format!("CreateFileW({path})")))
    } else {
        Ok(handle)
    }
}

/// Owned HID handle. Not internally synchronized: use it from one thread at a time.
pub struct HidHandle {
    handle: HANDLE,
    path: String,
    product: String,
    caps: HIDP_CAPS,
}

unsafe impl Send for HidHandle {}

impl HidHandle {
    pub fn open_usage(vendor_id: u16, product_id: u16, usage_page: u16) -> Result<Self> {
        Self::open_usage_with_hint(vendor_id, product_id, usage_page, None)
    }

    /// Open the first usable collection for `usage_page`. If `path_hint` is a
    /// substring of one path, prefer that one; otherwise fall back to the first.
    pub fn open_usage_with_hint(
        vendor_id: u16,
        product_id: u16,
        usage_page: u16,
        path_hint: Option<&str>,
    ) -> Result<Self> {
        let mut candidates: Vec<HidInterface> = HidInterface::enumerate()?
            .into_iter()
            .filter(|i| {
                i.vendor_id == vendor_id
                    && i.product_id == product_id
                    && i.usage_page == usage_page
                    && i.open_error.is_none()
            })
            .collect();

        if let Some(hint) = path_hint {
            if let Some(pos) = candidates.iter().position(|i| {
                i.path
                    .to_ascii_lowercase()
                    .contains(&hint.to_ascii_lowercase())
            }) {
                let found = candidates.remove(pos);
                candidates.insert(0, found);
            }
        }

        let found = candidates.into_iter().next().ok_or_else(|| {
            Error::new(format!(
                "no HID interface {vendor_id:04x}:{product_id:04x} usage_page {usage_page:04x} (close SWARM II/OpenRGB if the device is busy)"
            ))
        })?;

        let handle = open_path(&found.path, GENERIC_READ | GENERIC_WRITE)?;
        let caps = read_caps(handle).ok_or_else(|| Error::new("HidP_GetCaps failed"))?;
        let product = read_wide_string(handle, false);
        Ok(Self {
            handle,
            path: found.path,
            product,
            caps,
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn product(&self) -> &str {
        &self.product
    }

    pub fn usage_page(&self) -> u16 {
        self.caps.UsagePage
    }

    pub fn output_len(&self) -> u16 {
        self.caps.OutputReportByteLength
    }

    pub fn feature_len(&self) -> u16 {
        self.caps.FeatureReportByteLength
    }

    pub fn set_feature(&self, buf: &[u8]) -> Result<()> {
        if buf.is_empty() {
            return Err(Error::new("empty feature report"));
        }
        let ok = unsafe {
            HidD_SetFeature(self.handle, buf.as_ptr() as *const c_void, buf.len() as u32)
        };
        if !ok {
            Err(Error::last_os(&format!(
                "HidD_SetFeature(report 0x{:02x}, {} bytes)",
                buf[0],
                buf.len()
            )))
        } else {
            Ok(())
        }
    }

    pub fn get_feature(&self, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Err(Error::new("empty feature report"));
        }
        let ok = unsafe {
            HidD_GetFeature(
                self.handle,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
            )
        };
        if !ok {
            Err(Error::last_os(&format!(
                "HidD_GetFeature(report 0x{:02x}, {} bytes)",
                buf[0],
                buf.len()
            )))
        } else {
            Ok(())
        }
    }

    pub fn write_output(&self, buf: &[u8]) -> Result<()> {
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                self.handle,
                buf.as_ptr(),
                buf.len() as u32,
                &mut written,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(Error::last_os(&format!("WriteFile({} bytes)", buf.len())));
        }
        if written as usize != buf.len() {
            return Err(Error::new(format!(
                "short HID write: {written}/{} bytes",
                buf.len()
            )));
        }
        Ok(())
    }
}

impl Drop for HidHandle {
    fn drop(&mut self) {
        if !is_invalid_handle(self.handle) {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

/// Helper for probing: list every HID interface with raw metadata.
pub fn dump_interfaces(only_target: bool) -> Result<Vec<HidInterface>> {
    let all = HidInterface::enumerate()?;
    Ok(if only_target {
        all.into_iter().filter(|i| i.is_target()).collect()
    } else {
        all
    })
}
