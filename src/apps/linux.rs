//! Focused window via Hyprland's IPC socket. Wayland gives ordinary clients
//! no portable way to see the focused window, so on other compositors
//! `foreground_process_name` returns `None` and app rules stay inactive.

use super::basename;
use serde::Deserialize;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const IPC_TIMEOUT: Duration = Duration::from_millis(500);

/// Socket of the Hyprland instance that answered last.
static SOCKET: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn foreground_process_name() -> Option<String> {
    let pid = active_window_pid()?;
    if pid == std::process::id() {
        return None;
    }
    process_name(pid)
}

fn active_window_pid() -> Option<u32> {
    let mut cached = SOCKET.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(socket) = cached.as_deref() {
        if let Some(reply) = request(socket, b"j/activewindow") {
            return parse_pid(&reply);
        }
        *cached = None;
    }
    for socket in hyprland_sockets() {
        if let Some(reply) = request(&socket, b"j/activewindow") {
            *cached = Some(socket);
            return parse_pid(&reply);
        }
    }
    None
}

/// Candidates, best first: the instance named in our environment, then
/// every running instance (newest first). The daemon may have started before
/// the compositor, e.g. as a systemd user service, or outlived a restart.
fn hyprland_sockets() -> Vec<PathBuf> {
    let Some(root) = std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("hypr"))
    else {
        return Vec::new();
    };
    let mut sockets = Vec::new();
    if let Some(signature) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") {
        sockets.push(root.join(signature).join(".socket.sock"));
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return sockets;
    };
    // Hyprland removes `hyprland.lock` on exit; directories without it are
    // left over from earlier sessions.
    let mut running: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let started = fs::metadata(dir.join("hyprland.lock")).ok()?.modified().ok()?;
            Some((started, dir.join(".socket.sock")))
        })
        .collect();
    running.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, socket) in running {
        if !sockets.contains(&socket) {
            sockets.push(socket);
        }
    }
    sockets
}

fn request(socket: &Path, command: &[u8]) -> Option<Vec<u8>> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(IPC_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IPC_TIMEOUT)).ok()?;
    stream.write_all(command).ok()?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).ok()?;
    Some(reply)
}

/// `{}` when nothing is focused.
fn parse_pid(reply: &[u8]) -> Option<u32> {
    #[derive(Deserialize)]
    struct ActiveWindow {
        pid: Option<i64>,
    }
    let window: ActiveWindow = serde_json::from_slice(reply).ok()?;
    u32::try_from(window.pid?).ok().filter(|&pid| pid > 0)
}

/// Executable basename, lower-cased. Wine/Proton games run inside a `wine*`
/// loader; for those the Windows executable in argv[0] is the useful name,
/// so rules written for Windows (`cs2.exe`) keep matching.
fn process_name(pid: u32) -> Option<String> {
    let dir = PathBuf::from(format!("/proc/{pid}"));
    let exe = fs::read_link(dir.join("exe")).ok().and_then(|exe| {
        let name = exe.file_name()?.to_string_lossy().into_owned();
        Some(name.trim_end_matches(" (deleted)").to_owned())
    });
    let name = match exe {
        Some(name) if !name.starts_with("wine") => name,
        // A Wine loader, or an exe link we may not read.
        _ => {
            let cmdline = fs::read(dir.join("cmdline")).ok()?;
            let argv0 = cmdline.split(|&b| b == 0).next()?;
            basename(&String::from_utf8_lossy(argv0)).to_owned()
        }
    };
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}
