//! systemd user service `vulcand.service`, wanted by `default.target`: it
//! starts with the user's service manager at login, whatever the desktop or
//! compositor. Starting before the compositor is fine: the tray waits for a
//! StatusNotifierWatcher and app rules find Hyprland once it runs.

use super::daemon_exe;
use crate::config::xdg_config_home;
use crate::error::{Error, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const UNIT: &str = "vulcand.service";

fn unit_dir() -> Result<PathBuf> {
    xdg_config_home()
        .map(|dir| dir.join("systemd").join("user"))
        .ok_or_else(|| Error::new("neither XDG_CONFIG_HOME nor HOME is set"))
}

fn systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| Error::new(format!("systemctl: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::new(format!(
            "systemctl --user {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// Double-quoted `ExecStart=` argument: C escapes inside the quotes, `%%`
/// for unit specifiers, `$$` for variable expansion.
fn exec_quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn unit_file(exe: &str) -> String {
    format!(
        "[Unit]\n\
         Description=lumen: ROCCAT Vulcan II Mini lighting\n\
         \n\
         [Service]\n\
         ExecStart={}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exec_quote(exe)
    )
}

/// Install and enable the unit for `vulcand` next to this executable.
/// Takes effect at the next login; a running daemon is left alone.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let dir = unit_dir()?;
    let unit = dir.join(UNIT);
    if enabled {
        let exe = daemon_exe()?;
        fs::create_dir_all(&dir)
            .map_err(|e| Error::new(format!("create {}: {e}", dir.display())))?;
        let tmp = unit.with_extension("service.tmp");
        fs::write(&tmp, unit_file(&exe.to_string_lossy()))
            .and_then(|()| fs::rename(&tmp, &unit))
            .map_err(|e| Error::new(format!("write {}: {e}", unit.display())))?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", UNIT])
    } else {
        if is_enabled() {
            systemctl(&["disable", UNIT])?;
        }
        if unit.exists() {
            fs::remove_file(&unit)
                .map_err(|e| Error::new(format!("remove {}: {e}", unit.display())))?;
            systemctl(&["daemon-reload"])?;
        }
        Ok(())
    }
}

/// Whether the `default.target` wants-link that `systemctl enable` creates
/// exists. A file check, cheap enough for every tray menu rebuild.
pub fn is_enabled() -> bool {
    unit_dir()
        .map(|dir| dir.join("default.target.wants").join(UNIT).exists())
        .unwrap_or(false)
}
