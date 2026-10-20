# Failure modes — Vulcan II Mini host driver

Status: written **before** implementation, as required. Each item is a falsifiable
hypothesis or a known failure class. `probe` must confirm the hardware facts before
any effect loop is allowed to run.

Scope: user-space HID only. **No firmware writes, no flashing, no erase.** The only
writes are lighting feature reports and LED output reports.

Target: ROCCAT Vulcan II Mini, USB `VID_1E7D&PID_2F09`, observed on this machine.
A Turtle Beach-branded variant may use another PID; it is out of scope until probed.

---

## 0. Hardware facts that must be verified first

If any of these is false, stop and fix the protocol layer before rendering frames.

| # | Hypothesis | How probe falsifies it |
|---|------------|------------------------|
| H1 | Control collection is usage page `0xFF01`, LED collection is `0xFF00` | Print every HID interface for the PID with usage page + report lengths |
| H2 | Control feature reports use IDs `0x04`, `0x09`, `0x0E`, `0x11`, `0x13` | Send/read each and report `GetLastError` |
| H3 | LED output report is 65 bytes, report ID `0x00`, magic `0xA1` | Check `OutputReportByteLength`; one harmless frame |
| H4 | Mini color payload is 345 bytes; 6 output packets of 60 data bytes | Build and dump packet lengths before writing |
| H5 | Logical key count is 69; hardware slots are non-contiguous | Dump slot map; light one key at a time and confirm visually |
| H6 | Static report `0x11` is 332 bytes and needs `0x13` keepalives to commit | Try static, then static+keepalive, observe |
| H7 | Direct mode is enabled by feature report `0x0E 05 01` | Send one frame in direct mode, observe |
| H8 | Version query is `0x09`, 9 bytes; firmware at byte 2, layout at byte 6 | Print values; reject unknown layout |
| H9 | Static/direct brightness is 1..69; speed is 1..11 | Clamp and reject zero |

## 1. Device discovery / open failures

1. **Device absent.** USB unplugged, hub off, KVM switched, or boot before enumeration.
   - Detect: no matching VID/PID/paths.
   - Mitigate: retry with backoff (250 ms → 5 s), never spin; log once per state change.
2. **Wrong PID / branded variant.** A Turtle Beach unit or a different Vulcan model.
   - Detect: VID/PID not in allowlist; layout byte or report sizes differ.
   - Mitigate: allowlist only `1E7D:2F09`; print unknown devices in `probe`; do not
     guess a protocol for an unknown PID.
3. **SWARM II (or another tool) owns the interface.** `CreateFileW` fails with
   `ERROR_ACCESS_DENIED` or `ERROR_SHARING_VIOLATION`.
   - Mitigate: open with `FILE_SHARE_READ|FILE_SHARE_WRITE`; report actionable error
     ("close SWARM II / OpenRGB"); retry.
4. **Interface path changes after re-enumeration.** Paths contain port/instance IDs.
   - Mitigate: never persist a path by default; re-enumerate on every connect. Optional
     `path_hint` config for multi-keyboard users.
5. **Multiple keyboards connected.** Wrong keyboard receives frames.
   - Detect: more than one matching device.
   - Mitigate: choose the first by deterministic path sort, log serial/path; allow
     explicit `device.path` in config. Never silently control all.
6. **Collection ordering differs.** MI_01/MI_03 assumption breaks on another revision.
   - Mitigate: select by usage page (`0xFF01` / `0xFF00`), not interface number.
7. **HID report caps lie or are zero.** `HidP_GetCaps` returns 0 / wrong sizes.
   - Mitigate: reject device unless output length ≥ 65 and feature length ≥ 123;
     print caps.
8. **Windows power management / selective suspend.** Handle goes stale after sleep.
   - Detect: writes fail with `ERROR_DEVICE_NOT_CONNECTED`/`ERROR_INVALID_HANDLE`.
   - Mitigate: close, re-enumerate, reconnect; backoff; never reuse a stale handle.
9. **Session 0 / service context.** No tray, no foreground window, no HID access.
   - Mitigate: run as the interactive user; document it.
10. **Elevated process / anti-cheat.** Some games run elevated; foreground detection
    may fail, but HID writes usually still work.
    - Mitigate: process-name lookup uses `PROCESS_QUERY_LIMITED_INFORMATION`; on
      failure keep the previous profile rather than flashing a default.

## 2. Protocol / report failures

11. **Report ID vs payload off-by-one.** `HidD_SetFeature` expects the report ID in
    byte 0; checksum/length fields may include it.
    - Mitigate: build packets from explicit field offsets; probe reads back version.
    - QA: golden dumps of every packet, assert lengths and checksums.
12. **Feature report length mismatch.** Windows may reject a buffer that is not the
    declared length for that report ID.
    - Detect: `HidD_SetFeature` false + `GetLastError`.
    - Mitigate: use exact lengths (5, 9, 123, 332); never send a padded generic buffer.
13. **Checksum wrong.** Static report checksum must cover the same byte range the
    firmware expects (including/omitting report ID is the ambiguity).
    - Mitigate: implement one definition, document it; if static does not commit,
      test both variants in `probe --static-checksum=with-id|without-id`.
14. **Packet frame length wrong.** 345 vs 332 vs 360 bytes; packet count 6 vs 5.
    - Mitigate: derive from constants; assert `data_capacity >= payload`.
15. **Packet sequence wrong.** Index must start at 1 and be contiguous; length fields
    only in packet 1 (hypothesis). Write order must be 1..N.
    - Mitigate: single writer; dump sequence; no interleaving between frames.
16. **Writes too fast / device drops frames.** 6 interrupt writes per frame at 30 fps.
    - Detect: intermittent stale colors / no update.
    - Mitigate: 30 fps default; optional 2 ms pacing between packets; measure write
      latency; drop frames instead of queuing.
17. **LED interface input pipe fills.** Device may send ACKs; nobody reads them.
    - Mitigate: drain input reports on a dedicated read handle/thread, or read with a
      zero timeout after each frame; if writes stall, add the drain.
18. **Direct mode not active.** Firmware effect overrides frames.
    - Observed: sending only `0x0E 05 01` is not enough to prove the LED path is
      live. The daemon now sends `0x11` mode `0x0B`, then `0x0E 05 01`, on every
      connect/reconnect.
    - Remaining verification: a human must confirm the LEDs change; HID success
      is not color readback.
19. **Direct mode left on at exit.** Keyboard freezes on last frame after process dies.
    - Mitigate: restore static profile on graceful exit; watchdog is impossible after
      a hard kill, so document it.
20. **Static mode silently ignored.** Mini may require the keepalive handshake.
    - Mitigate: send static report, wait-ready, keepalive ×3 (25 ms), then static again
      if needed; `probe` supports both orders.
21. **Zero speed/brightness rejected.** Firmware may ignore zeros.
    - Mitigate: clamp speed to 1..11, brightness to 1..69; `0` maps to defaults.
22. **Firmware revision changes behavior.** Newer firmware may differ.
    - Mitigate: print version; allow `--no-keepalive`/`--keepalive` overrides; never
      auto-update firmware.
23. **Layout byte unknown (JIS/other).** Wrong key labels / map.
    - Observed: this unit reports layout `0`, not 104/105. Do not reject it.
    - Mitigate: use the authored 69-key table; log the raw layout byte. JIS/other
      variants are out of scope until a unit is probed.
24. **Slot map wrong for a key.** One LED lights the wrong key or stays dark.
    - Mitigate: per-key test mode (`probe --sweep`) lights slots one by one; map is
      data, not logic; user confirms visually once.

## 3. Rendering / daemon failures

25. **Render loop blocks on USB.** A slow/failed write stalls effects and UI.
    - Mitigate: single dedicated render thread; no allocation in the frame path;
      writes are independent; on error reconnect.
26. **Frame tearing.** Six reports are not atomic; a slow frame can show a seam.
    - Mitigate: keep packets contiguous; acceptable at 30 fps; no partial frame
      queuing.
27. **CPU burn.** Busy loop.
    - Mitigate: sleep until next deadline, not fixed sleep; clamp missed frames.
28. **Time source jumps.** Sleep/resume makes `Instant` jump; effects flash.
    - Mitigate: clamp `dt` to 100 ms; reset phase after a long gap.
29. **Panic in render/daemon.** Keyboard stays in last frame or direct mode.
    - Mitigate: no panics in packet builders (checked arithmetic, explicit errors);
      `catch_unwind` at thread boundary; graceful shutdown handler.
30. **Deadlock / lock poisoning.** Shared state between render, watcher, tray.
    - Mitigate: `Arc<AtomicU64>` generation + `RwLock` with poison recovery; short
      critical sections; no locks held across USB calls.
31. **Reconnect storm.** Device flaps, app logs/opens thousands of times.
    - Mitigate: exponential backoff capped at 5 s; state-change logging.
32. **Config file missing/corrupt/partial.** Daemon exits or resets user profiles.
    - Mitigate: default config on first run; parse errors fall back to last good
      in-memory config; log path and line; atomic save (`tmp` + rename); keep `.bak`.
33. **Config schema drift.** Old keys / new keys.
    - Mitigate: `version` field; ignore unknown fields; validate ranges on load.
34. **Two instances.** Two renderers fight, two tray icons.
    - Mitigate: named mutex/single-instance guard; second instance exits with message.
35. **Startup race.** Daemon starts before keyboard enumeration.
    - Mitigate: connect retry loop independent of tray.
36. **Exit path not run.** User kills process / logs off / power loss.
    - Mitigate: Windows console control handler + WM_QUERYENDSESSION where possible;
      document that a hard kill can leave a static/direct frame.
37. **Sleep/resume.** Timers drift, handle invalid.
    - Mitigate: reconnect on write failure; re-apply frame after resume.
38. **Tray/Explorer restart.** Icon disappears.
    - Mitigate: handle `TaskbarCreated` message and re-add icon.
39. **Tray menu stale.** Profile list changed on reload.
    - Mitigate: rebuild menu on config reload; command IDs map to a stable profile list.
40. **Open config fails.** No editor association.
    - Mitigate: fall back to `notepad.exe`; show error in tray tooltip/log.

## 4. Per-application profile failures

41. **Foreground process lookup fails.** Protected/UWP/elevated app, window closed.
    - Mitigate: keep last profile; debounce; never switch on transient empty window.
42. **Alt-tab flapping.** Profile switches on every focus change.
    - Mitigate: debounce window (default 800 ms) and only apply after stable foreground.
43. **Process name case/path differences.** Rule `cs2.exe` vs `Counter-Strike 2/cs2.exe`.
    - Mitigate: match case-insensitive basename; optional full-path rules.
44. **Rule points to missing profile.** Silent no-op.
    - Mitigate: validate on load; log and fall back to default.
45. **Manual override fights rules.** User selects profile, watcher immediately reverts.
    - Observed: "pin until the foreground process changes" reverted the tray
      choice to `default_profile` on every alt-tab, even with no rules.
    - Mitigate: the tray choice is the persistent base (`state.json`). App rules
      only overlay it while a matching process is focused; no match never
      resets anything.
46. **Watcher polls too often.** CPU/wakeups.
    - Mitigate: 750 ms–1 s poll; cheap Win32 calls.
47. **Game launcher vs game process.** Launcher rule matches, game doesn't.
    - Mitigate: document rule on the actual game exe; allow multiple rules.
48. **Anti-cheat false positive.** Global keyboard hooks look suspicious.
    - Mitigate: **no keyboard hooks in v1.** Effects are time-based; no key logging.

## 5. Safety / user-impact failures

49. **Firmware damage.** Out of scope: no DFU/flash commands are implemented.
50. **Photosensitive flashing.** High-contrast effects at high speed.
    - Mitigate: speed floor, smooth sine, brightness clamp; no strobe effect.
51. **Over-brightness / heat.** Clamp brightness; default 70%.
52. **Malformed config injects huge values.** Validate all ranges; reject NaN/inf.
53. **Path traversal / arbitrary file write.** Config path fixed; profile names not
    used as filenames (single JSON).
54. **Accidental key remapping.** Not implemented; lighting only. This avoids the
    most dangerous class of "gaming software" bugs.
55. **User expects macros/remap.** Explicitly out of scope for v1; requires a separate
    protocol investigation and is not silently faked.

## 6. Verification limitations

56. **HID write success ≠ visible LED update.** There is no color readback.
    - Mitigate: `probe` prints every write result; `--sweep` requests one visual
      confirmation; QA loop pairs each protocol change with a human-visible test.
57. **CI has no keyboard.** Automated tests can only verify packet construction,
    checksums, layout invariants, config parsing, effect bounds.
    - Mitigate: golden packet tests; hardware tests behind `--device`.
58. **Windows-only.** No Linux/macOS support in this codebase.
59. **Different USB port/driver stack.** Same device may expose different paths; caps
    are re-read every connect.

## 7. Stop-ship conditions

Do not merge/release a build if any of these is true:

- `probe` cannot read version/layout or open both collections.
- Any packet length/checksum assertion fails.
- A frame write returns an error that is not handled by reconnect.
- Config parse failure destroys the existing file.
- Exit path leaves direct mode enabled without attempting restore.
- A build sends any feature report other than `0x04`, `0x09`, `0x0E`, `0x11`, `0x13`.
- The render path contains an unchecked slice index or `unwrap` on device I/O.

## 8. QA loop (hardware in the loop)

1. `cargo test` — packet builders, checksum, layout, config, effects.
2. `cargo run --bin vulcanctl -- probe` — enumerate, read version, print caps.
3. `vulcanctl static --color 00ff00 --brightness 40` — visual check.
4. `vulcanctl frame --color ff0000` — direct-mode visual check.
5. `vulcanctl sweep --delay 300` — verify slot map (optional).
6. `vulcanctl effect --name wave --seconds 5` — smoothness/tearing check.
7. `vulcanctl restore` — confirm static restore and direct off.
8. Start daemon, verify per-app rule with a known process; alt-tab debounce.
9. Kill daemon gracefully; verify restore. Kill hard; verify documented last-frame
   behavior and recovery on next start.
10. Repeat after unplug/replug, sleep/resume, and with SWARM II open (expected fail
    with actionable error).

---

## 9. Hardware results (firmware 1.26, PID 0x2F09)

Measured on the target machine after the first implementation pass. These are
observations, not assumptions.

| Fact | Result |
|------|--------|
| HID collections | 8 interfaces total; control = `MI_01&COL05`, usage page `0xFF01`, feature max 1044; LED = `MI_03`, usage page `0xFF00`, in/out 65 |
| Keyboard/consumer collections | `MI_00`, `MI_01&COL01..04`, `MI_02` are opened by Windows; `CreateFileW` returns access denied for those (expected) |
| Identity report `0x09` | Works with 9 bytes. Version byte 2 = `126` (1.26). Layout byte 6 = `0`. Reading with <9 bytes fails with `ERROR_INVALID_PARAMETER` (87) |
| Ready report `0x04` | **Requires at least 4 bytes**, not 3. 3 bytes fails with error 87; 4+ bytes works and byte 1 is `1` |
| Static report `0x11` | 332 bytes accepted; commit succeeds with wait-ready + three `0x13` keepalives |
| Keepalive `0x13` | 123 bytes accepted. `HidD_GetFeature` on `0x11`/`0x13` is invalid (write-only); it returns error 31 |
| Direct mode | `0x11` mode `0x0B` then `0x0E 05 01` accepted; six 65-byte LED packets per frame accepted. The daemon arms this on every connect/reconnect |
| Frame pacing | 120 consecutive frames at 30 fps produced no write errors; no input drain was needed in this test |
| Layout byte | `0` on this unit, not 104/105. The driver uses the authored 69-key table and does not reject 0 |
| App watcher | Foreground process name resolved and rule switching logged correctly |

Open questions still requiring visual confirmation:

- Whether the static frame and direct frame are actually visible (HID success is
  not color readback).
- Whether the slot map lights the expected physical key for every key.
- Behaviour of the Turtle Beach-branded variant and other firmware revisions.
