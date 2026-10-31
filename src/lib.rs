#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("lumen supports Windows and Linux only");

pub mod apps;
pub mod autostart;
pub mod color;
pub mod config;
#[cfg(target_os = "linux")]
pub mod control;
pub mod daemon;
pub mod effects;
pub mod error;
pub mod hid;
pub mod layout;
pub mod protocol;
pub mod runtime;
pub mod takeover;
pub mod tray;
