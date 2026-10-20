<div align="center">

# lumen

**Per-key RGB for the ROCCAT Vulcan II Mini, without SWARM.**

![rust](https://img.shields.io/badge/rust-stable-black?style=flat-square&logo=rust)
![license](https://img.shields.io/badge/license-GPL--2.0--or--later-blue?style=flat-square)

</div>

## What it is

A tiny Windows tray app that drives the keyboard's LEDs directly over USB HID.
It switches lighting profiles to match the app you're using, and never touches
the firmware.

```
foreground app  →  profile  →  effect @ 30–60 fps  →  6 × 65 B HID reports  →  69 keys
```

## Quick start

Needs Rust with the MSVC toolchain and the Windows SDK (its resource compiler
embeds the icon).

```powershell
cargo build --release
.\target\release\vulcanctl.exe probe        # find the keyboard, read firmware
.\target\release\vulcand.exe                # start the tray app
```

Click the tray icon (on Windows 11 it may be hidden under **^**). The menu
follows the system light/dark theme:

- **Profile**, **Animation**, **Brightness**: your choice sticks. It is saved
  to `state.json` and survives restarts and alt-tabbing.
- **App rules** from the config temporarily replace your choice while that app
  is focused, then hand it back.
- **Pause lighting** and **Start with Windows** are toggles. **Edit config**
  opens the JSON in your default editor.

`vulcanctl quit` stops the tray app and leaves a static frame on the keyboard.

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

The config lives at `%APPDATA%\vulcan\config.json`. It's created on first run and
reloaded automatically about 1 s after you save it. `default_profile` is used
until you pick something in the tray. After that, your tray choice in
`state.json` wins.

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
| `vulcanctl autostart on\|off` | start with Windows (current user) |
| `vulcanctl quit` | stop the tray app and restore |

All flags: `vulcanctl --help`.

## Safety

- **Lighting only.** lumen sends lighting reports to the keyboard and nothing
  else. It has no firmware update or DFU code, so it can't brick the keyboard.
- To get exclusive access it closes **OpenRGB** and **SWARM** if they are
  running, and CLI commands that write to the keyboard stop `vulcand` first.
- No keyboard hooks, no macros, no key remapping. Profile switching only
  reads which app is in the foreground.
- Tested on `1E7D:2F09`, firmware `1.26`, Windows 11 x64.
