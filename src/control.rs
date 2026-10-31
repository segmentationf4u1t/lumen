//! Control socket for desktop shells: `$XDG_RUNTIME_DIR/lumen.sock`, one JSON
//! object per line in each direction. The Quickshell panel in
//! `quickshell/lumen` is the client.
//!
//! Daemon → client:
//! - `{"type":"layout","width","height","keys":[{"index","name","x","y","w"}]}` once;
//! - `{"type":"state",…}` on connect and whenever it changes;
//! - `{"type":"frame","rgb":"rrggbb…"}`, one triple per LED index, while the
//!   client asked for frames and the picture changes (at most ~30 per second).
//!
//! Client → daemon, `{"cmd":…}`: `profile` (`name`), `effect` (`name`, null =
//! from profile), `brightness` (`value` 0..1, null = from profile), `paused`,
//! `autostart` and `frames` (`on`), `reload`, `edit_config`, `open_folder`.

use crate::autostart;
use crate::effects::Effect;
use crate::error::{Error, Result};
use crate::layout::{self, BOARD_WIDTH};
use crate::protocol::LED_COUNT;
use crate::runtime::Shared;
use crate::tray;
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// How often a client gets state changes and preview frames.
const TICK: Duration = Duration::from_millis(33);
const ACCEPT_POLL: Duration = Duration::from_millis(100);
/// A client that stops reading must not stall its writer forever.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

pub fn socket_path() -> PathBuf {
    crate::config::runtime_dir().join("lumen.sock")
}

/// Accept clients until shutdown. Runs on its own thread.
pub fn serve(shared: Arc<Shared>) -> Result<()> {
    let path = socket_path();
    // Only the vulcand holding the daemon lock gets here, so a socket file
    // already at `path` was left by one that died without cleaning up.
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| Error::new(format!("bind {}: {e}", path.display())))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    // Non-blocking so the loop notices shutdown.
    listener.set_nonblocking(true)?;

    while !shared.is_shutdown() {
        match listener.accept() {
            Ok((stream, _)) => {
                let shared = Arc::clone(&shared);
                let _ = thread::Builder::new()
                    .name("control-client".to_owned())
                    .spawn(move || client(shared, stream));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(e) => {
                shared.log(&format!("control socket: {e}"));
                thread::sleep(ACCEPT_POLL);
            }
        }
    }
    let _ = fs::remove_file(&path);
    Ok(())
}

fn client(shared: Arc<Shared>, stream: UnixStream) {
    let frames = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicBool::new(false));
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    {
        let (shared, frames, closed) = (Arc::clone(&shared), Arc::clone(&frames), Arc::clone(&closed));
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                handle(&shared, &line, &frames);
            }
            closed.store(true, Ordering::SeqCst);
        });
    }

    let mut out = stream;
    let _ = out.set_nonblocking(false);
    let _ = out.set_write_timeout(Some(WRITE_TIMEOUT));
    let mut last_state = String::new();
    let mut last_frame = None;
    let mut ok = send(&mut out, &layout_message());
    while ok && !closed.load(Ordering::SeqCst) && !shared.is_shutdown() {
        let state = state_message(&shared).to_string();
        if state != last_state {
            ok = send(&mut out, &state);
            last_state = state;
        }
        if ok && frames.load(Ordering::SeqCst) {
            let frame = shared.frame();
            if last_frame != Some(frame) {
                ok = send(&mut out, &frame_message(&frame));
                last_frame = Some(frame);
            }
        }
        thread::sleep(TICK);
    }
    // Ends the reader thread too.
    let _ = out.shutdown(Shutdown::Both);
}

fn send(out: &mut UnixStream, line: &str) -> bool {
    out.write_all(line.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .is_ok()
}

fn handle(shared: &Arc<Shared>, line: &str, frames: &AtomicBool) {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let on = msg["on"].as_bool().unwrap_or(false);
    match msg["cmd"].as_str() {
        Some("profile") => {
            if let Some(name) = msg["name"].as_str() {
                tray::choose_profile(shared, name);
            }
        }
        Some("effect") => match msg["name"].as_str() {
            None => tray::choose_effect(shared, None),
            Some(name) => {
                if let Some(effect) = Effect::parse(name) {
                    tray::choose_effect(shared, Some(effect));
                }
            }
        },
        // A slider drag sends a stream of values; logging each would flood
        // vulcand.log.
        Some("brightness") => {
            shared.select_brightness(msg["value"].as_f64().map(|v| v as f32));
        }
        Some("paused") => tray::set_paused(shared, on),
        Some("autostart") => tray::set_autostart(shared, on),
        Some("frames") => frames.store(on, Ordering::SeqCst),
        Some("reload") => tray::reload(shared),
        Some("edit_config") => tray::open_path(shared, shared.config_path()),
        Some("open_folder") => {
            if let Some(dir) = shared.config_path().parent() {
                tray::open_path(shared, dir);
            }
        }
        _ => {}
    }
}

fn layout_message() -> String {
    let keys: Vec<Value> = layout::KEYS
        .iter()
        .map(|k| json!({ "index": k.index, "name": k.name, "x": k.x, "y": k.y, "w": k.w }))
        .collect();
    let height = layout::KEYS.iter().map(|k| k.y + 1.0).fold(0.0, f32::max);
    json!({ "type": "layout", "width": BOARD_WIDTH, "height": height, "keys": keys }).to_string()
}

fn state_message(shared: &Shared) -> Value {
    let cfg = shared.config();
    let sel = shared.selection();
    let active = shared.active();
    let profiles: Vec<Value> = cfg
        .profiles
        .iter()
        .map(|(name, p)| {
            json!({
                "name": name,
                "effect": p.effect.as_str(),
                "color": p.color,
                "secondary": p.secondary,
                "brightness": p.brightness,
            })
        })
        .collect();
    let effects: Vec<Value> = Effect::all()
        .iter()
        .map(|e| json!({ "id": e.as_str(), "label": tray::effect_label(*e) }))
        .collect();
    json!({
        "type": "state",
        "status": tray::status_label(shared),
        "connected": shared.status() == "connected",
        "paused": !shared.is_enabled(),
        "autostart": autostart::is_enabled(),
        "profile": sel.profile.unwrap_or_else(|| cfg.default_profile.clone()),
        "activeProfile": active.profile_name,
        "app": active.app,
        "effect": sel.effect.map(Effect::as_str),
        "activeEffect": active.effect.as_str(),
        "brightness": sel.brightness,
        "activeBrightness": active.brightness,
        "profiles": profiles,
        "effects": effects,
    })
}

fn frame_message(frame: &[[u8; 3]; LED_COUNT]) -> String {
    let mut rgb = String::with_capacity(LED_COUNT * 6);
    for [r, g, b] in frame {
        let _ = write!(rgb, "{r:02x}{g:02x}{b:02x}");
    }
    json!({ "type": "frame", "rgb": rgb }).to_string()
}
