//! Get exclusive access to the keyboard: close other RGB tools and hand the
//! device over between the tray daemon and one-shot CLI commands.
//!
//! Per OS: `DaemonLock` (one `vulcand` per user), `stop_daemon`, and
//! `close_conflicting_apps`.

use crate::error::{Error, Result};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{close_conflicting_apps, stop_daemon, DaemonLock};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{close_conflicting_apps, stop_daemon, DaemonLock, TRAY_CLASS};

/// Tools that open the same HID collections and fight over the LEDs.
/// Exact names only: substrings would hit unrelated apps (e.g. Unreal's
/// `SwarmAgent.exe`). Linux process names carry no extension.
const CONFLICTING: [&str; 6] = [
    "openrgb.exe",
    "swarm ii.exe",
    "swarm2.exe",
    "roccat_swarm.exe",
    "roccat_swarm_monitor.exe",
    "openrgb",
];

pub fn is_conflicting(exe: &str) -> bool {
    CONFLICTING
        .iter()
        .any(|name| exe.eq_ignore_ascii_case(name))
}

/// Start `vulcand` from next to the current executable.
pub fn start_daemon() -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| Error::new(format!("current_exe: {e}")))?
        .with_file_name(format!("vulcand{}", std::env::consts::EXE_SUFFIX));
    let mut command = std::process::Command::new(&exe);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        // Outlive this CLI: no terminal I/O, and no Ctrl+C from its job.
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
    }
    command
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
        assert!(is_conflicting("openrgb"));
        assert!(!is_conflicting("openrgb-helper"));
        assert!(!is_conflicting("vulcand.exe"));
        assert!(!is_conflicting("explorer.exe"));
    }
}
