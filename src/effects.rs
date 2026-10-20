use crate::color::{hsv, Rgb};
use crate::layout::{self, BOARD_WIDTH, KEY_COUNT};
use crate::protocol::LED_COUNT;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Static,
    Breathe,
    #[default]
    Wave,
    Spectrum,
    Plasma,
    Comet,
    Aurora,
    Scanner,
    Fire,
    /// Cycles through six bright animations with smooth crossfades.
    Showcase,
}

impl Effect {
    pub fn all() -> &'static [Effect] {
        &[
            Effect::Static,
            Effect::Breathe,
            Effect::Wave,
            Effect::Spectrum,
            Effect::Plasma,
            Effect::Comet,
            Effect::Aurora,
            Effect::Scanner,
            Effect::Fire,
            Effect::Showcase,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Static => "static",
            Effect::Breathe => "breathe",
            Effect::Wave => "wave",
            Effect::Spectrum => "spectrum",
            Effect::Plasma => "plasma",
            Effect::Comet => "comet",
            Effect::Aurora => "aurora",
            Effect::Scanner => "scanner",
            Effect::Fire => "fire",
            Effect::Showcase => "showcase",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Effect::all()
            .iter()
            .copied()
            .find(|e| e.as_str().eq_ignore_ascii_case(s))
    }
}

fn default_color() -> Rgb {
    Rgb(0x00, 0xAE, 0xEF)
}

fn default_secondary() -> Rgb {
    Rgb(0xFF, 0x2A, 0xAA)
}

fn default_speed() -> f32 {
    1.0
}

fn default_brightness() -> f32 {
    0.7
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub effect: Effect,
    #[serde(default = "default_color")]
    pub color: Rgb,
    #[serde(default = "default_secondary")]
    pub secondary: Rgb,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    /// Optional per-key override, keyed by `layout::Key::name`.
    #[serde(default)]
    pub keys: BTreeMap<String, Rgb>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            effect: Effect::Wave,
            color: default_color(),
            secondary: default_secondary(),
            speed: default_speed(),
            brightness: default_brightness(),
            keys: BTreeMap::new(),
        }
    }
}

impl Profile {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.speed.is_finite() || !(0.1..=5.0).contains(&self.speed) {
            return Err(format!("speed must be 0.1..5.0, got {}", self.speed));
        }
        if !self.brightness.is_finite() || !(0.0..=1.0).contains(&self.brightness) {
            return Err(format!(
                "brightness must be 0.0..1.0, got {}",
                self.brightness
            ));
        }
        let mut seen: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
        for name in self.keys.keys() {
            let Some(key) = layout::by_name(name) else {
                return Err(format!("unknown key name {name:?}"));
            };
            let normalized = key.name.to_ascii_lowercase();
            if let Some(previous) = seen.insert(normalized, name) {
                return Err(format!(
                    "keys {previous:?} and {name:?} refer to the same key"
                ));
            }
        }
        Ok(())
    }

    /// Case-insensitive lookup, matching `validate`/`layout::by_name`.
    fn override_for(&self, name: &str) -> Option<Rgb> {
        self.keys.get(name).copied().or_else(|| {
            self.keys
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, color)| *color)
        })
    }
}

const SHOWCASE_EFFECTS: [Effect; 6] = [
    Effect::Plasma,
    Effect::Comet,
    Effect::Aurora,
    Effect::Scanner,
    Effect::Fire,
    Effect::Spectrum,
];
const SHOWCASE_SEGMENT: f32 = 6.0;
const SHOWCASE_FADE: f32 = 0.9;

/// Render one frame into `out`. Allocation-free and bounded by construction.
pub fn render(profile: &Profile, t: f32, out: &mut [[u8; 3]; LED_COUNT]) {
    render_with(profile, profile.effect, profile.brightness, t, out);
}

/// Same as `render`, with the effect and brightness supplied by the caller
/// (tray overrides) instead of read from the profile.
pub fn render_with(
    profile: &Profile,
    effect: Effect,
    brightness: f32,
    t: f32,
    out: &mut [[u8; 3]; LED_COUNT],
) {
    let t = if t.is_finite() { t.max(0.0) } else { 0.0 };
    let speed = if profile.speed.is_finite() {
        profile.speed.clamp(0.1, 5.0)
    } else {
        1.0
    };
    let brightness = if brightness.is_finite() {
        brightness.clamp(0.0, 1.0)
    } else {
        0.0
    };

    if effect == Effect::Showcase {
        render_showcase(profile, t, speed, out);
    } else {
        render_one(profile, effect, t, speed, out);
    }

    // Global brightness is applied once, after the effect/crossfade, so the
    // same curve is used for every sub-effect.
    if brightness != 1.0 {
        for rgb in out.iter_mut() {
            for channel in rgb.iter_mut() {
                *channel = (*channel as f32 * brightness).round() as u8;
            }
        }
    }

    // Per-key overrides apply after the effect (and after a crossfade).
    for key in layout::KEYS.iter().take(KEY_COUNT) {
        if let Some(over) = profile.override_for(key.name) {
            let rgb = over.scale(brightness);
            out[key.index as usize] = [rgb.0, rgb.1, rgb.2];
        }
    }
}

fn render_one(
    profile: &Profile,
    effect: Effect,
    t: f32,
    speed: f32,
    out: &mut [[u8; 3]; LED_COUNT],
) {
    for key in layout::KEYS.iter().take(KEY_COUNT) {
        let x = (key.x / BOARD_WIDTH).clamp(0.0, 1.0);
        let y = (key.y / 5.0).clamp(0.0, 1.0);
        let base = match effect {
            Effect::Static => profile.color,
            Effect::Breathe => {
                let phase = t * speed * 0.5;
                let factor = 0.10 + 0.90 * (0.5 + 0.5 * (TAU * phase).sin());
                profile.color.scale(factor)
            }
            Effect::Wave => {
                let phase = x * 0.65 - t * speed * 0.08;
                let mix = 0.5 + 0.5 * (TAU * phase).sin();
                profile.color.lerp(profile.secondary, mix)
            }
            Effect::Spectrum => {
                let hue = t * speed * 0.05 + x * 0.35 + y * 0.03;
                hsv(hue, 1.0, 1.0)
            }
            Effect::Plasma => {
                // Three interfering waves make a smooth, full-saturation field.
                let v = (x * 9.0 + t * speed * 0.8).sin()
                    + (y * 7.0 - t * speed * 0.6).sin()
                    + ((x + y) * 6.0 + t * speed * 1.1).sin();
                let hue = (v * 0.5 / 3.0 + 0.5 + t * speed * 0.015).rem_euclid(1.0);
                hsv(hue, 1.0, 1.0)
            }
            Effect::Comet => {
                // A bright white-core comet with a coloured tail, looping.
                let head = (t * speed * 0.35).rem_euclid(1.0) * (BOARD_WIDTH + 3.0) - 1.5;
                let d = (key.x - head).abs();
                let tail = (-d * 1.15).exp();
                let hue = (0.55 + t * speed * 0.06).rem_euclid(1.0);
                let core = if d < 0.8 {
                    hsv(hue, 0.25, 1.0)
                } else {
                    hsv(hue, 1.0, 1.0)
                };
                core.scale(tail)
            }
            Effect::Aurora => {
                // Flowing vertical curtains; cyan/green/magenta bands.
                let band = (y * 3.2 + (x * 5.0 + t * speed * 0.7).sin() * 0.35).sin();
                let hue = (x * 0.55 + band * 0.12 + t * speed * 0.02).rem_euclid(1.0);
                let value = 0.72 + 0.28 * (band * 0.5 + 0.5);
                hsv(hue, 0.95, value)
            }
            Effect::Scanner => {
                // Bouncing bright scanner with a short trail.
                let phase = (t * speed * 0.30).rem_euclid(2.0);
                let pos = if phase < 1.0 { phase } else { 2.0 - phase } * (BOARD_WIDTH - 1.0);
                let d = (key.x - pos).abs();
                let trail = (-d * 1.5).exp();
                let color = if d < 0.55 {
                    Rgb::WHITE
                } else {
                    hsv(0.52 + d * 0.015, 0.9, 1.0)
                };
                color.scale(0.06 + 0.94 * trail)
            }
            Effect::Fire => {
                // Warm, smooth flicker (no hard strobe).
                let flicker =
                    (x * 17.0 + t * speed * 2.1).sin() * (y * 11.0 - t * speed * 1.3).sin();
                let intensity = (0.72 + 0.28 * flicker).clamp(0.35, 1.0);
                let hue = 0.015 + 0.10 * (1.0 - intensity);
                hsv(hue, 1.0, 1.0).scale(intensity)
            }
            Effect::Showcase => profile.color,
        };
        out[key.index as usize] = [base.0, base.1, base.2];
    }
}

fn render_showcase(profile: &Profile, t: f32, speed: f32, out: &mut [[u8; 3]; LED_COUNT]) {
    let count = SHOWCASE_EFFECTS.len();
    let total = SHOWCASE_SEGMENT * count as f32;
    let t = t.rem_euclid(total);
    let index = ((t / SHOWCASE_SEGMENT) as usize).min(count - 1);
    let local = t - index as f32 * SHOWCASE_SEGMENT;

    render_one(profile, SHOWCASE_EFFECTS[index], t, speed, out);

    if local > SHOWCASE_SEGMENT - SHOWCASE_FADE {
        let next = (index + 1) % count;
        let mut other = [[0u8; 3]; LED_COUNT];
        render_one(profile, SHOWCASE_EFFECTS[next], t, speed, &mut other);
        let fade = (local - (SHOWCASE_SEGMENT - SHOWCASE_FADE)) / SHOWCASE_FADE;
        for (a, b) in out.iter_mut().zip(other.iter()) {
            for c in 0..3 {
                a[c] = (a[c] as f32 + (b[c] as f32 - a[c] as f32) * fade).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(p: &Profile, t: f32) -> [[u8; 3]; LED_COUNT] {
        let mut out = [[0u8; 3]; LED_COUNT];
        render(p, t, &mut out);
        out
    }

    #[test]
    fn static_is_exact_and_ignores_time() {
        let p = Profile {
            effect: Effect::Static,
            color: Rgb(10, 20, 30),
            brightness: 1.0,
            ..Default::default()
        };
        assert_eq!(frame(&p, 0.0), [[10, 20, 30]; LED_COUNT]);
        assert_eq!(frame(&p, 123.4), [[10, 20, 30]; LED_COUNT]);
    }

    #[test]
    fn zero_brightness_is_black() {
        let p = Profile {
            effect: Effect::Spectrum,
            brightness: 0.0,
            ..Default::default()
        };
        assert_eq!(frame(&p, 3.0), [[0, 0, 0]; LED_COUNT]);
    }

    #[test]
    fn per_key_override_wins() {
        let p = Profile {
            effect: Effect::Static,
            color: Rgb(0, 0, 0),
            brightness: 1.0,
            keys: [(("W").to_string(), Rgb(255, 0, 0))].into_iter().collect(),
            ..Default::default()
        };
        let out = frame(&p, 0.0);
        let w = layout::by_name("W").unwrap().index as usize;
        assert_eq!(out[w], [255, 0, 0]);
        assert_eq!(out[layout::by_name("Q").unwrap().index as usize], [0, 0, 0]);
    }

    #[test]
    fn effect_override_replaces_profile_effect() {
        let p = Profile {
            effect: Effect::Static,
            color: Rgb::BLACK,
            brightness: 1.0,
            ..Default::default()
        };
        let mut out = [[0u8; 3]; LED_COUNT];
        render_with(&p, Effect::Spectrum, 1.0, 0.0, &mut out);
        assert!(out.iter().flatten().any(|&v| v > 0));
    }

    #[test]
    fn brightness_scales_after_effect() {
        let p = Profile {
            effect: Effect::Static,
            color: Rgb::WHITE,
            brightness: 0.5,
            ..Default::default()
        };
        assert_eq!(frame(&p, 0.0), [[128, 128, 128]; LED_COUNT]);
    }

    #[test]
    fn lowercase_per_key_override_applies() {
        let p = Profile {
            effect: Effect::Static,
            color: Rgb(0, 0, 0),
            brightness: 1.0,
            keys: [("w".to_string(), Rgb(255, 0, 0))].into_iter().collect(),
            ..Default::default()
        };
        p.validate().unwrap();
        let out = frame(&p, 0.0);
        let w = layout::by_name("W").unwrap().index as usize;
        assert_eq!(out[w], [255, 0, 0]);
    }

    #[test]
    fn showcase_cycles_and_loops() {
        let p = Profile {
            effect: Effect::Showcase,
            brightness: 1.0,
            speed: 1.0,
            ..Default::default()
        };
        let start = frame(&p, 0.0);
        let second = frame(&p, 6.0);
        let wrapped = frame(&p, 36.0);
        assert_ne!(start, second);
        assert_eq!(start, wrapped);
        assert!(start.iter().flatten().any(|&v| v > 0));
    }

    #[test]
    fn new_effects_are_bright() {
        for effect in [
            Effect::Plasma,
            Effect::Comet,
            Effect::Aurora,
            Effect::Scanner,
            Effect::Fire,
        ] {
            let p = Profile {
                effect,
                brightness: 1.0,
                speed: 1.0,
                ..Default::default()
            };
            let out = frame(&p, 3.0);
            let max = out.iter().flatten().copied().max().unwrap_or(0);
            assert!(max > 100, "{effect:?} is too dark: max={max}");
        }
    }

    #[test]
    fn all_effects_stay_in_bounds_and_change_over_time() {
        for effect in Effect::all() {
            let p = Profile {
                effect: *effect,
                brightness: 1.0,
                speed: 2.0,
                ..Default::default()
            };
            let a = frame(&p, 0.0);
            let b = frame(&p, 0.37);
            if !matches!(effect, Effect::Static) {
                assert_ne!(a, b, "{effect:?} should animate");
            }
        }
    }

    #[test]
    fn profile_validation_rejects_bad_values_and_keys() {
        let bad_speed = Profile {
            speed: 0.0,
            ..Default::default()
        };
        assert!(bad_speed.validate().is_err());
        let bad_brightness = Profile {
            brightness: f32::NAN,
            ..Default::default()
        };
        assert!(bad_brightness.validate().is_err());
        let mut bad_key = Profile {
            brightness: 0.5,
            ..Default::default()
        };
        bad_key.keys.insert("NOPE".into(), Rgb::WHITE);
        assert!(bad_key.validate().is_err());

        let mut duplicate_case = Profile {
            brightness: 0.5,
            ..Default::default()
        };
        duplicate_case.keys.insert("w".into(), Rgb::WHITE);
        duplicate_case.keys.insert("W".into(), Rgb::BLACK);
        assert!(duplicate_case.validate().is_err());
    }
}
