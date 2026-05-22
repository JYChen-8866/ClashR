use std::borrow::Cow;
use std::path::PathBuf;

use gpui::{AssetSource, Result, SharedString};
use gpui_component_assets::Assets as ComponentAssets;

/// Combined asset source.
///
/// Routing:
///   - "service-icons/<name>" → `<cwd>/icons/<name>` on disk
///   - everything else → gpui-component bundled assets
pub struct CombinedAssets {
    icons_dir: PathBuf,
}

impl CombinedAssets {
    pub fn new() -> Self {
        let icons_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("icons");
        Self { icons_dir }
    }
}

impl AssetSource for CombinedAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        // Handle service-icons/ prefix
        if let Some(rel) = path.strip_prefix("service-icons/") {
            let file = self.icons_dir.join(rel);
            if let Ok(bytes) = std::fs::read(&file) {
                return Ok(Some(Cow::Owned(bytes)));
            }
            // Fall through to bundled assets if not found.
        }

        // Handle icons/ prefix (for app icon and other icons)
        if let Some(rel) = path.strip_prefix("icons/") {
            let file = self.icons_dir.join(rel);
            if let Ok(bytes) = std::fs::read(&file) {
                return Ok(Some(Cow::Owned(bytes)));
            }
            // Fall through to bundled assets if not found.
        }

        ComponentAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        ComponentAssets.list(path)
    }
}
