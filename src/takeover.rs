//! Get exclusive access to the keyboard: close other RGB tools and hand the
//! device over between the tray daemon and one-shot CLI commands.

use crate::error::{Error, Result};
use std::mem::size_of;
use std::ptr::null;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

pub const TRAY_CLASS: &str = "VulcanControlTray";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Tools that open the same HID collections and fight over the LEDs.
/// Exact names only: substrings would hit unrelated apps (e.g. Unreal's
/// `SwarmAgent.exe`).
const CONFLICTING: [&str; 5] = [
    "openrgb.exe",
    "swarm ii.exe",
    "swarm2.exe",
    "roccat_swarm.exe",
    "roccat_swarm_monitor.exe",
];

pub fn is_conflicting(exe: &str) -> bool {
    CONFLICTING
        .iter()
        .any(|name| exe.eq_ignore_ascii_case(name))
}

/// Close SWARM II / OpenRGB if running. Returns the closed executables.
pub fn close_conflicting_apps() -> Vec<String> {
    let mut closed = Vec::new();
    let me = unsafe { GetCurrentProcessId() };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return closed;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while ok {
        let end = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if entry.th32ProcessID != me && is_conflicting(&exe) && terminate(entry.th32ProcessID) {
            closed.push(exe);
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    closed
}

fn terminate(pid: u32) -> bool {
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return false;
        }
        let ok = TerminateProcess(process, 0) != 0
            && WaitForSingleObject(process, 3000) == WAIT_OBJECT_0;
        CloseHandle(process);
        ok
    }
}

/// Ask a running `vulcand` to exit and wait until it has released the device.
/// Returns false when no daemon was running.
pub fn stop_daemon(timeout: Duration) -> Result<bool> {
    let class = wide(TRAY_CLASS);
    let hwnd = unsafe { FindWindowW(class.as_ptr(), null()) };
    if hwnd.is_null() {
        return Ok(false);
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    if process.is_null() {
        // Can't wait on it; give the graceful restore a moment.
        std::thread::sleep(Duration::from_millis(1500));
        return Ok(true);
    }
    let waited = unsafe { WaitForSingleObject(process, timeout.as_millis() as u32) };
    unsafe { CloseHandle(process) };
    if waited != WAIT_OBJECT_0 {
        return Err(Error::new("vulcand did not exit in time"));
    }
    Ok(true)
}

/// Start `vulcand.exe` from next to the current executable.
pub fn start_daemon() -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| Error::new(format!("current_exe: {e}")))?
        .with_file_name("vulcand.exe");
    std::process::Command::new(&exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| Error::new(format!("start {}: {e}", exe.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicting_tool_names() {
        assert!(is_conflicting("Swarm II.exe"));
        assert!(is_conflicting("ROCCAT_Swarm.exe"));
        assert!(!is_conflicting("SwarmAgent.exe"));
        assert!(!is_conflicting("SwarmCoordinator.exe"));
        assert!(is_conflicting("OpenRGB.exe"));
        assert!(!is_conflicting("vulcand.exe"));
        assert!(!is_conflicting("explorer.exe"));
    }
}
