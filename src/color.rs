use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// sRGB triplet. Serialized as `#rrggbb`.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        let s = s.trim().strip_prefix('#').unwrap_or(s.trim());
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("expected #rrggbb, got {s:?}"));
        }
        let r = u8::from_str_radix(&s[0..2], 16).map_err(|e| e.to_string())?;
        let g = u8::from_str_radix(&s[2..4], 16).map_err(|e| e.to_string())?;
        let b = u8::from_str_radix(&s[4..6], 16).map_err(|e| e.to_string())?;
        Ok(Rgb(r, g, b))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    pub fn scale(self, factor: f32) -> Self {
        let f = if factor.is_finite() {
            factor.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Rgb(
            (self.0 as f32 * f).round() as u8,
            (self.1 as f32 * f).round() as u8,
            (self.2 as f32 * f).round() as u8,
        )
    }

    pub fn lerp(self, other: Rgb, t: f32) -> Self {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(
            mix(self.0, other.0),
            mix(self.1, other.1),
            mix(self.2, other.2),
        )
    }
}

impl fmt::Debug for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// HSV (h in turns, s/v in 0..1) to sRGB.
pub fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let h = if h.is_finite() {
        h.rem_euclid(1.0)
    } else {
        0.0
    };
    let s = if s.is_finite() {
        s.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };

    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);

    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgb(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        for s in ["#000000", "#ffffff", "#12A0ff", "00ff00"] {
            let c = Rgb::parse(s).unwrap();
            assert_eq!(Rgb::parse(&c.to_hex()).unwrap(), c);
        }
        assert!(Rgb::parse("#12345").is_err());
        assert!(Rgb::parse("#gggggg").is_err());
    }

    #[test]
    fn scale_clamps() {
        assert_eq!(Rgb(255, 100, 0).scale(-1.0), Rgb::BLACK);
        assert_eq!(Rgb(255, 100, 0).scale(0.5), Rgb(128, 50, 0));
        assert_eq!(Rgb(255, 100, 0).scale(9.0), Rgb(255, 100, 0));
    }

    #[test]
    fn hsv_primaries() {
        assert_eq!(hsv(0.0, 1.0, 1.0), Rgb(255, 0, 0));
        assert_eq!(hsv(1.0 / 3.0, 1.0, 1.0), Rgb(0, 255, 0));
        assert_eq!(hsv(2.0 / 3.0, 1.0, 1.0), Rgb(0, 0, 255));
        assert_eq!(hsv(0.0, 0.0, 0.0), Rgb::BLACK);
    }
}
