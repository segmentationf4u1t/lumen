//! StatusNotifierItem tray over D-Bus (ksni). Shown by any SNI host: Waybar's
//! `tray` module, Quickshell, KDE, GNOME with the AppIndicator extension.

use super::{
    choose_brightness, choose_effect, choose_profile, effect_label, reload, status_label,
    toggle_autostart, toggle_pause, BRIGHTNESS_STEPS, MAX_PROFILES,
};
use crate::autostart;
use crate::effects::Effect;
use crate::error::{Error, Result};
use crate::runtime::Shared;
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{Icon, MenuItem, ToolTip};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(250);
/// Hosts scale the closest size; larger entries only bloat every icon update.
const MAX_ICON_SIZE: u32 = 64;

static ICONS: LazyLock<(Vec<Icon>, Vec<Icon>)> = LazyLock::new(|| {
    (
        decode_ico(include_bytes!("../../assets/lumen.ico")),
        decode_ico(include_bytes!("../../assets/lumen-paused.ico")),
    )
});

/// ICO entries as SNI pixmaps: ARGB32, network byte order.
fn decode_ico(bytes: &[u8]) -> Vec<Icon> {
    let Ok(dir) = ico::IconDir::read(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    dir.entries()
        .iter()
        .filter(|entry| entry.width() <= MAX_ICON_SIZE && entry.height() <= MAX_ICON_SIZE)
        .filter_map(|entry| entry.decode().ok())
        .map(|image| Icon {
            width: image.width() as i32,
            height: image.height() as i32,
            data: image
                .rgba_data()
                .chunks_exact(4)
                .flat_map(|px| [px[3], px[0], px[1], px[2]])
                .collect(),
        })
        .collect()
}

/// dbusmenu reads a single `_` as a mnemonic marker.
fn escape(label: &str) -> String {
    label.replace('_', "__")
}

struct LumenTray {
    shared: Arc<Shared>,
}

impl ksni::Tray for LumenTray {
    // Left click opens the menu, like on Windows.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "lumen".into()
    }

    fn title(&self) -> String {
        "lumen".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    // Middle click.
    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        toggle_pause(&self.shared);
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let (active, paused) = &*ICONS;
        if self.shared.is_enabled() {
            active.clone()
        } else {
            paused.clone()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let tip = self.shared.tip();
        let (title, description) = tip.split_once('\n').unwrap_or((&tip, ""));
        ToolTip {
            title: title.to_owned(),
            description: description.to_owned(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        build_menu(&self.shared)
    }
}

fn label(text: String) -> MenuItem<LumenTray> {
    StandardItem {
        label: text,
        enabled: false,
        ..Default::default()
    }
    .into()
}

fn action(text: &str, activate: impl Fn(&Arc<Shared>) + Send + 'static) -> MenuItem<LumenTray> {
    StandardItem {
        label: text.to_owned(),
        activate: Box::new(move |tray: &mut LumenTray| activate(&tray.shared)),
        ..Default::default()
    }
    .into()
}

fn radio(
    options: Vec<String>,
    selected: Option<usize>,
    select: impl Fn(&Arc<Shared>, usize) + Send + 'static,
) -> MenuItem<LumenTray> {
    RadioGroup {
        // Out of range: nothing checked.
        selected: selected.unwrap_or(usize::MAX),
        select: Box::new(move |tray: &mut LumenTray, i| select(&tray.shared, i)),
        options: options
            .into_iter()
            .map(|label| RadioItem {
                label,
                ..Default::default()
            })
            .collect(),
    }
    .into()
}

fn submenu(text: String, items: Vec<MenuItem<LumenTray>>) -> MenuItem<LumenTray> {
    SubMenu {
        label: text,
        submenu: items,
        ..Default::default()
    }
    .into()
}

fn checkmark(
    text: &str,
    checked: bool,
    activate: impl Fn(&Arc<Shared>) + Send + 'static,
) -> MenuItem<LumenTray> {
    CheckmarkItem {
        label: text.to_owned(),
        checked,
        activate: Box::new(move |tray: &mut LumenTray| activate(&tray.shared)),
        ..Default::default()
    }
    .into()
}

/// Same entries as the Windows menu. Rebuilt by ksni after every click and
/// every `Handle::update`.
fn build_menu(shared: &Shared) -> Vec<MenuItem<LumenTray>> {
    let cfg = shared.config();
    let sel = shared.selection();
    let active = shared.active();
    let profiles: Vec<String> = cfg.profiles.keys().take(MAX_PROFILES).cloned().collect();
    let base = sel
        .profile
        .clone()
        .unwrap_or_else(|| cfg.default_profile.clone());

    let mut menu = vec![
        label("lumen  ·  Vulcan II Mini".to_owned()),
        label(escape(&status_label(shared))),
    ];
    if let Some(process) = &active.app {
        menu.push(label(escape(&format!(
            "App rule: {process} → {}",
            active.profile_name
        ))));
    }
    menu.push(MenuItem::Separator);

    // Profile
    let selected = profiles.iter().position(|p| *p == base);
    let options = profiles.iter().map(|p| escape(p)).collect();
    menu.push(submenu(
        format!("Profile: {}", escape(&base)),
        vec![radio(options, selected, move |shared, i| {
            if let Some(name) = profiles.get(i) {
                choose_profile(shared, name);
            }
        })],
    ));

    // Animation: "From profile" first, then every effect.
    let base_effect = cfg.profile(&base).map(|p| p.effect).unwrap_or_default();
    let effects = Effect::all();
    let options = std::iter::once(format!("From profile ({})", effect_label(base_effect)))
        .chain(effects.iter().map(|e| effect_label(*e).to_owned()))
        .collect();
    let selected = match sel.effect {
        None => Some(0),
        Some(e) => effects.iter().position(|x| *x == e).map(|i| i + 1),
    };
    menu.push(submenu(
        format!("Animation: {}", effect_label(sel.effect.unwrap_or(base_effect))),
        vec![radio(options, selected, |shared, i| match i {
            0 => choose_effect(shared, None),
            i => {
                if let Some(effect) = Effect::all().get(i - 1) {
                    choose_effect(shared, Some(*effect));
                }
            }
        })],
    ));

    // Brightness: "From profile" first, then the fixed steps.
    let options = std::iter::once("From profile".to_owned())
        .chain(BRIGHTNESS_STEPS.iter().map(|pct| format!("{pct}%")))
        .collect();
    let selected = match sel.brightness {
        None => Some(0),
        Some(b) => {
            let pct = (b * 100.0).round() as u32;
            BRIGHTNESS_STEPS.iter().position(|s| *s == pct).map(|i| i + 1)
        }
    };
    menu.push(submenu(
        format!("Brightness: {}%", (active.brightness * 100.0).round()),
        vec![radio(options, selected, |shared, i| match i {
            0 => choose_brightness(shared, None),
            i => {
                if let Some(pct) = BRIGHTNESS_STEPS.get(i - 1) {
                    choose_brightness(shared, Some(*pct));
                }
            }
        })],
    ));

    menu.extend([
        MenuItem::Separator,
        checkmark("Pause lighting", !shared.is_enabled(), |shared| {
            toggle_pause(shared)
        }),
        checkmark("Start on login", autostart::is_enabled(), |shared| {
            toggle_autostart(shared)
        }),
        MenuItem::Separator,
        action("Edit config…", |shared| open_path(shared, shared.config_path())),
        action("Open data folder", |shared| {
            if let Some(dir) = shared.config_path().parent() {
                open_path(shared, dir);
            }
        }),
        action("Reload config", reload),
        MenuItem::Separator,
        action("Exit lumen", |shared| shared.shutdown()),
    ]);
    menu
}

/// Variables a desktop launcher needs. A daemon started by systemd before
/// the compositor lacks them; compositors export them to the systemd user
/// manager (`dbus-update-activation-environment --systemd`) once running.
const SESSION_VARS: [&str; 4] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
];

fn session_env() -> Vec<(String, String)> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
        return Vec::new();
    }
    let Ok(output) = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('='))
        // `$'...'` values are C-quoted; none of the wanted ones need that.
        .filter(|(key, value)| SESSION_VARS.contains(key) && !value.starts_with("$'"))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// Open a file or folder with the desktop's default application.
pub(crate) fn open_path(shared: &Arc<Shared>, target: &Path) {
    let spawned = Command::new("xdg-open")
        .arg(target)
        .envs(session_env())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        // xdg-open returns once the handler is launched; reap it off the
        // D-Bus thread.
        Ok(mut child) => {
            let shared = Arc::clone(shared);
            let target = target.to_owned();
            thread::spawn(move || match child.wait() {
                Ok(status) if status.success() => {}
                Ok(status) => shared.log(&format!(
                    "xdg-open {} failed: {status}",
                    target.display()
                )),
                Err(e) => shared.log(&format!("xdg-open {}: {e}", target.display())),
            });
        }
        Err(e) => shared.log(&format!("xdg-open {}: {e}", target.display())),
    }
}

pub fn run(shared: Arc<Shared>) -> Result<()> {
    // The host may start after us (autostart before the bar); ksni then
    // registers as soon as a StatusNotifierWatcher appears.
    let handle = LumenTray {
        shared: Arc::clone(&shared),
    }
    .assume_sni_available(true)
    .spawn()
    .map_err(|e| Error::new(format!("tray: {e}")))?;

    // Icon, tooltip and menu follow `Shared`; push them when it changes.
    let mut shown = shared.generation();
    while !shared.is_shutdown() {
        thread::sleep(POLL);
        // Read before refreshing: a change racing the refresh re-triggers.
        let generation = shared.generation();
        if generation != shown {
            shown = generation;
            handle.update(|_| {});
        }
    }
    handle.shutdown().wait();
    Ok(())
}
