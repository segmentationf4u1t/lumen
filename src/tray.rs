//! Tray UI: a Win32 notification-area icon on Windows, a StatusNotifierItem
//! (Waybar, Quickshell, KDE, ...) on Linux. Labels and menu actions are
//! shared so both menus behave and log the same.
//!
//! `run(Arc<Shared>) -> Result<()>` blocks until `Shared::shutdown`.

use crate::autostart;
use crate::effects::Effect;
use crate::runtime::Shared;
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::run;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{close, run};

// The Linux control socket (`crate::control`) drives the same actions.
#[cfg(target_os = "linux")]
pub(crate) use linux::open_path;

const BRIGHTNESS_STEPS: [u32; 5] = [100, 75, 50, 25, 10];
const MAX_PROFILES: usize = 800;

pub(crate) fn effect_label(effect: Effect) -> &'static str {
    match effect {
        Effect::Static => "Static",
        Effect::Breathe => "Breathe",
        Effect::Wave => "Wave",
        Effect::Spectrum => "Spectrum",
        Effect::Plasma => "Plasma",
        Effect::Comet => "Comet",
        Effect::Aurora => "Aurora",
        Effect::Scanner => "Scanner",
        Effect::Fire => "Fire",
        Effect::Showcase => "Showcase (cycles all)",
    }
}

pub(crate) fn status_label(shared: &Shared) -> String {
    if !shared.is_enabled() {
        return "Paused".to_owned();
    }
    let status = shared.status();
    if status == "connected" {
        "Connected".to_owned()
    } else if status == "starting" {
        "Starting…".to_owned()
    } else if status.starts_with("connect failed") {
        "Keyboard not found — retrying".to_owned()
    } else if status.starts_with("write failed") {
        "Connection lost — retrying".to_owned()
    } else if status.starts_with("direct mode failed") {
        "Keyboard busy — retrying".to_owned()
    } else if status.starts_with("config error") {
        "Config error — see vulcand.log".to_owned()
    } else {
        status.chars().take(60).collect()
    }
}

// ---------------------------------------------------------------------------
// Menu actions

fn toggle_pause(shared: &Shared) {
    set_paused(shared, shared.is_enabled());
}

pub(crate) fn set_paused(shared: &Shared, paused: bool) {
    if shared.is_enabled() != paused {
        return;
    }
    shared.set_enabled(!paused);
    shared.log(if paused { "paused" } else { "resumed" });
}

fn toggle_autostart(shared: &Shared) {
    set_autostart(shared, !autostart::is_enabled());
}

pub(crate) fn set_autostart(shared: &Shared, enable: bool) {
    match autostart::set_enabled(enable) {
        Ok(()) => shared.log(&format!("autostart {}", if enable { "on" } else { "off" })),
        Err(e) => shared.log(&format!("autostart failed: {e}")),
    }
}

pub(crate) fn reload(shared: &Arc<Shared>) {
    match crate::daemon::reload_config(shared) {
        Ok(()) => {
            if shared.status().starts_with("config error") {
                shared.set_status("connected");
            }
        }
        Err(e) => {
            shared.log(&format!("reload failed: {e}"));
            shared.set_status(format!("config error: {e}"));
        }
    }
}

pub(crate) fn choose_profile(shared: &Shared, name: &str) {
    if shared.select_profile(name) {
        shared.log(&format!("profile: {name}"));
    }
}

/// `None`: use the profile's own animation.
pub(crate) fn choose_effect(shared: &Shared, effect: Option<Effect>) {
    shared.select_effect(effect);
    match effect {
        Some(effect) => shared.log(&format!("animation: {}", effect.as_str())),
        None => shared.log("animation: from profile"),
    }
}

/// `None`: use the profile's own brightness.
fn choose_brightness(shared: &Shared, percent: Option<u32>) {
    shared.select_brightness(percent.map(|pct| pct as f32 / 100.0));
    match percent {
        Some(pct) => shared.log(&format!("brightness: {pct}%")),
        None => shared.log("brightness: from profile"),
    }
}
