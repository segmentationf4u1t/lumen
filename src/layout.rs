//! Physical key layout for the 69-key Vulcan II Mini.
//!
//! Coordinates are keyboard units (1u = one standard key cap). The board is
//! 16u wide and 5u tall. This table is authored from the physical layout, not
//! generated from another controller's source.

#[derive(Clone, Copy, Debug)]
pub struct Key {
    /// Logical key index. This is also the index used by `Effect` overrides.
    pub index: u8,
    /// Stable config identifier (ASCII, upper case).
    pub name: &'static str,
    /// Short label used by diagnostics.
    pub label: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub const KEY_COUNT: usize = 69;
pub const BOARD_WIDTH: f32 = 16.0;
pub const BOARD_HEIGHT: f32 = 5.0;

const fn k(index: u8, name: &'static str, label: &'static str, x: f32, y: f32, w: f32) -> Key {
    Key {
        index,
        name,
        label,
        x,
        y,
        w,
        h: 1.0,
    }
}

/// Row-major physical table. Every key appears exactly once.
pub const KEYS: [Key; KEY_COUNT] = [
    // Row 0
    k(0, "ESC", "Esc", 0.0, 0.0, 1.0),
    k(6, "1", "1", 1.0, 0.0, 1.0),
    k(12, "2", "2", 2.0, 0.0, 1.0),
    k(13, "3", "3", 3.0, 0.0, 1.0),
    k(17, "4", "4", 4.0, 0.0, 1.0),
    k(22, "5", "5", 5.0, 0.0, 1.0),
    k(28, "6", "6", 6.0, 0.0, 1.0),
    k(29, "7", "7", 7.0, 0.0, 1.0),
    k(34, "8", "8", 8.0, 0.0, 1.0),
    k(39, "9", "9", 9.0, 0.0, 1.0),
    k(43, "0", "0", 10.0, 0.0, 1.0),
    k(49, "MINUS", "-", 11.0, 0.0, 1.0),
    k(55, "EQUAL", "=", 12.0, 0.0, 1.0),
    k(60, "BACKSPACE", "Bksp", 13.0, 0.0, 2.0),
    k(64, "DEL", "Del", 15.0, 0.0, 1.0),
    // Row 1
    k(1, "TAB", "Tab", 0.0, 1.0, 1.5),
    k(7, "Q", "Q", 1.5, 1.0, 1.0),
    k(14, "W", "W", 2.5, 1.0, 1.0),
    k(18, "E", "E", 3.5, 1.0, 1.0),
    k(19, "R", "R", 4.5, 1.0, 1.0),
    k(23, "T", "T", 5.5, 1.0, 1.0),
    k(30, "Y", "Y", 6.5, 1.0, 1.0),
    k(36, "U", "U", 7.5, 1.0, 1.0),
    k(35, "I", "I", 8.5, 1.0, 1.0),
    k(40, "O", "O", 9.5, 1.0, 1.0),
    k(44, "P", "P", 10.5, 1.0, 1.0),
    k(50, "LBRACKET", "[", 11.5, 1.0, 1.0),
    k(56, "RBRACKET", "]", 12.5, 1.0, 1.0),
    k(61, "BACKSLASH", "\\", 13.5, 1.0, 1.5),
    k(65, "HOME", "Home", 15.0, 1.0, 1.0),
    // Row 2
    k(2, "CAPS", "Caps", 0.0, 2.0, 1.75),
    k(8, "A", "A", 1.75, 2.0, 1.0),
    k(15, "S", "S", 2.75, 2.0, 1.0),
    k(20, "D", "D", 3.75, 2.0, 1.0),
    k(24, "F", "F", 4.75, 2.0, 1.0),
    k(25, "G", "G", 5.75, 2.0, 1.0),
    k(31, "H", "H", 6.75, 2.0, 1.0),
    k(37, "J", "J", 7.75, 2.0, 1.0),
    k(41, "K", "K", 8.75, 2.0, 1.0),
    k(45, "L", "L", 9.75, 2.0, 1.0),
    k(46, "SEMICOLON", ";", 10.75, 2.0, 1.0),
    k(51, "QUOTE", "'", 11.75, 2.0, 1.0),
    k(57, "ENTER", "Enter", 12.75, 2.0, 2.25),
    k(66, "PGUP", "PgUp", 15.0, 2.0, 1.0),
    // Row 3
    k(3, "LSHIFT", "Shift", 0.0, 3.0, 1.25),
    k(5, "INTL_BACKSLASH", "\\", 1.25, 3.0, 1.0),
    k(9, "Z", "Z", 2.25, 3.0, 1.0),
    k(16, "X", "X", 3.25, 3.0, 1.0),
    k(21, "C", "C", 4.25, 3.0, 1.0),
    k(26, "V", "V", 5.25, 3.0, 1.0),
    k(27, "B", "B", 6.25, 3.0, 1.0),
    k(32, "N", "N", 7.25, 3.0, 1.0),
    k(38, "M", "M", 8.25, 3.0, 1.0),
    k(42, "COMMA", ",", 9.25, 3.0, 1.0),
    k(47, "PERIOD", ".", 10.25, 3.0, 1.0),
    k(52, "SLASH", "/", 11.25, 3.0, 1.0),
    k(58, "RSHIFT", "Shift", 12.25, 3.0, 1.75),
    k(62, "UP", "Up", 14.0, 3.0, 1.0),
    k(67, "PGDN", "PgDn", 15.0, 3.0, 1.0),
    // Row 4
    k(4, "LCTRL", "Ctrl", 0.0, 4.0, 1.25),
    k(10, "LWIN", "Win", 1.25, 4.0, 1.25),
    k(11, "LALT", "Alt", 2.5, 4.0, 1.25),
    k(33, "SPACE", "Space", 3.75, 4.0, 6.25),
    k(48, "RALT", "Alt", 10.0, 4.0, 1.0),
    k(53, "FN", "Fn", 11.0, 4.0, 1.0),
    k(54, "RCTRL", "Ctrl", 12.0, 4.0, 1.0),
    k(59, "LEFT", "Left", 13.0, 4.0, 1.0),
    k(63, "DOWN", "Down", 14.0, 4.0, 1.0),
    k(68, "RIGHT", "Right", 15.0, 4.0, 1.0),
];

pub fn key(index: usize) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.index as usize == index)
}

pub fn by_name(name: &str) -> Option<&'static Key> {
    let name = name.trim();
    KEYS.iter().find(|k| k.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn layout_is_complete_and_unique() {
        assert_eq!(KEYS.len(), KEY_COUNT);
        let mut indices = HashSet::new();
        let mut names = HashSet::new();
        for k in KEYS {
            assert!(indices.insert(k.index), "duplicate index {}", k.index);
            assert!(names.insert(k.name), "duplicate name {}", k.name);
        }
        for i in 0..KEY_COUNT {
            assert!(indices.contains(&(i as u8)), "missing index {i}");
        }
    }

    #[test]
    fn layout_fits_board() {
        for k in KEYS {
            assert!(k.x >= 0.0 && k.y >= 0.0);
            assert!(k.x + k.w <= BOARD_WIDTH + 0.001, "{} overflows", k.name);
            assert!(k.y + k.h <= BOARD_HEIGHT + 0.001, "{} overflows", k.name);
        }
    }

    #[test]
    fn names_are_stable_lookup() {
        assert_eq!(by_name("w").unwrap().index, 14);
        assert_eq!(by_name("SPACE").unwrap().index, 33);
        assert!(by_name("nope").is_none());
    }
}
