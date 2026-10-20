use crate::config::Config;
use crate::effects::{Effect, Profile};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// What the user picked in the tray. Persisted to `state.json` so it survives
/// restarts. Focus changes never modify it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    #[serde(default)]
    pub profile: Option<String>,
    /// Animation override for the selected profile.
    #[serde(default)]
    pub effect: Option<Effect>,
    /// Global brightness override, 0..1.
    #[serde(default)]
    pub brightness: Option<f32>,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Default)]
struct Runtime {
    selection: Selection,
    /// (process, profile) of a matching app rule while that app has focus.
    app: Option<(String, String)>,
}

/// Fully resolved lighting for the next frame.
#[derive(Clone, Debug)]
pub struct Active {
    pub profile_name: String,
    pub profile: Profile,
    pub effect: Effect,
    pub brightness: f32,
    /// Set when an app rule currently drives the lighting.
    pub app: Option<String>,
}

pub struct Shared {
    config: RwLock<Arc<Config>>,
    config_path: PathBuf,
    state_path: PathBuf,
    log_path: PathBuf,
    runtime: Mutex<Runtime>,
    enabled: AtomicBool,
    shutdown: AtomicBool,
    pub tray_hwnd: AtomicIsize,
    status: Mutex<String>,
}

impl Shared {
    pub fn new(config: Config, config_path: PathBuf) -> Arc<Self> {
        let dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let state_path = dir.join("state.json");
        let mut selection: Selection = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        sanitize(&mut selection, &config);
        let enabled = config.settings.enabled && !selection.paused;
        Arc::new(Self {
            config: RwLock::new(Arc::new(config)),
            config_path,
            state_path,
            log_path: dir.join("vulcand.log"),
            runtime: Mutex::new(Runtime {
                selection,
                app: None,
            }),
            enabled: AtomicBool::new(enabled),
            shutdown: AtomicBool::new(false),
            tray_hwnd: AtomicIsize::new(0),
            status: Mutex::new("starting".to_owned()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Runtime> {
        self.runtime.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> Arc<Config> {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn replace_config(&self, config: Config) {
        {
            let mut runtime = self.lock();
            sanitize(&mut runtime.selection, &config);
            if let Some((_, profile)) = &runtime.app {
                if config.profile(profile).is_none() {
                    runtime.app = None;
                }
            }
        }
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(config);
    }

    /// Resolve what should be rendered right now.
    pub fn active(&self) -> Active {
        let cfg = self.config();
        let runtime = self.lock();
        let sel = &runtime.selection;
        let base = sel.profile.as_deref().unwrap_or(&cfg.default_profile);

        let (name, profile_effect_override, app) = match &runtime.app {
            Some((process, profile)) => (profile.as_str(), None, Some(process.clone())),
            None => (base, sel.effect, None),
        };
        let (name, profile) = cfg.active_profile(name);
        Active {
            profile_name: name.to_owned(),
            effect: profile_effect_override.unwrap_or(profile.effect),
            brightness: sel.brightness.unwrap_or(profile.brightness),
            profile: profile.clone(),
            app,
        }
    }

    pub fn selection(&self) -> Selection {
        self.lock().selection.clone()
    }

    /// Choose a base profile. Clears the animation override.
    pub fn select_profile(&self, name: &str) -> bool {
        if self.config().profile(name).is_none() {
            self.log(&format!("profile {name:?} does not exist"));
            return false;
        }
        self.update(|sel| {
            sel.profile = Some(name.to_owned());
            sel.effect = None;
        })
    }

    pub fn select_effect(&self, effect: Option<Effect>) -> bool {
        self.update(|sel| sel.effect = effect)
    }

    pub fn select_brightness(&self, brightness: Option<f32>) -> bool {
        let brightness = brightness
            .filter(|b| b.is_finite())
            .map(|b| b.clamp(0.05, 1.0));
        self.update(|sel| sel.brightness = brightness)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
        self.update(|sel| sel.paused = !enabled);
    }

    /// Record the app-rule match for the focused process. Returns true when
    /// the effective lighting source changed.
    pub fn set_app_match(&self, matched: Option<(String, String)>) -> bool {
        let mut runtime = self.lock();
        if runtime.app == matched {
            return false;
        }
        runtime.app = matched;
        true
    }

    fn update(&self, change: impl FnOnce(&mut Selection)) -> bool {
        let snapshot = {
            let mut runtime = self.lock();
            let before = runtime.selection.clone();
            change(&mut runtime.selection);
            if runtime.selection == before {
                return false;
            }
            runtime.selection.clone()
        };
        self.save_state(&snapshot);
        true
    }

    fn save_state(&self, selection: &Selection) {
        let Ok(text) = serde_json::to_string_pretty(selection) else {
            return;
        };
        let tmp = self.state_path.with_extension("json.tmp");
        let result = std::fs::write(&tmp, text + "\n")
            .and_then(|()| std::fs::rename(&tmp, &self.state_path));
        if let Err(e) = result {
            self.log(&format!("save state failed: {e}"));
        }
    }

    pub fn set_status(&self, status: impl Into<String>) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status.into();
    }

    pub fn status(&self) -> String {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn tip(&self) -> String {
        if !self.is_enabled() {
            return "lumen — paused".to_owned();
        }
        let active = self.active();
        let source = match &active.app {
            Some(process) => format!("{} ({process})", active.profile_name),
            None => active.profile_name.clone(),
        };
        let status = self.status();
        let suffix = if status == "connected" {
            String::new()
        } else {
            format!("\n{status}")
        };
        format!(
            "lumen — {}\n{} · {}%{}",
            active.effect.as_str(),
            source,
            (active.brightness * 100.0).round(),
            suffix
        )
    }

    pub fn log(&self, msg: &str) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        #[cfg(debug_assertions)]
        eprintln!("[{secs}] {msg}");
        if let Some(parent) = self.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(f, "[{secs}] {msg}");
        }
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}

/// Drop selections that no longer make sense for `config`.
fn sanitize(selection: &mut Selection, config: &Config) {
    if let Some(name) = &selection.profile {
        if config.profile(name).is_none() {
            selection.profile = None;
            selection.effect = None;
        }
    }
    selection.brightness = selection
        .brightness
        .filter(|b| b.is_finite())
        .map(|b| b.clamp(0.05, 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppRule;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vulcan-runtime-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config_with_showcase_default() -> Config {
        let mut cfg = Config::default();
        cfg.profiles.insert(
            "showcase".into(),
            Profile {
                effect: Effect::Showcase,
                ..Default::default()
            },
        );
        cfg.profiles.insert(
            "fps".into(),
            Profile {
                effect: Effect::Static,
                ..Default::default()
            },
        );
        cfg.default_profile = "showcase".into();
        cfg.app_rules.push(AppRule {
            process: "cs2.exe".into(),
            profile: "fps".into(),
        });
        cfg
    }

    #[test]
    fn tray_choice_survives_focus_changes_without_rules() {
        let dir = temp_dir("focus");
        let shared = Shared::new(config_with_showcase_default(), dir.join("config.json"));
        shared.select_profile("default");
        shared.select_effect(Some(Effect::Plasma));

        // Focus moves between apps that have no rule: nothing may change.
        assert!(!shared.set_app_match(None));
        let active = shared.active();
        assert_eq!(active.profile_name, "default");
        assert_eq!(active.effect, Effect::Plasma);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn app_rule_overlays_and_then_restores_tray_choice() {
        let dir = temp_dir("rule");
        let shared = Shared::new(config_with_showcase_default(), dir.join("config.json"));
        shared.select_profile("default");
        shared.select_effect(Some(Effect::Plasma));

        assert!(shared.set_app_match(Some(("cs2.exe".into(), "fps".into()))));
        let active = shared.active();
        assert_eq!(active.profile_name, "fps");
        assert_eq!(active.effect, Effect::Static);
        assert_eq!(active.app.as_deref(), Some("cs2.exe"));

        assert!(shared.set_app_match(None));
        let active = shared.active();
        assert_eq!(active.profile_name, "default");
        assert_eq!(active.effect, Effect::Plasma);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn selection_persists_across_restarts() {
        let dir = temp_dir("persist");
        let path = dir.join("config.json");
        {
            let shared = Shared::new(config_with_showcase_default(), path.clone());
            shared.select_profile("default");
            shared.select_effect(Some(Effect::Fire));
            shared.select_brightness(Some(0.5));
        }
        let shared = Shared::new(config_with_showcase_default(), path);
        let active = shared.active();
        assert_eq!(active.profile_name, "default");
        assert_eq!(active.effect, Effect::Fire);
        assert_eq!(active.brightness, 0.5);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn choosing_a_profile_clears_the_animation_override() {
        let dir = temp_dir("clear");
        let shared = Shared::new(config_with_showcase_default(), dir.join("config.json"));
        shared.select_effect(Some(Effect::Comet));
        shared.select_profile("default");
        assert_eq!(shared.active().effect, Effect::Wave);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_state_profile_falls_back_to_config_default() {
        let dir = temp_dir("stale");
        std::fs::write(
            dir.join("state.json"),
            r#"{ "profile": "deleted", "effect": "fire" }"#,
        )
        .unwrap();
        let shared = Shared::new(config_with_showcase_default(), dir.join("config.json"));
        let active = shared.active();
        assert_eq!(active.profile_name, "showcase");
        assert_eq!(active.effect, Effect::Showcase);
        let _ = std::fs::remove_dir_all(dir);
    }
}
