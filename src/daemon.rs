use crate::apps::{foreground_process_name, match_rule};
use crate::config::{default_config_path, Config};
use crate::effects;
use crate::error::{Error, Result};
use crate::protocol::{VulcanDevice, BRIGHTNESS_MAX, LED_COUNT, SPEED_MAX};
use crate::runtime::Shared;
use crate::tray;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

static CTRL_SHUTDOWN: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl_handler(_event: u32) -> windows_sys::core::BOOL {
    CTRL_SHUTDOWN.store(true, Ordering::SeqCst);
    1
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct SingleInstance(HANDLE);

impl SingleInstance {
    fn acquire() -> Result<Option<Self>> {
        let name = wide(r"Local\VulcanControlDaemon");
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(Error::last_os("CreateMutexW"));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn run() -> Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    let mut config_path = default_config_path();
    let mut with_tray = true;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::new("--config requires a path"))?;
                config_path = PathBuf::from(value);
            }
            "--no-tray" => with_tray = false,
            "--help" | "-h" => {
                println!(
                    "vulcand — Vulcan II Mini lighting daemon\n\
                     options: --config PATH  --no-tray"
                );
                return Ok(());
            }
            other => return Err(Error::new(format!("unknown argument {other:?}"))),
        }
    }

    // Crisp tray icon and menus on scaled displays. Must precede any window.
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }

    let Some(_instance) = SingleInstance::acquire()? else {
        eprintln!("vulcand is already running");
        return Ok(());
    };

    let (config, startup_error) = match Config::load_or_create(&config_path) {
        Ok(cfg) => (cfg, None),
        Err(e) => {
            // Corrupt config must not be overwritten. Run with safe defaults
            // and leave the file intact for the user to fix.
            eprintln!("config error: {e}; using built-in defaults");
            (Config::default(), Some(e))
        }
    };

    let shared = Shared::new(config, config_path);
    shared.log("vulcand starting");
    if let Some(e) = startup_error {
        shared.log(&format!("config error: {e}; using built-in defaults"));
        shared.set_status(format!("config error: {e}"));
    }

    let _ = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), 1) };

    let tray_handle = if with_tray {
        let s = Arc::clone(&shared);
        Some(
            thread::Builder::new()
                .name("tray".to_owned())
                .spawn(move || {
                    let result =
                        std::panic::catch_unwind(AssertUnwindSafe(|| tray::run(Arc::clone(&s))));
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            s.log(&format!("tray error: {e}"));
                            s.shutdown();
                        }
                        Err(_) => {
                            s.log("tray panicked");
                            s.shutdown();
                        }
                    }
                })
                .map_err(|e| Error::new(format!("spawn tray: {e}")))?,
        )
    } else {
        None
    };

    let watcher = {
        let s = Arc::clone(&shared);
        thread::Builder::new()
            .name("app-watcher".to_owned())
            .spawn(move || watch_loop(s))
            .map_err(|e| Error::new(format!("spawn watcher: {e}")))?
    };

    let render_result = std::panic::catch_unwind(AssertUnwindSafe(|| render_loop(&shared)));
    if render_result.is_err() {
        shared.log("render loop panicked; attempting restore");
        if let Ok(dev) = VulcanDevice::open_with_hint(shared.config().device.path.as_deref()) {
            restore_current(&shared, &dev);
        }
    }

    shared.shutdown();
    let hwnd = shared.tray_hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe { PostMessageW(hwnd as _, WM_CLOSE, 0, 0) };
    }
    let _ = tray_handle.map(|h| h.join());
    let _ = watcher.join();
    shared.log("vulcand stopped");
    Ok(())
}

fn render_loop(shared: &Arc<Shared>) {
    let mut device: Option<VulcanDevice> = None;
    let mut backoff = Duration::from_millis(250);
    let mut next_connect = Instant::now();
    let mut next_reload = Instant::now() + Duration::from_secs(1);
    let mut last_mtime = mtime(shared.config_path());
    let started = Instant::now();
    let mut colors = [[0u8; 3]; LED_COUNT];
    let mut next_frame = Instant::now();
    let mut next_direct_check = Instant::now();

    while !shared.is_shutdown() && !CTRL_SHUTDOWN.load(Ordering::SeqCst) {
        let now = Instant::now();

        if now >= next_reload {
            reload_if_changed(shared, &mut last_mtime);
            next_reload = now + Duration::from_secs(1);
        }

        if !shared.is_enabled() {
            if let Some(dev) = device.take() {
                restore_current(shared, &dev);
                shared.log("paused; direct mode released");
            }
            thread::sleep(Duration::from_millis(200));
            continue;
        }

        if device.is_none() {
            if now < next_connect {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            let cfg = shared.config();
            let hint = cfg.device.path.as_deref();
            match VulcanDevice::open_with_hint(hint) {
                Ok(dev) => match dev.begin_direct() {
                    Ok(()) => {
                        shared.log(&format!(
                            "connected, direct armed: fw {} layout {} led {}",
                            dev.info().version(),
                            dev.info().layout,
                            dev.led_path()
                        ));
                        shared.set_status("connected");
                        device = Some(dev);
                        backoff = Duration::from_millis(250);
                        next_frame = Instant::now();
                    }
                    Err(e) => {
                        shared.log(&format!("direct mode failed: {e}"));
                        shared.set_status(format!("direct mode failed: {e}"));
                        next_connect = now + backoff;
                        backoff = (backoff * 2).min(Duration::from_secs(5));
                    }
                },
                Err(e) => {
                    for exe in crate::takeover::close_conflicting_apps() {
                        shared.log(&format!("closed {exe} (it holds the keyboard)"));
                    }
                    let msg = format!("connect failed: {e}");
                    if shared.status() != msg {
                        shared.log(&msg);
                        shared.set_status(msg);
                    }
                    next_connect = now + backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
        }

        if now >= next_direct_check {
            next_direct_check = now + Duration::from_secs(1);
            if let Some(dev) = device.as_ref() {
                if let Ok(false) = dev.direct_enabled() {
                    shared.log("keyboard left direct mode; re-arming");
                    if let Err(e) = dev.begin_direct() {
                        shared.log(&format!("re-arm failed: {e}"));
                        device = None;
                        continue;
                    }
                }
            }
        }

        let cfg = shared.config();
        let active = shared.active();
        effects::render_with(
            &active.profile,
            active.effect,
            active.brightness,
            started.elapsed().as_secs_f32(),
            &mut colors,
        );

        if let Some(dev) = device.as_ref() {
            if let Err(e) = dev.send_frame(&colors) {
                shared.log(&format!("frame write failed: {e}"));
                shared.set_status(format!("write failed: {e}"));
                device = None;
                next_connect = Instant::now() + backoff;
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
        }

        let fps = cfg.settings.fps.clamp(10, 60);
        let frame_time = Duration::from_secs_f64(1.0 / fps as f64);
        next_frame += frame_time;
        let now = Instant::now();
        if next_frame > now {
            thread::sleep(next_frame - now);
        } else if now.duration_since(next_frame) > frame_time * 2 {
            // Recover from sleep/resume or a slow frame without a burst.
            next_frame = now + frame_time;
        }
    }

    if let Some(dev) = device.take() {
        restore_current(shared, &dev);
    }
}

/// Leave the keyboard showing a static snapshot of the current selection.
fn restore_current(shared: &Arc<Shared>, dev: &VulcanDevice) {
    let active = shared.active();
    let profile = &active.profile;
    let mut colors = [[0u8; 3]; LED_COUNT];
    effects::render_with(profile, active.effect, 1.0, 0.0, &mut colors);
    let speed = ((profile.speed / 5.0) * (SPEED_MAX as f32 - 1.0) + 1.0)
        .round()
        .clamp(1.0, SPEED_MAX as f32) as u8;
    let brightness = (active.brightness * BRIGHTNESS_MAX as f32)
        .round()
        .clamp(1.0, BRIGHTNESS_MAX as f32) as u8;
    if let Err(e) = dev.restore(&colors, speed, brightness) {
        shared.log(&format!("restore failed: {e}"));
    }
}

fn reload_if_changed(shared: &Arc<Shared>, last_mtime: &mut Option<SystemTime>) {
    let current = mtime(shared.config_path());
    if current == *last_mtime {
        return;
    }
    *last_mtime = current;
    match Config::load(shared.config_path()) {
        Ok(cfg) => {
            shared.replace_config(cfg);
            shared.log("config reloaded");
            if shared.status().starts_with("config error") {
                shared.set_status("connected");
            }
        }
        Err(e) => {
            shared.log(&format!("config reload failed: {e}"));
            shared.set_status(format!("config error: {e}"));
        }
    }
}

pub fn reload_config(shared: &Arc<Shared>) -> Result<()> {
    let cfg = Config::load(shared.config_path())?;
    shared.replace_config(cfg);
    shared.log("config reloaded (tray)");
    Ok(())
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn watch_loop(shared: Arc<Shared>) {
    let mut candidate: Option<String> = None;
    let mut candidate_since = Instant::now();

    while !shared.is_shutdown() && !CTRL_SHUTDOWN.load(Ordering::SeqCst) {
        let now = Instant::now();
        // `None` means our own tray menu or a window we can't inspect; keep
        // the current state instead of treating it as a focus change.
        if let Some(process) = foreground_process_name() {
            if candidate.as_deref() != Some(process.as_str()) {
                candidate = Some(process);
                candidate_since = now;
            }
        }

        let debounce = shared.config().settings.debounce_ms;
        if now.duration_since(candidate_since).as_millis() as u64 >= debounce {
            // Re-evaluated every tick, not only on focus changes, so edited,
            // added or removed rules apply right after a config reload.
            evaluate_app_rules(&shared, candidate.as_deref());
        }

        thread::sleep(Duration::from_millis(250));
    }
}

/// Apply the app rule for the focused `process`, if any. App rules only
/// overlay the tray choice; no match releases the overlay and never resets
/// to `default_profile`. Returns true when the overlay changed.
fn evaluate_app_rules(shared: &Shared, process: Option<&str>) -> bool {
    let cfg = shared.config();
    let matched = process.and_then(|process| {
        match_rule(&cfg.app_rules, process).map(|rule| (process.to_owned(), rule.profile.clone()))
    });
    let describe = matched
        .as_ref()
        .map(|(process, profile)| format!("app rule {process} -> {profile}"));
    let changed = shared.set_app_match(matched);
    if changed {
        shared.log(&describe.unwrap_or_else(|| "app rule released".to_owned()));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppRule;
    use crate::effects::{Effect, Profile};

    fn config(rules: &[(&str, &str)]) -> Config {
        let mut cfg = Config::default();
        cfg.profiles.insert(
            "fps".into(),
            Profile {
                effect: Effect::Static,
                ..Default::default()
            },
        );
        cfg.app_rules = rules
            .iter()
            .map(|(process, profile)| AppRule {
                process: (*process).into(),
                profile: (*profile).into(),
            })
            .collect();
        cfg
    }

    #[test]
    fn rule_changes_apply_on_reload_without_a_focus_change() {
        let dir = std::env::temp_dir().join(format!(
            "vulcan-daemon-rules-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let shared = Shared::new(config(&[]), dir.join("config.json"));

        // Focused on cs2.exe the whole time; only the config changes.
        assert!(!evaluate_app_rules(&shared, Some("cs2.exe")));
        assert_eq!(shared.active().profile_name, "default");

        shared.replace_config(config(&[("cs2.exe", "fps")]));
        assert!(evaluate_app_rules(&shared, Some("cs2.exe")));
        assert_eq!(shared.active().profile_name, "fps");

        shared.replace_config(config(&[]));
        assert!(evaluate_app_rules(&shared, Some("cs2.exe")));
        assert_eq!(shared.active().profile_name, "default");
        assert!(shared.active().app.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
