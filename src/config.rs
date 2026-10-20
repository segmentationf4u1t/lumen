use crate::effects::Profile;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppRule {
    #[serde(default)]
    pub process: String,
    #[serde(default)]
    pub profile: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fps: default_fps(),
            debounce_ms: default_debounce_ms(),
            enabled: true,
        }
    }
}

fn default_fps() -> u32 {
    30
}

fn default_debounce_ms() -> u64 {
    800
}

fn default_true() -> bool {
    true
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

fn default_profile_name() -> String {
    "default".to_owned()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceSelector {
    /// Optional full HID path or substring. Useful with multiple keyboards.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_profile_name")]
    pub default_profile: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub app_rules: Vec<AppRule>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub device: DeviceSelector,
}

impl Default for Config {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert("default".to_owned(), Profile::default());
        Self {
            version: CONFIG_VERSION,
            default_profile: "default".to_owned(),
            profiles,
            app_rules: Vec::new(),
            settings: Settings::default(),
            device: DeviceSelector::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.version != CONFIG_VERSION {
            return Err(format!(
                "unsupported config version {}, expected {CONFIG_VERSION}",
                self.version
            ));
        }
        if self.profiles.is_empty() {
            return Err("at least one profile is required".to_owned());
        }
        if !self.profiles.contains_key(&self.default_profile) {
            return Err(format!(
                "default_profile {:?} does not exist",
                self.default_profile
            ));
        }
        for (name, profile) in &self.profiles {
            if name.trim().is_empty() {
                return Err("profile name cannot be empty".to_owned());
            }
            if name.chars().any(|c| c.is_control()) {
                return Err(format!("profile name {name:?} contains control characters"));
            }
            profile
                .validate()
                .map_err(|e| format!("profile {name:?}: {e}"))?;
        }
        for (idx, rule) in self.app_rules.iter().enumerate() {
            if rule.process.trim().is_empty() {
                return Err(format!("app_rules[{idx}].process is empty"));
            }
            if !self.profiles.contains_key(&rule.profile) {
                return Err(format!(
                    "app_rules[{idx}] points to missing profile {:?}",
                    rule.profile
                ));
            }
        }
        if !(10..=60).contains(&self.settings.fps) {
            return Err(format!(
                "settings.fps must be 10..60, got {}",
                self.settings.fps
            ));
        }
        if !(200..=5000).contains(&self.settings.debounce_ms) {
            return Err(format!(
                "settings.debounce_ms must be 200..5000, got {}",
                self.settings.debounce_ms
            ));
        }
        Ok(())
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }

    /// Current profile or the default, whichever exists.
    pub fn active_profile(&self, name: &str) -> (&str, &Profile) {
        if let Some((key, profile)) = self.profiles.get_key_value(name) {
            (key, profile)
        } else {
            let key = self
                .profiles
                .get_key_value(&self.default_profile)
                .map(|(k, _)| k.as_str())
                .unwrap_or_else(|| {
                    self.profiles
                        .keys()
                        .next()
                        .map(String::as_str)
                        .unwrap_or("default")
                });
            (key, &self.profiles[key])
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        let cfg: Config = serde_json::from_str(text)?;
        cfg.validate().map_err(Error::new)?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .map_err(|e| Error::new(format!("read {}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| Error::new(format!("{}: {e}", path.display())))
    }

    /// Load, or create the default config when the file does not exist.
    /// A corrupt existing file is never overwritten.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match Self::load(path) {
            Ok(cfg) => Ok(cfg),
            Err(_) if !path.exists() => {
                let cfg = Self::default();
                cfg.save_atomic(path)?;
                Ok(cfg)
            }
            Err(e) => Err(e),
        }
    }

    pub fn save_atomic(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::new(format!("create {}: {e}", parent.display())))?;
        }
        let text = serde_json::to_string_pretty(self)? + "\n";
        let tmp = path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = fs::File::create(&tmp)
                .map_err(|e| Error::new(format!("create {}: {e}", tmp.display())))?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        if path.exists() {
            let bak = path.with_extension("json.bak");
            let _ = fs::copy(path, bak);
        }
        fs::rename(&tmp, path).map_err(|e| {
            Error::new(format!(
                "rename {} -> {}: {e}",
                tmp.display(),
                path.display()
            ))
        })?;
        Ok(())
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("vulcan")
    } else if let Ok(exe) = std::env::current_exe() {
        exe.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(".")
    }
}

pub fn default_config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn log_path() -> PathBuf {
    config_dir().join("vulcand.log")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;
    use crate::effects::Effect;

    #[test]
    fn default_is_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn parse_full_example() {
        let text = r##"{
          "version": 1,
          "default_profile": "desktop",
          "profiles": {
            "desktop": {
              "effect": "wave",
              "color": "#00aaff",
              "secondary": "#ff00aa",
              "speed": 1.5,
              "brightness": 0.6,
              "keys": { "W": "#ff0000", "SPACE": "#00ff00" }
            },
            "fps": { "effect": "static", "color": "#ff6600", "brightness": 1.0 }
          },
          "app_rules": [{ "process": "cs2.exe", "profile": "fps" }],
          "settings": { "fps": 60, "debounce_ms": 500 }
        }"##;
        let cfg = Config::parse(text).unwrap();
        assert_eq!(cfg.profiles["desktop"].effect, Effect::Wave);
        assert_eq!(cfg.profiles["desktop"].keys["W"], Rgb(255, 0, 0));
        assert_eq!(cfg.app_rules[0].profile, "fps");
    }

    #[test]
    fn rejects_bad_profile_reference() {
        let mut cfg = Config::default();
        cfg.app_rules.push(AppRule {
            process: "game.exe".into(),
            profile: "missing".into(),
        });
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn atomic_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "vulcan-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("config.json");
        let mut cfg = Config::default();
        cfg.profiles.insert(
            "fps".into(),
            Profile {
                effect: Effect::Static,
                color: Rgb(1, 2, 3),
                ..Default::default()
            },
        );
        cfg.app_rules.push(AppRule {
            process: "game.exe".into(),
            profile: "fps".into(),
        });
        cfg.save_atomic(&path).unwrap();
        cfg.save_atomic(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.profiles["fps"].color, Rgb(1, 2, 3));
        assert!(path.with_extension("json.bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
