<div align="center">

# lumen

**Per-key RGB for the ROCCAT Vulcan II Mini, without SWARM.**

![rust](https://img.shields.io/badge/rust-stable-black?style=flat-square&logo=rust)
![license](https://img.shields.io/badge/license-GPL--2.0--or--later-blue?style=flat-square)

</div>

## What it is

A tiny tray app for Windows and Linux that drives the keyboard's LEDs directly
over USB HID. It switches lighting profiles to match the app you're using, and
never touches the firmware.

```
foreground app  →  profile  →  effect @ up to 60 fps  →  6 × 65 B HID reports  →  69 keys
```

## Quick start

### Windows

Needs Rust with the MSVC toolchain and the Windows SDK (its resource compiler
embeds the icon).

```powershell
cargo build --release
.\target\release\vulcanctl.exe probe        # find the keyboard, read firmware
.\target\release\vulcand.exe                # start the tray app
```

Click the tray icon (on Windows 11 it may be hidden under **^**). The menu
follows the system light/dark theme.

### Linux

Needs Rust and nothing else: no C libraries, no root at runtime. Allow your
user to open the keyboard's hidraw nodes once:

```sh
sudo install -m644 assets/70-vulcan.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw
```

```sh
cargo build --release
./target/release/vulcanctl probe            # find the keyboard, read firmware
./target/release/vulcand                    # start the tray app
```

The tray icon is a StatusNotifierItem: it shows up in any SNI host (Waybar's
`tray` module, Quickshell, KDE, GNOME with the AppIndicator extension). If no
host runs yet, it appears once one does. `vulcand --no-tray` runs headless.

### Quickshell panel (Linux)

With a [Quickshell](https://quickshell.org) bar, `quickshell/lumen/LumenPopup.qml`
replaces the generic tray menu with a panel: live keyboard preview, preset
cards, animation and brightness controls, pause and start-on-login. It talks to
`vulcand` over `$XDG_RUNTIME_DIR/lumen.sock` (JSON lines; protocol in
`src/control.rs`) and needs the JetBrainsMono Nerd Font and
[tabler-icons](https://tabler.io/icons) webfont.

```sh
ln -s "$PWD/quickshell/lumen" ~/.config/quickshell/lumen
```

Then load `lumen/LumenPopup.qml` in a popup of your bar and open it when the tray
item with id `lumen` is clicked. The panel takes an optional `dropdown` property
with a `close()` method.

### The tray menu

- **Profile**, **Animation**, **Brightness**: your choice sticks. It is saved
  to `state.json` and survives restarts and alt-tabbing.
- **App rules** from the config temporarily replace your choice while that app
  is focused, then hand it back.
- **Pause lighting** and **Start with Windows** / **Start on login** are
  toggles. **Edit config** opens the JSON in your default editor.

`vulcanctl quit` stops the tray app and leaves a static frame on the keyboard.

On Linux, **Start on login** installs a systemd user service
(`~/.config/systemd/user/vulcand.service`, wanted by `default.target`), so it
works with any desktop or compositor. It points at the `vulcand` it was enabled
from; re-enable it after moving the binary.

## Effects

| Effect | Look |
|---|---|
| `plasma` | saturated, flowing color field |
| `comet` | white-core comet with a colored tail |
| `aurora` | drifting cyan, green and magenta curtains |
| `scanner` | bouncing bright beam with a trail |
| `fire` | warm, smooth flicker, no strobe |
| `spectrum` | rainbow sweeping across the board |
| `wave` | two-color gradient wave |
| `breathe` | single color, slow pulse |
| `static` | one solid color |
| `showcase` | 36 s loop of the six effects above `wave`, with crossfades |

## Configuration

The config lives at `%APPDATA%\vulcan\config.json` on Windows and
`~/.config/vulcan/config.json` (`$XDG_CONFIG_HOME`) on Linux, next to
`state.json` and `vulcand.log`. It's created on first run and reloaded
automatically about 1 s after you save it. `default_profile` is used until you
pick something in the tray. After that, your tray choice in `state.json` wins.

A new config starts with dark-volcanic presets, slow and smooth, at 60 fps:

| Preset | Look |
|---|---|
| `default` | magma glow flowing over dark crust (`wave`) |
| `magma` | magma and sulfur waves |
| `lava` | red lava over dark crust, slower |
| `ember` | slow ember breathing |
| `eruption` | `fire` flicker at full brightness |
| `basalt` | dim static magma |
| `fps` | static magma, cream WASD |

Every change of profile, animation, brightness or app rule crossfades over
0.6 s instead of cutting.

```json
{
  "version": 1,
  "default_profile": "showcase",
  "profiles": {
    "showcase": { "effect": "showcase", "brightness": 1.0 },
    "fps": {
      "effect": "static",
      "color": "#ff6600",
      "keys": { "W": "#ffffff", "A": "#ffffff", "S": "#ffffff", "D": "#ffffff" }
    }
  },
  "app_rules": [{ "process": "cs2.exe", "profile": "fps" }],
  "settings": { "fps": 60, "debounce_ms": 800 }
}
```

| Field | Meaning |
|---|---|
| `effect` | any name from [Effects](#effects) |
| `color`, `secondary` | `#rrggbb` |
| `speed` | `0.1`–`5.0` |
| `brightness` | `0.0`–`1.0`, scales everything including per-key colors |
| `keys` | per-key colors; names come from `vulcanctl dump-layout`, case-insensitive |
| `app_rules` | process name → profile, matched case-insensitively |

On Linux, app rules need Hyprland: Wayland gives other apps no portable way to
see the focused window, so elsewhere they stay inactive. The process name is
the executable's file name (`firefox`, `cs2`); Wine and Proton games match by
their Windows `.exe` name, so Windows rules like `cs2.exe` keep working.

If the config is broken, lumen keeps the last working one (defaults at startup) and
doesn't overwrite your file.

## CLI

| Command | Does |
|---|---|
| `vulcanctl probe` | list HID interfaces, read firmware and layout |
| `vulcanctl demo` | run `showcase` at full brightness, then restore |
| `vulcanctl effect --name <fx>` | run one effect, then restore |
| `vulcanctl static --color <hex>` | set a persistent static color |
| `vulcanctl sweep` | light keys one at a time to check the key map |
| `vulcanctl autostart on\|off` | start `vulcand` at login (current user) |
| `vulcanctl quit` | stop the tray app and restore |

All flags: `vulcanctl --help`.

## Safety

- **Lighting only.** lumen sends lighting reports to the keyboard and nothing
  else. It has no firmware update or DFU code, so it can't brick the keyboard.
- The static frame left on exit (and `vulcanctl static`) is stored in the
  keyboard's active onboard profile, replacing that profile's saved effect.
  The other onboard profiles are not touched.
- To get exclusive access it closes **OpenRGB** and **SWARM** if they are
  running (on Linux: OpenRGB, with SIGTERM), and CLI commands that write to the
  keyboard stop `vulcand` first.
- No keyboard hooks, no macros, no key remapping. Profile switching only
  reads which app is in the foreground.
- Tested on `1E7D:2F09`, firmware `1.26`: Windows 11 x64, and Arch Linux with
  Hyprland 0.56 and Quickshell.
