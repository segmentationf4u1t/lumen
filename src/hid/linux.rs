//! Linux HID backend over hidraw: sysfs enumeration, report-descriptor
//! parsing for usage pages and report lengths, `HIDIOC[SG]FEATURE` ioctls,
//! plain `write(2)` for output reports.

use super::HidInterface;
use crate::error::{Error, Result};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

// The feature ioctls below use the asm-generic `_IOC` layout.
#[cfg(any(
    target_arch = "powerpc",
    target_arch = "powerpc64",
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "sparc",
    target_arch = "sparc64"
))]
compile_error!("hidraw ioctl encoding for this architecture is not implemented");

pub(super) const BUSY_HINT: &str = "close OpenRGB if the device is busy";
const ACCESS_HINT: &str = "install assets/70-vulcan.rules, see README";
const SYSFS_HIDRAW: &str = "/sys/class/hidraw";
const BUS_USB: u16 = 0x0003;

pub(super) fn enumerate() -> Result<Vec<HidInterface>> {
    let entries = match fs::read_dir(SYSFS_HIDRAW) {
        Ok(entries) => entries,
        // No hidraw device exists (or hidraw is not built into the kernel).
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::new(format!("read {SYSFS_HIDRAW}: {e}"))),
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = Path::new("/dev")
            .join(entry.file_name())
            .to_string_lossy()
            .into_owned();
        let device = entry.path().join("device");
        let uevent = fs::read_to_string(device.join("uevent")).unwrap_or_default();
        let (bus, vendor_id, product_id) = uevent_value(&uevent, "HID_ID")
            .and_then(parse_hid_id)
            .unwrap_or((0, 0, 0));
        let product = (bus == BUS_USB)
            .then(|| usb_product(&device))
            .flatten()
            .or_else(|| uevent_value(&uevent, "HID_NAME").map(str::to_owned))
            .unwrap_or_default();
        let base = HidInterface {
            open_error: check_access(&path).err(),
            path,
            vendor_id,
            product_id,
            usage_page: 0,
            usage: 0,
            input_len: 0,
            output_len: 0,
            feature_len: 0,
            product,
            serial: uevent_value(&uevent, "HID_UNIQ").unwrap_or("").to_owned(),
        };

        let descriptor = fs::read(device.join("report_descriptor")).unwrap_or_default();
        let collections = parse_descriptor(&descriptor);
        if collections.is_empty() {
            out.push(base);
            continue;
        }
        for c in collections {
            out.push(HidInterface {
                usage_page: c.usage_page,
                usage: c.usage,
                input_len: c.input_len,
                output_len: c.output_len,
                feature_len: c.feature_len,
                ..base.clone()
            });
        }
    }
    // hidraw2 before hidraw10. Stable, so collections keep descriptor order.
    out.sort_by(|a, b| (a.path.len(), &a.path).cmp(&(b.path.len(), &b.path)));
    Ok(out)
}

fn uevent_value<'a>(uevent: &'a str, key: &str) -> Option<&'a str> {
    uevent
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

/// `HID_ID=0003:00001E7D:00002F09` -> (bus, vendor, product).
fn parse_hid_id(id: &str) -> Option<(u16, u16, u16)> {
    let mut parts = id.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    let bus = parts.next()??;
    let vendor = parts.next()??;
    let product = parts.next()??;
    Some((bus as u16, vendor as u16, product as u16))
}

/// The USB product string (what Windows' `HidD_GetProductString` returns).
/// sysfs: hidrawN/device -> HID device; its parent is the USB interface and
/// the grandparent the USB device carrying `product`.
fn usb_product(device: &Path) -> Option<String> {
    let usb_device = fs::canonicalize(device).ok()?.parent()?.parent()?.join("product");
    let product = fs::read_to_string(usb_device).ok()?;
    Some(product.trim().to_owned())
}

fn access_error(path: &str, e: std::io::Error) -> String {
    if matches!(e.kind(), ErrorKind::PermissionDenied) {
        format!("{path}: {e} ({ACCESS_HINT})")
    } else {
        format!("{path}: {e}")
    }
}

/// Permission check without opening: opening a hidraw node powers up the
/// device and starts its interrupt pipe, which unrelated devices don't need.
fn check_access(path: &str) -> Result<(), String> {
    let c_path = CString::new(path).map_err(|_| format!("{path}: invalid path"))?;
    if unsafe { libc::access(c_path.as_ptr(), libc::R_OK | libc::W_OK) } == 0 {
        Ok(())
    } else {
        Err(access_error(path, std::io::Error::last_os_error()))
    }
}

const HIDIOCSFEATURE: u8 = 0x06;
const HIDIOCGFEATURE: u8 = 0x07;

/// `_IOC(_IOC_READ | _IOC_WRITE, 'H', nr, len)`; the length field is 14 bits.
fn hidioc_feature(nr: u8, len: usize) -> Result<libc::Ioctl> {
    const IOC_WRITE: u64 = 1;
    const IOC_READ: u64 = 2;
    if len >= 1 << 14 {
        return Err(Error::new(format!("feature report too long: {len} bytes")));
    }
    let request =
        ((IOC_READ | IOC_WRITE) << 30) | ((len as u64) << 16) | ((b'H' as u64) << 8) | nr as u64;
    Ok(request as libc::Ioctl)
}

pub(super) struct Device {
    file: File,
}

impl Device {
    pub(super) fn open(path: &str) -> Result<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map(|file| Self { file })
            .map_err(|e| Error::new(format!("open {}", access_error(path, e))))
    }

    pub(super) fn set_feature(&self, buf: &[u8]) -> Result<()> {
        let request = hidioc_feature(HIDIOCSFEATURE, buf.len())?;
        // SAFETY: the request encodes `buf.len()`; the kernel reads at most that many bytes.
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), request, buf.as_ptr()) };
        if rc < 0 {
            Err(Error::last_os(&format!(
                "HIDIOCSFEATURE(report 0x{:02x}, {} bytes)",
                buf[0],
                buf.len()
            )))
        } else {
            Ok(())
        }
    }

    pub(super) fn get_feature(&self, buf: &mut [u8]) -> Result<()> {
        let request = hidioc_feature(HIDIOCGFEATURE, buf.len())?;
        let report = buf[0];
        // SAFETY: the request encodes `buf.len()`; the kernel writes at most that many bytes.
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), request, buf.as_mut_ptr()) };
        if rc < 0 {
            Err(Error::last_os(&format!(
                "HIDIOCGFEATURE(report 0x{report:02x}, {} bytes)",
                buf.len()
            )))
        } else {
            Ok(())
        }
    }

    /// hidraw takes the report ID as the first byte, 0 for devices without
    /// report IDs, exactly like `WriteFile` on Windows.
    pub(super) fn write_output(&self, buf: &[u8]) -> Result<()> {
        match (&self.file).write(buf) {
            Ok(n) if n == buf.len() => Ok(()),
            Ok(n) => Err(Error::new(format!(
                "short HID write: {n}/{} bytes",
                buf.len()
            ))),
            Err(e) => Err(Error::new(format!("hidraw write({} bytes): {e}", buf.len()))),
        }
    }
}

/// A top-level collection with its largest report of each kind, in bytes
/// including the report ID byte (Windows' `HIDP_CAPS::*ReportByteLength`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Collection {
    usage_page: u16,
    usage: u16,
    input_len: u16,
    output_len: u16,
    feature_len: u16,
}

const TAG_INPUT: u8 = 0x8;
const TAG_OUTPUT: u8 = 0x9;
const TAG_COLLECTION: u8 = 0xA;
const TAG_FEATURE: u8 = 0xB;
const TAG_END_COLLECTION: u8 = 0xC;

/// Walk a HID report descriptor (HID 1.11, section 6.2.2) far enough to find
/// the top-level collections and their report sizes.
fn parse_descriptor(desc: &[u8]) -> Vec<Collection> {
    #[derive(Clone, Copy, Default)]
    struct Globals {
        usage_page: u32,
        report_size: u32,
        report_count: u32,
        report_id: u8,
    }

    struct Open {
        usage_page: u16,
        usage: u16,
        /// (main item tag, report ID, total bits)
        reports: Vec<(u8, u8, u32)>,
    }

    impl Open {
        fn len(&self, kind: u8) -> u16 {
            self.reports
                .iter()
                .filter(|r| r.0 == kind)
                .map(|r| r.2.div_ceil(8) + 1)
                .max()
                .unwrap_or(0)
                .min(u16::MAX as u32) as u16
        }
    }

    let mut out = Vec::new();
    let mut globals = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    // First Usage local item before the next main item: (data, 32-bit form).
    let mut usage: Option<(u32, bool)> = None;
    let mut depth = 0usize;
    let mut current: Option<Open> = None;

    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xFE {
            // Long item: bDataSize, bLongItemTag, data. Reserved; skip.
            i += 3 + desc.get(i + 1).copied().unwrap_or(0) as usize;
            continue;
        }
        let size = match prefix & 0x03 {
            3 => 4,
            n => n as usize,
        };
        let Some(bytes) = desc.get(i + 1..i + 1 + size) else {
            break;
        };
        let data = bytes
            .iter()
            .rev()
            .fold(0u32, |acc, &b| (acc << 8) | b as u32);
        i += 1 + size;
        let tag = prefix >> 4;

        match (prefix >> 2) & 0x03 {
            // Main
            0 => {
                match tag {
                    TAG_INPUT | TAG_OUTPUT | TAG_FEATURE => {
                        if let Some(open) = current.as_mut() {
                            let bits = globals.report_size.saturating_mul(globals.report_count);
                            let id = globals.report_id;
                            match open.reports.iter_mut().find(|r| r.0 == tag && r.1 == id) {
                                Some(r) => r.2 = r.2.saturating_add(bits),
                                None => open.reports.push((tag, id, bits)),
                            }
                        }
                    }
                    TAG_COLLECTION => {
                        if depth == 0 {
                            let (page, id) = match usage {
                                Some((data, true)) => (data >> 16, data & 0xFFFF),
                                Some((data, false)) => (globals.usage_page, data),
                                None => (globals.usage_page, 0),
                            };
                            current = Some(Open {
                                usage_page: page as u16,
                                usage: id as u16,
                                reports: Vec::new(),
                            });
                        }
                        depth += 1;
                    }
                    TAG_END_COLLECTION => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            if let Some(open) = current.take() {
                                out.push(Collection {
                                    usage_page: open.usage_page,
                                    usage: open.usage,
                                    input_len: open.len(TAG_INPUT),
                                    output_len: open.len(TAG_OUTPUT),
                                    feature_len: open.len(TAG_FEATURE),
                                });
                            }
                        }
                    }
                    _ => {}
                }
                // Local items only apply to the main item that follows them.
                usage = None;
            }
            // Global
            1 => match tag {
                0x0 => globals.usage_page = data,
                0x7 => globals.report_size = data,
                0x8 => globals.report_id = data as u8,
                0x9 => globals.report_count = data,
                0xA => stack.push(globals),
                0xB => {
                    if let Some(saved) = stack.pop() {
                        globals = saved;
                    }
                }
                _ => {}
            },
            // Local: Usage
            2 if tag == 0x0 && usage.is_none() => usage = Some((data, size == 4)),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Interface 3 of a real Vulcan II Mini: the LED stream collection, no
    /// report IDs, 64-byte input and output reports.
    #[test]
    fn led_interface_without_report_ids() {
        let desc = hex(
            "0600ff0901a10105070900150026ff0075089540810205080900150026ff00750895409102c0",
        );
        assert_eq!(
            parse_descriptor(&desc),
            vec![Collection {
                usage_page: 0xFF00,
                usage: 0x01,
                input_len: 65,
                output_len: 65,
                feature_len: 0,
            }]
        );
    }

    /// Interface 1 of a real Vulcan II Mini: mouse, system control, consumer
    /// and two vendor collections behind report IDs. The control collection's
    /// lengths must match the reports `protocol` sends: 0x04 is 4 bytes,
    /// 0x11 is 332 and the largest (0x08) is 1044 with its 2-byte count.
    #[test]
    fn control_interface_with_report_ids_and_nested_collections() {
        let desc = hex(concat!(
            "05010902a10185010901a1000509190129051500250195057501810295017503810105010930093116",
            "008026ff7f95027510810609381581257f950175088106c0c005010980a1018502198129831500250175",
            "019503810295058101c0050c0901a101854219002aff02150026ff02751095018100c00602ff0901a101",
            "8503150026ff00092f750895078100c00601ff0901a10185041900297f1500257f75089503b101850595",
            "03b10185069585b1018507950ab1018508961304b10185099508b101850a960501b101850b952cb10185",
            "0c960502b101850d950fb101850e9504b101850f957fb1018510950fb1018511964b01b1018512950db1",
            "018513957ab1018514956fb1018515951fb1018516957ab101c0",
        ));
        let collections = parse_descriptor(&desc);
        let pages: Vec<(u16, u16)> = collections
            .iter()
            .map(|c| (c.usage_page, c.usage))
            .collect();
        assert_eq!(
            pages,
            [(0x01, 0x02), (0x01, 0x80), (0x0C, 0x01), (0xFF02, 0x01), (0xFF01, 0x01)]
        );
        let control = collections[4];
        assert_eq!(
            (control.input_len, control.output_len, control.feature_len),
            (0, 0, 1044)
        );
        // The mouse report spans a nested physical collection.
        assert!(collections[0].input_len > 1);
    }

    #[test]
    fn hid_id_and_uevent_parsing() {
        let uevent = "DRIVER=hid-generic\nHID_ID=0003:00001E7D:00002F09\nHID_NAME=ROCCAT ROCCAT Vulcan II Mini\nHID_UNIQ=\n";
        assert_eq!(
            uevent_value(uevent, "HID_ID").and_then(parse_hid_id),
            Some((0x0003, 0x1E7D, 0x2F09))
        );
        assert_eq!(uevent_value(uevent, "HID_UNIQ"), Some(""));
        assert_eq!(uevent_value(uevent, "HID"), None);
    }
}
