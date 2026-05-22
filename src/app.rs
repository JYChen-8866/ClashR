use crate::services::config::AppConfig;

pub struct AppState {
    pub config: AppConfig,
}

impl AppState {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            config: AppConfig::load(),
        }
    }
}
