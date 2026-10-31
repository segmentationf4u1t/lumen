//! HID access, split per OS. Windows sees one interface per top-level
//! collection (SetupAPI); Linux sees one `/dev/hidraw*` node per USB
//! interface and lists each of its top-level collections separately, so the
//! selection logic below is the same on both.

use crate::error::{Error, Result};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

pub const VENDOR_ID: u16 = 0x1E7D;
pub const PRODUCT_ID: u16 = 0x2F09;
pub const USAGE_PAGE_CONTROL: u16 = 0xFF01;
pub const USAGE_PAGE_LED: u16 = 0xFF00;

/// One top-level collection. Report lengths include the report ID byte,
/// which is always sent first (0 when the device has no report IDs).
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
        sys::enumerate()
    }

    pub fn is_target(&self) -> bool {
        self.vendor_id == VENDOR_ID && self.product_id == PRODUCT_ID
    }
}

/// Owned HID handle. Not internally synchronized: use it from one thread at a time.
pub struct HidHandle {
    device: sys::Device,
    info: HidInterface,
}

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
        let matching: Vec<HidInterface> = HidInterface::enumerate()?
            .into_iter()
            .filter(|i| {
                i.vendor_id == vendor_id && i.product_id == product_id && i.usage_page == usage_page
            })
            .collect();
        let blocked = matching.iter().find_map(|i| i.open_error.clone());
        let mut candidates: Vec<HidInterface> = matching
            .into_iter()
            .filter(|i| i.open_error.is_none())
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
            let mut msg = format!(
                "no HID interface {vendor_id:04x}:{product_id:04x} usage_page {usage_page:04x} ({})",
                sys::BUSY_HINT
            );
            if let Some(e) = blocked {
                msg.push_str("; ");
                msg.push_str(&e);
            }
            Error::new(msg)
        })?;

        Ok(Self {
            device: sys::Device::open(&found.path)?,
            info: found,
        })
    }

    pub fn path(&self) -> &str {
        &self.info.path
    }

    pub fn product(&self) -> &str {
        &self.info.product
    }

    pub fn usage_page(&self) -> u16 {
        self.info.usage_page
    }

    pub fn output_len(&self) -> u16 {
        self.info.output_len
    }

    pub fn feature_len(&self) -> u16 {
        self.info.feature_len
    }

    /// `buf[0]` is the report ID.
    pub fn set_feature(&self, buf: &[u8]) -> Result<()> {
        if buf.is_empty() {
            return Err(Error::new("empty feature report"));
        }
        self.device.set_feature(buf)
    }

    /// Set `buf[0]` to the report ID; the reply overwrites `buf` from byte 0.
    pub fn get_feature(&self, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Err(Error::new("empty feature report"));
        }
        self.device.get_feature(buf)
    }

    pub fn write_output(&self, buf: &[u8]) -> Result<()> {
        if buf.is_empty() {
            return Err(Error::new("empty output report"));
        }
        self.device.write_output(buf)
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
