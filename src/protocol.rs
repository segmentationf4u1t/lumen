//! Vulcan II Mini lighting protocol.
//!
//! Facts used here were established by probing the device and from public
//! reverse-engineering notes. This is an independent implementation; it does
//! not vendor or translate another project's source.
//!
//! All writes are lighting reports. No firmware/DFU command exists in this file.

use crate::error::{Error, Result};
use crate::hid::{HidHandle, PRODUCT_ID, USAGE_PAGE_CONTROL, USAGE_PAGE_LED, VENDOR_ID};
use std::thread::sleep;
use std::time::Duration;

pub const LED_COUNT: usize = 69;
pub const PACKET_COUNT: usize = 6;
pub const PACKET_LEN: usize = 65;
pub const PAYLOAD_LEN: usize = 345;
pub const DATA_PER_PACKET: usize = 60;
pub const STATIC_REPORT_LEN: usize = 332;
pub const KEEPALIVE_LEN: usize = 123;

pub const MODE_STATIC: u8 = 0x01;
pub const MODE_DIRECT: u8 = 0x0B;
pub const SPEED_MIN: u8 = 1;
pub const SPEED_MAX: u8 = 11;
pub const BRIGHTNESS_MIN: u8 = 1;
pub const BRIGHTNESS_MAX: u8 = 0x45; // 69

/// Logical key index -> hardware LED slot.
///
/// Slots are not contiguous. This table is hardware topology data; the
/// `sweep` command exists so it can be verified key by key on the real board.
pub const SLOT_MAP: [u8; LED_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 42, 43, 45, 46, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 64, 65, 66, 68, 70, 71, 72, 73, 74, 75, 76,
];

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub product: String,
    pub path: String,
    pub version_code: u8,
    pub layout: u8,
}

impl DeviceInfo {
    pub fn version(&self) -> String {
        format!("{}.{:02}", self.version_code / 100, self.version_code % 100)
    }
}

pub struct VulcanDevice {
    control: HidHandle,
    led: HidHandle,
    info: DeviceInfo,
}

impl VulcanDevice {
    pub fn open() -> Result<Self> {
        Self::open_with_hint(None)
    }

    pub fn open_with_hint(path_hint: Option<&str>) -> Result<Self> {
        let control =
            HidHandle::open_usage_with_hint(VENDOR_ID, PRODUCT_ID, USAGE_PAGE_CONTROL, path_hint)?;
        let led =
            HidHandle::open_usage_with_hint(VENDOR_ID, PRODUCT_ID, USAGE_PAGE_LED, path_hint)?;
        if led.output_len() < 64 {
            return Err(Error::new(format!(
                "LED output report is {} bytes, expected at least 64",
                led.output_len()
            )));
        }

        let mut info = DeviceInfo {
            product: control.product().to_owned(),
            path: control.path().to_owned(),
            version_code: 0,
            layout: 0,
        };

        // Report 0x09 is the identity report. Read it twice: some firmware
        // revisions return a stale first response after the interface opens.
        let mut query = [0u8; 9];
        query[0] = 0x09;
        control.get_feature(&mut query)?;
        query[0] = 0x09;
        control.get_feature(&mut query)?;
        info.version_code = query[2];
        info.layout = query[6];

        Ok(Self { control, led, info })
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn control_path(&self) -> &str {
        self.control.path()
    }

    pub fn led_path(&self) -> &str {
        self.led.path()
    }

    pub fn enable_direct(&self, enabled: bool) -> Result<()> {
        let report = [0x0E, 0x05, u8::from(enabled), 0x00, 0x00];
        self.control.set_feature(&report)
    }

    /// Enter streaming mode. Some firmware revisions need both the mode report
    /// (`0x11` with mode 0x0B) and the direct switch (`0x0E 05 01`); sending
    /// only one can leave the firmware effect in control while HID writes still
    /// return success.
    pub fn begin_direct(&self) -> Result<()> {
        let black = [[0u8; 3]; LED_COUNT];
        let report = build_mode_report(&black, MODE_DIRECT, 6, BRIGHTNESS_MAX);
        self.control.set_feature(&report)?;
        // Give the mode switch time to settle before the first stream packet.
        let _ = self.wait_ready();
        self.enable_direct(true)?;
        let _ = self.wait_ready();
        Ok(())
    }

    pub fn wait_ready(&self) -> Result<()> {
        // Firmware 1.26 on the Mini accepts a 4-byte buffer for report 0x04
        // (3 bytes is rejected with ERROR_INVALID_PARAMETER). Other revisions
        // may require the full feature report length, so fall back to it.
        if let Ok(()) = self.try_wait_ready(4) {
            return Ok(());
        }
        let fallback = (self.control.feature_len() as usize).max(4);
        self.try_wait_ready(fallback)
    }

    fn try_wait_ready(&self, length: usize) -> Result<()> {
        let mut buf = vec![0u8; length];
        buf[0] = 0x04;
        for attempt in 0..100 {
            if attempt > 0 {
                sleep(Duration::from_millis(25));
            }
            self.control.get_feature(&mut buf)?;
            if buf.len() > 1 && buf[1] == 1 {
                return Ok(());
            }
        }
        Err(Error::new(format!(
            "device did not report ready after 100 tries ({} byte buffer)",
            length
        )))
    }

    pub fn send_keepalive(&self) -> Result<()> {
        self.control.set_feature(&build_keepalive())
    }

    pub fn send_frame(&self, colors: &[[u8; 3]; LED_COUNT]) -> Result<()> {
        let mut packets = [[0u8; PACKET_LEN]; PACKET_COUNT];
        build_frames(colors, &mut packets);
        let without_report_id = self.led.output_len() == 64;
        for packet in &packets {
            if without_report_id {
                // No report IDs on this collection: Windows expects the
                // 64-byte payload without the leading 0x00 report ID.
                self.led.write_output(&packet[1..])?;
            } else {
                self.led.write_output(packet)?;
            }
        }
        Ok(())
    }

    /// Static mode is stored by the keyboard and survives this process.
    /// The Mini needs the keepalive handshake for the report to commit.
    pub fn send_static(
        &self,
        colors: &[[u8; 3]; LED_COUNT],
        speed: u8,
        brightness: u8,
    ) -> Result<()> {
        let report = build_mode_report(colors, MODE_STATIC, speed, brightness);
        self.control.set_feature(&report)?;
        // Readiness is advisory. Do not abort before the keepalives: some
        // firmware revisions reject the 0x04 probe but still commit 0x11.
        let _ = self.wait_ready();
        for _ in 0..3 {
            self.send_keepalive()?;
            sleep(Duration::from_millis(25));
        }
        Ok(())
    }

    /// Leave the keyboard in a deterministic, host-free state.
    pub fn restore(&self, colors: &[[u8; 3]; LED_COUNT], speed: u8, brightness: u8) -> Result<()> {
        // Ignore a failed direct-off; try the static report regardless.
        let _ = self.enable_direct(false);
        self.send_static(colors, speed, brightness)
    }
}

pub fn build_payload(colors: &[[u8; 3]; LED_COUNT]) -> [u8; PAYLOAD_LEN] {
    let mut payload = [0u8; PAYLOAD_LEN];
    for (logical, rgb) in colors.iter().enumerate() {
        let slot = SLOT_MAP[logical] as usize;
        let offset = slot * 3;
        // SLOT_MAP is compile-time fixed and max is 76, so offset + 2 <= 230.
        payload[offset] = rgb[0];
        payload[offset + 1] = rgb[1];
        payload[offset + 2] = rgb[2];
    }
    payload
}

pub fn build_frames(colors: &[[u8; 3]; LED_COUNT], out: &mut [[u8; PACKET_LEN]; PACKET_COUNT]) {
    let payload = build_payload(colors);
    for (p, packet) in out.iter_mut().enumerate() {
        packet.fill(0);
        packet[0] = 0x00; // HID report ID
        packet[1] = 0xA1;
        packet[2] = (p + 1) as u8;
        if p == 0 {
            packet[3] = (PAYLOAD_LEN & 0xFF) as u8;
            packet[4] = (PAYLOAD_LEN >> 8) as u8;
        }
        let start = p * DATA_PER_PACKET;
        let end = (start + DATA_PER_PACKET).min(PAYLOAD_LEN);
        packet[5..5 + (end - start)].copy_from_slice(&payload[start..end]);
    }
}

pub fn build_static_report(
    colors: &[[u8; 3]; LED_COUNT],
    speed: u8,
    brightness: u8,
) -> [u8; STATIC_REPORT_LEN] {
    build_mode_report(colors, MODE_STATIC, speed, brightness)
}

pub fn build_mode_report(
    colors: &[[u8; 3]; LED_COUNT],
    mode: u8,
    speed: u8,
    brightness: u8,
) -> [u8; STATIC_REPORT_LEN] {
    let speed = speed.clamp(SPEED_MIN, SPEED_MAX);
    let brightness = brightness.clamp(BRIGHTNESS_MIN, BRIGHTNESS_MAX);

    let mut report = [0u8; STATIC_REPORT_LEN];
    report[0] = 0x11;
    report[1] = (STATIC_REPORT_LEN & 0xFF) as u8;
    report[2] = (STATIC_REPORT_LEN >> 8) as u8;
    // Mini-specific header: 0x02 in the mode-class byte and 0xFB in byte 7.
    report[3] = 0x02;
    report[4] = mode;
    report[5] = speed;
    report[6] = brightness;
    report[7] = 0xFB;

    for (logical, rgb) in colors.iter().enumerate() {
        let slot = SLOT_MAP[logical] as usize;
        let offset = 9 + slot * 3;
        report[offset] = rgb[0];
        report[offset + 1] = rgb[1];
        report[offset + 2] = rgb[2];
    }

    let sum: u32 = report[..STATIC_REPORT_LEN - 2]
        .iter()
        .map(|&b| b as u32)
        .sum();
    report[STATIC_REPORT_LEN - 2] = (sum & 0xFF) as u8;
    report[STATIC_REPORT_LEN - 1] = (sum >> 8) as u8;
    report
}

pub fn build_keepalive() -> [u8; KEEPALIVE_LEN] {
    let mut report = [0u8; KEEPALIVE_LEN];
    report[0] = 0x13;
    report[1] = 0x7B;
    report[2] = 0x1E;
    report[3 + 4 * 4] = 0xA4;
    report[3 + 4 * 5] = 0xA5;
    report[3 + 4 * 16] = 0xB0;
    report[3 + 4 * 22] = 0xB6;
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(c: [u8; 3]) -> [[u8; 3]; LED_COUNT] {
        [c; LED_COUNT]
    }

    #[test]
    fn slot_map_is_complete_and_fits_payload() {
        let mut seen = [false; 128];
        for slot in SLOT_MAP {
            assert!(!seen[slot as usize], "duplicate slot {slot}");
            seen[slot as usize] = true;
            assert!((slot as usize) * 3 + 2 < PAYLOAD_LEN);
        }
    }

    #[test]
    fn payload_places_rgb_at_slot() {
        let mut colors = [[0u8; 3]; LED_COUNT];
        colors[0] = [1, 2, 3];
        colors[68] = [4, 5, 6];
        let p = build_payload(&colors);
        assert_eq!(&p[0..3], &[1, 2, 3]);
        let last = SLOT_MAP[68] as usize * 3;
        assert_eq!(&p[last..last + 3], &[4, 5, 6]);
    }

    #[test]
    fn frame_output_truncation_for_64_byte_reports() {
        let mut packets = [[0u8; PACKET_LEN]; PACKET_COUNT];
        build_frames(&solid([9, 8, 7]), &mut packets);
        let full = &packets[0];
        let no_id = &packets[0][1..];
        assert_eq!(full.len(), 65);
        assert_eq!(no_id.len(), 64);
        assert_eq!(no_id[0], 0xA1);
        assert_eq!(no_id[1], 1);
    }

    #[test]
    fn frames_have_expected_headers_and_lengths() {
        let mut packets = [[0u8; PACKET_LEN]; PACKET_COUNT];
        build_frames(&solid([9, 8, 7]), &mut packets);
        for (i, p) in packets.iter().enumerate() {
            assert_eq!(p[0], 0x00);
            assert_eq!(p[1], 0xA1);
            assert_eq!(p[2], i as u8 + 1);
            if i == 0 {
                assert_eq!(p[3], (PAYLOAD_LEN & 0xFF) as u8);
                assert_eq!(p[4], (PAYLOAD_LEN >> 8) as u8);
            } else {
                assert_eq!(p[3], 0);
                assert_eq!(p[4], 0);
            }
        }
    }

    #[test]
    fn direct_mode_report_header() {
        let report = build_mode_report(&solid([0, 0, 0]), MODE_DIRECT, 6, BRIGHTNESS_MAX);
        assert_eq!(report[0], 0x11);
        assert_eq!(report[4], MODE_DIRECT);
        assert_eq!(report[7], 0xFB);
    }

    #[test]
    fn static_report_checksum_and_bounds() {
        let report = build_static_report(&solid([0xAA, 0xBB, 0xCC]), 6, 69);
        assert_eq!(report[0], 0x11);
        assert_eq!(report[1], 0x4C);
        assert_eq!(report[2], 0x01);
        assert_eq!(report[4], MODE_STATIC);
        assert_eq!(report[7], 0xFB);
        let sum: u32 = report[..STATIC_REPORT_LEN - 2]
            .iter()
            .map(|&b| b as u32)
            .sum();
        assert_eq!(report[STATIC_REPORT_LEN - 2], (sum & 0xFF) as u8);
        assert_eq!(report[STATIC_REPORT_LEN - 1], (sum >> 8) as u8);
    }

    #[test]
    fn static_clamps_zero() {
        let report = build_static_report(&solid([1, 2, 3]), 0, 0);
        assert_eq!(report[5], SPEED_MIN);
        assert_eq!(report[6], BRIGHTNESS_MIN);
    }

    #[test]
    fn keepalive_markers() {
        let k = build_keepalive();
        assert_eq!(&k[..3], &[0x13, 0x7B, 0x1E]);
        assert_eq!(k[3 + 4 * 4], 0xA4);
        assert_eq!(k[3 + 4 * 5], 0xA5);
        assert_eq!(k[3 + 4 * 16], 0xB0);
        assert_eq!(k[3 + 4 * 22], 0xB6);
    }
}
