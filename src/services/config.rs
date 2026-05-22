use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub clash_core: ClashCoreConfig,
    pub system: SystemConfig,
    pub appearance: AppearanceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClashCoreConfig {
    pub mixed_port: u16,
    pub external_controller: String,
    pub secret: Option<String>,
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub system_proxy: bool,
    pub tun_mode: bool,
    pub auto_launch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    pub theme: String,
    pub language: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            clash_core: ClashCoreConfig {
                mixed_port: 7890,
                external_controller: "127.0.0.1:9097".to_string(),
                secret: None,
                config_path: None,
            },
            system: SystemConfig {
                system_proxy: false,
                tun_mode: false,
                auto_launch: false,
            },
            appearance: AppearanceConfig {
                theme: "dark".to_string(),
                language: "en".to_string(),
            },
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> PathBuf {
        dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("clashr")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        })?;
        std::fs::write(Self::config_path(), json)
    }
}
