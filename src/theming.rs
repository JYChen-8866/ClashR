//! Theme discovery, switching, and user-preference persistence.
//!
//! ClashR ships with a folder of `.json` themes under `./themes/` (compatible
//! with gpui-component's `ThemeSet` schema). At startup we register them
//! with the gpui-component theme registry; at runtime the Settings page
//! lets the user pick one, and we persist the choice to
//! `data/preferences.json`.

use std::path::PathBuf;

use gpui::{App, Window};
use gpui_component::{Theme, ThemeMode, ThemeRegistry};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::core::paths;

/// Persisted UI preferences (currently just the theme name).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Preferences {
    pub theme_name: Option<String>,
    #[serde(default)]
    pub system_proxy_enabled: bool,
    #[serde(default)]
    pub language: Option<String>,
}

impl Preferences {
    fn path() -> PathBuf {
        paths::data_dir().join("preferences.json")
    }

    pub fn load() -> Self {
        let p = Self::path();
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(prefs) = serde_json::from_str::<Preferences>(&s) {
                return prefs;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let p = Self::path();
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&p, s);
        }
    }
}

/// Find the `themes/` directory — in a .app bundle it lives under
/// Contents/Resources/; in dev it's relative to cwd.
fn themes_dir() -> PathBuf {
    crate::core::paths::resources_dir().join("themes")
}

/// Scan `./themes/*.json` and register each with the theme registry.
pub fn load_bundled_themes(cx: &mut App) {
    let dir = themes_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => {
            info!(path = %dir.display(), "themes directory not present");
            return;
        }
    };

    let registry = ThemeRegistry::global_mut(cx);
    let mut loaded = 0usize;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match registry.load_themes_from_str(&content) {
                Ok(()) => {
                    loaded += 1;
                }
                Err(e) => warn!(path = %path.display(), error = %e, "failed to parse theme"),
            },
            Err(e) => warn!(path = %path.display(), error = %e, "failed to read theme"),
        }
    }

    info!(count = loaded, "bundled themes registered");
}

/// All registered theme names, sorted.
pub fn list_theme_names(cx: &App) -> Vec<String> {
    let mut names: Vec<String> = ThemeRegistry::global(cx)
        .themes()
        .keys()
        .map(|s| s.to_string())
        .collect();
    names.sort();
    names
}

/// Apply a theme by its registered name, refreshing the window.
/// Returns true if the theme was found and applied.
pub fn apply_theme_by_name(name: &str, window: &mut Window, cx: &mut App) -> bool {
    let cfg = ThemeRegistry::global(cx)
        .themes()
        .get(name)
        .cloned();

    let Some(cfg) = cfg else {
        warn!(name, "requested theme not found in registry");
        return false;
    };

    let theme = Theme::global_mut(cx);
    theme.apply_config(&cfg);
    window.refresh();
    info!(name, "theme applied");
    true
}

/// Apply the user's saved theme on startup, if any. No-op if the saved name
/// is missing or no longer in the registry.
pub fn apply_saved_theme_or_default(window: &mut Window, cx: &mut App) {
    let prefs = Preferences::load();
    if let Some(name) = prefs.theme_name.as_deref() {
        if apply_theme_by_name(name, window, cx) {
            return;
        }
    }
    // Fall back to the registry's default light theme so the app still has
    // a usable look on first run.
    let default_cfg = ThemeRegistry::global(cx).default_light_theme().clone();
    let theme = Theme::global_mut(cx);
    theme.mode = ThemeMode::Light;
    theme.apply_config(&default_cfg);
    window.refresh();
}
