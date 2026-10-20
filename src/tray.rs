//! Raw Win32 notification-area UI: icon, tooltip, and a themed popup menu.

use crate::autostart;
use crate::effects::Effect;
use crate::error::{Error, Result};
use crate::runtime::Shared;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, LoadImageW,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    SetMenuDefaultItem, SetTimer, TrackPopupMenu, TranslateMessage, HICON, HMENU, IDI_APPLICATION,
    IMAGE_ICON, LR_DEFAULTCOLOR, MF_BYCOMMAND, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, SW_SHOWNORMAL, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NULL,
    WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
};

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
static ICONS: LazyLock<(isize, isize)> =
    LazyLock::new(|| (load_icon(ICON_ACTIVE), load_icon(ICON_PAUSED_ID)));
static ICON_ADDED: AtomicBool = AtomicBool::new(false);
static ICON_PAUSED: AtomicBool = AtomicBool::new(false);

const CLASS_NAME: &str = "VulcanControlTray";
const TRAY_ID: u32 = 1;
const TIMER_ID: usize = 1;
const CALLBACK_MSG: u32 = WM_APP + 1;
/// Resource IDs from `assets/lumen.rc`.
const ICON_ACTIVE: u16 = 1;
const ICON_PAUSED_ID: u16 = 2;

const CMD_PAUSE: u32 = 10;
const CMD_AUTOSTART: u32 = 11;
const CMD_EDIT_CONFIG: u32 = 12;
const CMD_OPEN_FOLDER: u32 = 13;
const CMD_RELOAD: u32 = 14;
const CMD_EXIT: u32 = 15;
const CMD_PROFILE: u32 = 100;
const CMD_EFFECT_FROM_PROFILE: u32 = 1000;
const CMD_EFFECT: u32 = 1001;
const CMD_BRIGHTNESS_FROM_PROFILE: u32 = 1100;
const CMD_BRIGHTNESS: u32 = 1101;
const BRIGHTNESS_STEPS: [u32; 5] = [100, 75, 50, 25, 10];
const MAX_PROFILES: usize = 800;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn effect_label(effect: Effect) -> &'static str {
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

fn status_label(shared: &Shared) -> String {
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
// Dark mode. Win32 popup menus only follow the system theme through
// undocumented uxtheme exports (by ordinal); this is what Explorer and
// Notepad++ use. Every call is optional: on builds without them the menu
// simply stays light.

type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, i32) -> i32;
type VoidFn = unsafe extern "system" fn();
type RawProc = unsafe extern "system" fn() -> isize;

struct DarkMode {
    refresh: Option<VoidFn>,
    flush: Option<VoidFn>,
}

static DARK: OnceLock<DarkMode> = OnceLock::new();

fn init_dark_mode(hwnd: HWND) {
    let lib = unsafe {
        LoadLibraryExW(
            wide("uxtheme.dll").as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if lib.is_null() {
        return;
    }
    let ordinal = |n: usize| unsafe { GetProcAddress(lib, n as *const u8) };
    unsafe {
        // 135: SetPreferredAppMode(AllowDark = 1) — follow the system setting.
        if let Some(f) = ordinal(135) {
            std::mem::transmute::<RawProc, SetPreferredAppMode>(f)(1);
        }
        // 133: AllowDarkModeForWindow(hwnd, TRUE).
        if let Some(f) = ordinal(133) {
            std::mem::transmute::<RawProc, AllowDarkModeForWindow>(f)(hwnd, 1);
        }
    }
    let dark = DarkMode {
        // 104: RefreshImmersiveColorPolicyState, 136: FlushMenuThemes.
        refresh: ordinal(104).map(|f| unsafe { std::mem::transmute::<RawProc, VoidFn>(f) }),
        flush: ordinal(136).map(|f| unsafe { std::mem::transmute::<RawProc, VoidFn>(f) }),
    };
    apply_theme(&dark);
    let _ = DARK.set(dark);
}

fn apply_theme(dark: &DarkMode) {
    unsafe {
        if let Some(refresh) = dark.refresh {
            refresh();
        }
        if let Some(flush) = dark.flush {
            flush();
        }
    }
}

// ---------------------------------------------------------------------------
// Icons

fn load_icon(id: u16) -> isize {
    unsafe {
        let dpi = GetDpiForSystem();
        let cx = GetSystemMetricsForDpi(SM_CXSMICON, dpi);
        let cy = GetSystemMetricsForDpi(SM_CYSMICON, dpi);
        let module = GetModuleHandleW(null());
        let icon = LoadImageW(
            module,
            id as usize as *const u16,
            IMAGE_ICON,
            cx,
            cy,
            LR_DEFAULTCOLOR,
        );
        if icon.is_null() {
            LoadIconW(null_mut(), IDI_APPLICATION) as isize
        } else {
            icon as isize
        }
    }
}

fn current_icon(paused: bool) -> HICON {
    let (active, paused_icon) = *ICONS;
    (if paused { paused_icon } else { active }) as HICON
}

fn notify_data(hwnd: HWND, shared: &Shared) -> NOTIFYICONDATAW {
    let paused = !shared.is_enabled();
    ICON_PAUSED.store(paused, Ordering::SeqCst);
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: CALLBACK_MSG,
        hIcon: current_icon(paused),
        ..Default::default()
    };
    let tip: Vec<u16> = shared
        .tip()
        .encode_utf16()
        .take(nid.szTip.len() - 1)
        .collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    nid
}

fn add_icon(hwnd: HWND, shared: &Shared) {
    let nid = notify_data(hwnd, shared);
    let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } != 0;
    ICON_ADDED.store(ok, Ordering::SeqCst);
    if !ok {
        shared.log("Shell_NotifyIconW(NIM_ADD) failed; will retry");
    }
}

fn refresh_icon(hwnd: HWND, shared: &Shared) {
    if !ICON_ADDED.load(Ordering::SeqCst) {
        add_icon(hwnd, shared);
        return;
    }
    let nid = notify_data(hwnd, shared);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn remove_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    ICON_ADDED.store(false, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Menu

/// Owned popup menu. Only the root is destroyed: `DestroyMenu` is recursive,
/// so submenus handed to `submenu` must not be destroyed separately.
struct Menu(HMENU);

impl Menu {
    fn new() -> Option<Self> {
        let menu = unsafe { CreatePopupMenu() };
        (!menu.is_null()).then_some(Self(menu))
    }

    fn item(&self, id: u32, text: &str, flags: u32) -> &Self {
        let text = wide(text);
        unsafe { AppendMenuW(self.0, MF_STRING | flags, id as usize, text.as_ptr()) };
        self
    }

    fn label(&self, text: &str) -> &Self {
        self.item(0, text, MF_GRAYED)
    }

    fn separator(&self) -> &Self {
        unsafe { AppendMenuW(self.0, MF_SEPARATOR, 0, null()) };
        self
    }

    fn submenu(&self, text: &str, child: Menu) -> &Self {
        let text = wide(text);
        let handle = child.0;
        std::mem::forget(child);
        unsafe { AppendMenuW(self.0, MF_POPUP | MF_STRING, handle as usize, text.as_ptr()) };
        self
    }

    fn radio(&self, first: u32, last: u32, checked: u32) -> &Self {
        unsafe { CheckMenuRadioItem(self.0, first, last, checked, MF_BYCOMMAND) };
        self
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        unsafe { DestroyMenu(self.0) };
    }
}

fn build_menu(shared: &Shared) -> Option<(Menu, Vec<String>)> {
    let cfg = shared.config();
    let sel = shared.selection();
    let active = shared.active();
    let profiles: Vec<String> = cfg.profiles.keys().take(MAX_PROFILES).cloned().collect();

    let root = Menu::new()?;
    root.label("lumen  ·  Vulcan II Mini");
    unsafe { SetMenuDefaultItem(root.0, 0, 1) }; // bold title row
    root.label(&status_label(shared));
    if let Some(process) = &active.app {
        root.label(&format!("App rule: {process} → {}", active.profile_name));
    }
    root.separator();

    // Profile
    let base = sel
        .profile
        .clone()
        .unwrap_or_else(|| cfg.default_profile.clone());
    let profile_menu = Menu::new()?;
    for (i, name) in profiles.iter().enumerate() {
        profile_menu.item(CMD_PROFILE + i as u32, name, 0);
    }
    if let Some(i) = profiles.iter().position(|p| *p == base) {
        let last = CMD_PROFILE + profiles.len().saturating_sub(1) as u32;
        profile_menu.radio(CMD_PROFILE, last, CMD_PROFILE + i as u32);
    }
    root.submenu(&format!("Profile\t{base}"), profile_menu);

    // Animation
    let base_effect = cfg.profile(&base).map(|p| p.effect).unwrap_or_default();
    let effects = Effect::all();
    let anim_menu = Menu::new()?;
    anim_menu.item(
        CMD_EFFECT_FROM_PROFILE,
        &format!("From profile ({})", effect_label(base_effect)),
        0,
    );
    anim_menu.separator();
    for (i, effect) in effects.iter().enumerate() {
        anim_menu.item(CMD_EFFECT + i as u32, effect_label(*effect), 0);
    }
    let last_effect = CMD_EFFECT + effects.len() as u32 - 1;
    let checked = sel
        .effect
        .and_then(|e| effects.iter().position(|x| *x == e))
        .map(|i| CMD_EFFECT + i as u32)
        .unwrap_or(CMD_EFFECT_FROM_PROFILE);
    anim_menu.radio(CMD_EFFECT_FROM_PROFILE, last_effect, checked);
    let shown = sel.effect.unwrap_or(base_effect);
    root.submenu(&format!("Animation\t{}", effect_label(shown)), anim_menu);

    // Brightness
    let bright_menu = Menu::new()?;
    bright_menu.item(CMD_BRIGHTNESS_FROM_PROFILE, "From profile", 0);
    bright_menu.separator();
    for (i, pct) in BRIGHTNESS_STEPS.iter().enumerate() {
        bright_menu.item(CMD_BRIGHTNESS + i as u32, &format!("{pct}%"), 0);
    }
    let last_bright = CMD_BRIGHTNESS + BRIGHTNESS_STEPS.len() as u32 - 1;
    let checked = sel
        .brightness
        .and_then(|b| {
            let pct = (b * 100.0).round() as u32;
            BRIGHTNESS_STEPS.iter().position(|s| *s == pct)
        })
        .map(|i| CMD_BRIGHTNESS + i as u32)
        .unwrap_or(CMD_BRIGHTNESS_FROM_PROFILE);
    bright_menu.radio(CMD_BRIGHTNESS_FROM_PROFILE, last_bright, checked);
    root.submenu(
        &format!("Brightness\t{}%", (active.brightness * 100.0).round()),
        bright_menu,
    );

    root.separator();
    root.item(
        CMD_PAUSE,
        "Pause lighting",
        if shared.is_enabled() { 0 } else { MF_CHECKED },
    );
    root.item(
        CMD_AUTOSTART,
        "Start with Windows",
        if autostart::is_enabled() {
            MF_CHECKED
        } else {
            0
        },
    );
    root.separator();
    root.item(CMD_EDIT_CONFIG, "Edit config…", 0);
    root.item(CMD_OPEN_FOLDER, "Open data folder", 0);
    root.item(CMD_RELOAD, "Reload config", 0);
    root.separator();
    root.item(CMD_EXIT, "Exit lumen", 0);
    Some((root, profiles))
}

fn shell_open(target: &std::path::Path) -> bool {
    let verb = wide("open");
    let file = wide(&target.to_string_lossy());
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    result as isize > 32
}

fn show_menu(hwnd: HWND) {
    let Some(shared) = SHARED.get() else { return };
    let Some((menu, profiles)) = build_menu(shared) else {
        return;
    };

    let mut pt = POINT { x: 0, y: 0 };
    let cmd = unsafe {
        GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu.0,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            null(),
        ) as u32;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        cmd
    };
    drop(menu);
    handle_command(hwnd, shared, cmd, &profiles);
}

fn handle_command(hwnd: HWND, shared: &Arc<Shared>, cmd: u32, profiles: &[String]) {
    match cmd {
        0 => return,
        CMD_PAUSE => {
            let enable = !shared.is_enabled();
            shared.set_enabled(enable);
            shared.log(if enable {
                "resumed via tray"
            } else {
                "paused via tray"
            });
        }
        CMD_AUTOSTART => {
            let enable = !autostart::is_enabled();
            match autostart::set_enabled(enable) {
                Ok(()) => shared.log(&format!("autostart {}", if enable { "on" } else { "off" })),
                Err(e) => shared.log(&format!("autostart failed: {e}")),
            }
        }
        CMD_EDIT_CONFIG => {
            let path = shared.config_path();
            if !shell_open(path) {
                let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
            }
        }
        CMD_OPEN_FOLDER => {
            if let Some(dir) = shared.config_path().parent() {
                shell_open(dir);
            }
        }
        CMD_RELOAD => match crate::daemon::reload_config(shared) {
            Ok(()) => {
                if shared.status().starts_with("config error") {
                    shared.set_status("connected");
                }
            }
            Err(e) => {
                shared.log(&format!("reload failed: {e}"));
                shared.set_status(format!("config error: {e}"));
            }
        },
        CMD_EXIT => {
            shared.shutdown();
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            return;
        }
        CMD_EFFECT_FROM_PROFILE => {
            shared.select_effect(None);
            shared.log("animation: from profile");
        }
        CMD_BRIGHTNESS_FROM_PROFILE => {
            shared.select_brightness(None);
            shared.log("brightness: from profile");
        }
        id if (CMD_EFFECT..CMD_BRIGHTNESS_FROM_PROFILE).contains(&id) => {
            if let Some(effect) = Effect::all().get((id - CMD_EFFECT) as usize) {
                shared.select_effect(Some(*effect));
                shared.log(&format!("animation: {}", effect.as_str()));
            }
        }
        id if id >= CMD_BRIGHTNESS => {
            if let Some(pct) = BRIGHTNESS_STEPS.get((id - CMD_BRIGHTNESS) as usize) {
                shared.select_brightness(Some(*pct as f32 / 100.0));
                shared.log(&format!("brightness: {pct}%"));
            }
        }
        id if (CMD_PROFILE..CMD_EFFECT_FROM_PROFILE).contains(&id) => {
            if let Some(name) = profiles.get((id - CMD_PROFILE) as usize) {
                if shared.select_profile(name) {
                    shared.log(&format!("profile: {name}"));
                }
            }
        }
        other => shared.log(&format!("unknown tray command {other}")),
    }
    refresh_icon(hwnd, shared);
}

// ---------------------------------------------------------------------------
// Window procedure

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(shared) = SHARED.get() else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };

    if msg == CALLBACK_MSG {
        match (lparam as u32) & 0xFFFF {
            WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
            _ => {}
        }
        return 0;
    }

    if TASKBAR_CREATED.get().is_some_and(|&m| m == msg) {
        // Explorer restarted (or the taskbar DPI changed): re-add the icon.
        ICON_ADDED.store(false, Ordering::SeqCst);
        add_icon(hwnd, shared);
        return 0;
    }

    match msg {
        WM_TIMER => {
            // Re-adds the icon if Explorer was not ready at login, keeps the
            // tooltip current, and swaps the paused/active icon.
            refresh_icon(hwnd, shared);
            if shared.is_shutdown() {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_SETTINGCHANGE => {
            // Light/dark switch: Windows broadcasts "ImmersiveColorSet".
            if lparam != 0 {
                let mut len = 0usize;
                let ptr = lparam as *const u16;
                while len < 64 && *ptr.add(len) != 0 {
                    len += 1;
                }
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                if name == "ImmersiveColorSet" {
                    if let Some(dark) = DARK.get() {
                        apply_theme(dark);
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            shared.shutdown();
            remove_icon(hwnd);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_QUERYENDSESSION => {
            shared.shutdown();
            1
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn run(shared: Arc<Shared>) -> Result<()> {
    SHARED
        .set(Arc::clone(&shared))
        .map_err(|_| Error::new("tray already initialized"))?;

    let class = wide(CLASS_NAME);
    let title = wide("lumen");
    let hinstance = unsafe { GetModuleHandleW(null()) };
    if hinstance.is_null() {
        return Err(Error::last_os("GetModuleHandleW"));
    }

    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance,
        hIcon: current_icon(false),
        lpszClassName: class.as_ptr(),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        // 1410 = ERROR_CLASS_ALREADY_EXISTS
        if err != 1410 {
            return Err(Error::new(format!("RegisterClassW failed: win32 {err}")));
        }
    }

    // Hidden top-level window, not HWND_MESSAGE: message-only windows do not
    // receive the TaskbarCreated / WM_SETTINGCHANGE broadcasts.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(Error::last_os("CreateWindowExW"));
    }

    init_dark_mode(hwnd);
    let _ = TASKBAR_CREATED.set(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) });
    shared.tray_hwnd.store(hwnd as isize, Ordering::SeqCst);
    add_icon(hwnd, &shared);
    unsafe { SetTimer(hwnd, TIMER_ID, 1000, None) };

    let mut msg = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if result == 0 {
            break;
        }
        if result == -1 {
            shared.log("GetMessageW failed");
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    shared.tray_hwnd.store(0, Ordering::SeqCst);
    Ok(())
}
