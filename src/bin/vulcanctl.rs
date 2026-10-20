use std::time::{Duration, Instant};
use vulcan::color::Rgb;
use vulcan::config::{default_config_path, Config};
use vulcan::effects::{self, Effect, Profile};
use vulcan::error::{Error, Result};
use vulcan::hid::{dump_interfaces, PRODUCT_ID, VENDOR_ID};
use vulcan::layout;
use vulcan::protocol::VulcanDevice;
use vulcan::protocol::{BRIGHTNESS_MAX, LED_COUNT, SLOT_MAP, SPEED_MAX};
use vulcan::{autostart, takeover};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() {
        help();
        return Ok(());
    }
    let cmd = argv.remove(0);
    if matches!(cmd.as_str(), "-h" | "--help" | "help")
        || argv.iter().any(|a| a == "-h" || a == "--help")
    {
        help();
        return Ok(());
    }

    // Writing commands need the device to themselves: stop the tray daemon
    // and close other RGB tools. Temporary commands bring the daemon back.
    let writes = matches!(
        cmd.as_str(),
        "static" | "frame" | "effect" | "demo" | "sweep" | "off" | "restore"
    );
    let temporary = matches!(cmd.as_str(), "frame" | "effect" | "demo" | "sweep");
    let mut daemon_was_running = false;
    if writes {
        daemon_was_running = takeover::stop_daemon(Duration::from_secs(5))?;
        if daemon_was_running {
            println!("stopped vulcand");
        }
        for exe in takeover::close_conflicting_apps() {
            println!("closed {exe}");
        }
    }
    let result = dispatch(&cmd, &argv);
    if daemon_was_running && temporary {
        takeover::start_daemon()?;
        println!("restarted vulcand");
    }
    result
}

fn dispatch(cmd: &str, argv: &[String]) -> Result<()> {
    let argv = argv.to_vec();
    match cmd {
        "probe" => probe(),
        "dump-layout" => dump_layout(),
        "static" => static_cmd(&argv),
        "frame" => frame_cmd(&argv),
        "effect" => effect_cmd(&argv),
        "demo" => demo_cmd(&argv),
        "sweep" => sweep_cmd(&argv),
        "off" => off_cmd(),
        "restore" => restore_cmd(),
        "validate-config" => validate_config_cmd(&argv),
        "autostart" => autostart_cmd(&argv),
        "quit" => quit_cmd(),
        other => Err(Error::new(format!("unknown command {other:?}; try --help"))),
    }
}

fn help() {
    println!(
        r#"vulcanctl — minimal Vulcan II Mini lighting control

USAGE:
  vulcanctl <command> [options]

COMMANDS:
  probe                       enumerate HID interfaces, read firmware/layout
  dump-layout                 print key index -> hardware slot
  static [--color rrggbb] [--brightness 0-100] [--speed 1-11]
                              set a persistent static frame
  frame [--color rrggbb] [--brightness 0-100] [--hold SECONDS]
                              send one direct-mode frame, then restore
  effect --name wave [--color rrggbb] [--brightness 0-100]
         [--seconds 5] [--fps 30]
                              run a direct-mode effect, then restore
  demo [--seconds 60] [--fps 60]
                              bright showcase loop: plasma, comet, aurora,
                              scanner, fire, spectrum (smooth crossfades)
  sweep [--delay MS]          light one key at a time (slot-map check)
  off                         all LEDs off (static black)
  restore                     restore config default profile as static
  validate-config [--config PATH]
                              parse and validate config
  autostart on|off            per-user registry autostart for vulcand.exe
  quit                        ask a running tray daemon to exit gracefully

SAFETY:
  Only lighting reports are written. No firmware/DFU commands exist here.
  Close SWARM II / OpenRGB before running.
"#
    );
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn parse_color(args: &[String], fallback: Rgb) -> Result<Rgb> {
    match opt(args, "--color") {
        Some(s) => Rgb::parse(s).map_err(Error::new),
        None => Ok(fallback),
    }
}

fn parse_brightness_percent(args: &[String], default: u8) -> Result<u8> {
    match opt(args, "--brightness") {
        Some(s) => {
            let v: u8 = s
                .parse()
                .map_err(|_| Error::new(format!("invalid brightness {s:?}")))?;
            Ok(v.min(100))
        }
        None => Ok(default),
    }
}

fn percent_to_static(pct: u8) -> u8 {
    if pct == 0 {
        return 1;
    }
    ((pct as u32 * BRIGHTNESS_MAX as u32) / 100).clamp(1, BRIGHTNESS_MAX as u32) as u8
}

fn percent_to_factor(pct: u8) -> f32 {
    (pct.min(100) as f32) / 100.0
}

fn parse_speed(args: &[String]) -> Result<u8> {
    match opt(args, "--speed") {
        Some(s) => {
            let v: u8 = s
                .parse()
                .map_err(|_| Error::new(format!("invalid speed {s:?}")))?;
            Ok(v.clamp(1, SPEED_MAX))
        }
        None => Ok(6),
    }
}

fn solid_colors(color: Rgb, brightness: f32) -> [[u8; 3]; LED_COUNT] {
    let c = color.scale(brightness);
    [[c.0, c.1, c.2]; LED_COUNT]
}

fn open_device() -> Result<VulcanDevice> {
    VulcanDevice::open().map_err(|e| {
        Error::new(format!(
            "{e}\nIs SWARM II/OpenRGB holding the device? Is the keyboard connected as 1e7d:2f09?"
        ))
    })
}

fn probe() -> Result<()> {
    println!("Enumerating HID interfaces (VID {VENDOR_ID:04x}, PID {PRODUCT_ID:04x})...\n");
    let all = dump_interfaces(false)?;
    let targets: Vec<_> = all.iter().filter(|i| i.is_target()).collect();
    println!("Target interfaces: {}\n", targets.len());
    for i in &targets {
        println!("path:        {}", i.path);
        println!(
            "  vid:pid:   {:04x}:{:04x}  usage_page: 0x{:04x}  usage: 0x{:04x}",
            i.vendor_id, i.product_id, i.usage_page, i.usage
        );
        println!(
            "  reports:   in {}  out {}  feature {}",
            i.input_len, i.output_len, i.feature_len
        );
        println!("  product:   {:?}  serial: {:?}", i.product, i.serial);
        if let Some(e) = &i.open_error {
            println!("  open note: {e}");
        }
        println!();
    }
    if targets.is_empty() {
        println!("No matching interface found. Connected ROCCAT devices:");
        for i in all.iter().filter(|i| i.vendor_id == VENDOR_ID) {
            println!(
                "  {:04x}:{:04x} usage 0x{:04x} out {} feature {}  {}",
                i.vendor_id, i.product_id, i.usage_page, i.output_len, i.feature_len, i.path
            );
        }
        return Err(Error::new("keyboard not found"));
    }

    println!("Opening control + LED collections...");
    let dev = open_device()?;
    let info = dev.info();
    println!("OK");
    println!("  product:   {:?}", info.product);
    println!(
        "  firmware:  {} (raw {})",
        info.version(),
        info.version_code
    );
    println!("  layout:    {}", info.layout);
    println!("  control:   {}", dev.control_path());
    println!("  led:       {}", dev.led_path());
    Ok(())
}

fn dump_layout() -> Result<()> {
    println!("idx  name             slot  x     y     w");
    for key in layout::KEYS {
        let slot = SLOT_MAP[key.index as usize];
        println!(
            "{:>3}  {:<15}  {:>4}  {:>4.2}  {:>4.2}  {:>4.2}",
            key.index, key.name, slot, key.x, key.y, key.w
        );
    }
    Ok(())
}

fn static_cmd(args: &[String]) -> Result<()> {
    let color = parse_color(args, Rgb(0xFF, 0xFF, 0xFF))?;
    let brightness_pct = parse_brightness_percent(args, 40)?;
    let speed = parse_speed(args)?;
    let colors = solid_colors(color, percent_to_factor(brightness_pct));
    let dev = open_device()?;
    println!(
        "Writing static {} at {}% (speed {})...",
        color, brightness_pct, speed
    );
    dev.enable_direct(false)?;
    dev.send_static(&colors, speed, percent_to_static(brightness_pct))?;
    println!("OK — static report committed");
    Ok(())
}

fn frame_cmd(args: &[String]) -> Result<()> {
    let color = parse_color(args, Rgb(0x00, 0xAE, 0xEF))?;
    let brightness_pct = parse_brightness_percent(args, 40)?;
    let hold: f32 = opt(args, "--hold")
        .map(|s| {
            s.parse()
                .map_err(|_| Error::new(format!("invalid hold {s:?}")))
        })
        .transpose()?
        .unwrap_or(2.0);
    let colors = solid_colors(color, percent_to_factor(brightness_pct));
    let dev = open_device()?;
    println!(
        "Direct frame {} at {}% for {:.1}s...",
        color, brightness_pct, hold
    );
    dev.begin_direct()?;
    dev.send_frame(&colors)?;
    std::thread::sleep(Duration::from_secs_f32(hold.max(0.0)));
    restore_default(&dev)?;
    println!("OK — frame sent, static restored");
    Ok(())
}

fn effect_cmd(args: &[String]) -> Result<()> {
    let name = opt(args, "--name").unwrap_or("wave");
    let effect = Effect::parse(name).ok_or_else(|| {
        Error::new(format!(
            "unknown effect {name:?} (static|breathe|wave|spectrum)"
        ))
    })?;
    let color = parse_color(args, Rgb(0x00, 0xAE, 0xEF))?;
    let brightness_pct = parse_brightness_percent(args, 60)?;
    let seconds: f32 = opt(args, "--seconds")
        .map(|s| {
            s.parse()
                .map_err(|_| Error::new(format!("invalid seconds {s:?}")))
        })
        .transpose()?
        .unwrap_or(5.0);
    let fps: u32 = opt(args, "--fps")
        .map(|s| {
            s.parse()
                .map_err(|_| Error::new(format!("invalid fps {s:?}")))
        })
        .transpose()?
        .unwrap_or(30)
        .clamp(1, 120);
    let speed: f32 = opt(args, "--speed")
        .map(|s| {
            s.parse::<f32>()
                .map_err(|_| Error::new(format!("invalid speed {s:?}")))
        })
        .transpose()?
        .unwrap_or(1.0)
        .clamp(0.1, 5.0);

    let profile = Profile {
        effect,
        color,
        speed,
        brightness: percent_to_factor(brightness_pct),
        ..Default::default()
    };
    profile.validate().map_err(Error::new)?;

    let dev = open_device()?;
    println!(
        "Effect {} at {}% for {:.1}s @ {} fps...",
        effect.as_str(),
        brightness_pct,
        seconds,
        fps
    );
    dev.begin_direct()?;
    let start = Instant::now();
    let frame_time = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next = Instant::now();
    let mut colors = [[0u8; 3]; LED_COUNT];
    while start.elapsed().as_secs_f32() < seconds {
        effects::render(&profile, start.elapsed().as_secs_f32(), &mut colors);
        dev.send_frame(&colors)?;
        next += frame_time;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    restore_default(&dev)?;
    println!("OK — effect stopped, static restored");
    Ok(())
}

fn demo_cmd(args: &[String]) -> Result<()> {
    let mut v = vec!["--name".to_string(), "showcase".to_string()];
    if opt(args, "--brightness").is_none() {
        v.push("--brightness".into());
        v.push("100".into());
    }
    if opt(args, "--fps").is_none() {
        v.push("--fps".into());
        v.push("60".into());
    }
    if opt(args, "--seconds").is_none() {
        v.push("--seconds".into());
        v.push("60".into());
    }
    v.extend(args.iter().cloned());
    effect_cmd(&v)
}

fn sweep_cmd(args: &[String]) -> Result<()> {
    let delay: u64 = opt(args, "--delay")
        .map(|s| {
            s.parse()
                .map_err(|_| Error::new(format!("invalid delay {s:?}")))
        })
        .transpose()?
        .unwrap_or(250);
    let dev = open_device()?;
    println!("Sweep: one key at a time, {delay} ms each. Watch the keyboard.");
    dev.begin_direct()?;
    for key in layout::KEYS {
        let mut colors = [[0u8; 3]; LED_COUNT];
        colors[key.index as usize] = [255, 255, 255];
        dev.send_frame(&colors)?;
        println!(
            "key {:>2} {:<15} slot {:>2}",
            key.index, key.name, SLOT_MAP[key.index as usize]
        );
        std::thread::sleep(Duration::from_millis(delay));
    }
    restore_default(&dev)?;
    println!("Sweep done, static restored");
    Ok(())
}

fn off_cmd() -> Result<()> {
    let dev = open_device()?;
    dev.enable_direct(false)?;
    dev.send_static(&[[0, 0, 0]; LED_COUNT], 6, 1)?;
    println!("LEDs off (static black)");
    Ok(())
}

fn restore_default(dev: &VulcanDevice) -> Result<()> {
    let path = default_config_path();
    let (colors, speed, brightness) = match Config::load(&path) {
        Ok(cfg) => {
            let profile = cfg
                .profile(&cfg.default_profile)
                .cloned()
                .unwrap_or_default();
            let mut out = [[0u8; 3]; LED_COUNT];
            effects::render(&profile, 0.0, &mut out);
            let speed = ((profile.speed / 5.0) * (SPEED_MAX as f32 - 1.0) + 1.0)
                .round()
                .clamp(1.0, SPEED_MAX as f32) as u8;
            let brightness = (profile.brightness * BRIGHTNESS_MAX as f32)
                .round()
                .clamp(1.0, BRIGHTNESS_MAX as f32) as u8;
            (out, speed, brightness)
        }
        Err(_) => (
            solid_colors(Rgb(0x00, 0xAE, 0xEF), 0.4),
            6,
            percent_to_static(40),
        ),
    };
    dev.restore(&colors, speed, brightness)
}

fn validate_config_cmd(args: &[String]) -> Result<()> {
    let path = opt(args, "--config")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_config_path);
    println!("Config: {}", path.display());
    let cfg = Config::load(&path)?;
    cfg.validate().map_err(Error::new)?;
    println!(
        "OK — version {}, default {:?}",
        cfg.version, cfg.default_profile
    );
    println!("Profiles:");
    for (name, p) in &cfg.profiles {
        println!(
            "  {:<16} {:<8} brightness {:.2} speed {:.2} keys {}",
            name,
            p.effect.as_str(),
            p.brightness,
            p.speed,
            p.keys.len()
        );
    }
    println!("App rules:");
    for r in &cfg.app_rules {
        println!("  {:<28} -> {}", r.process, r.profile);
    }
    Ok(())
}

fn quit_cmd() -> Result<()> {
    if !takeover::stop_daemon(Duration::from_secs(5))? {
        return Err(Error::new("vulcand is not running"));
    }
    println!("vulcand stopped");
    Ok(())
}

fn autostart_cmd(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("on") => {
            autostart::set_enabled(true)?;
            println!("autostart enabled for the current user (vulcand.exe)");
        }
        Some("off") => {
            autostart::set_enabled(false)?;
            println!("autostart disabled");
        }
        _ => return Err(Error::new("usage: vulcanctl autostart on|off")),
    }
    Ok(())
}

fn restore_cmd() -> Result<()> {
    let dev = open_device()?;
    restore_default(&dev)?;
    println!("Restored default profile as static");
    Ok(())
}
