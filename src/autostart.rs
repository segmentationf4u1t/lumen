//! Start `vulcand` at login for the current user, without elevation:
//! the per-user `Run` registry key on Windows, a systemd user service on
//! Linux. API: `set_enabled(bool) -> Result<()>`, `is_enabled() -> bool`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{is_enabled, set_enabled};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{is_enabled, set_enabled};

/// `vulcand` next to the running executable, or the running executable
/// itself (when that is `vulcand`, e.g. the tray toggle).
fn daemon_exe() -> crate::error::Result<std::path::PathBuf> {
    let current = std::env::current_exe()
        .map_err(|e| crate::error::Error::new(format!("current_exe: {e}")))?;
    let sibling = current
        .parent()
        .map(|p| p.join(format!("vulcand{}", std::env::consts::EXE_SUFFIX)))
        .filter(|p| p.exists());
    Ok(sibling.unwrap_or(current))
}
