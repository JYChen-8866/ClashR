use std::path::PathBuf;

/// Project-relative data directory, created lazily on first access.
pub fn data_dir() -> PathBuf {
    let dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("data");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn profiles_dir() -> PathBuf {
    let dir = data_dir().join("profiles");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Path of the YAML file backing a profile (one per uid).
pub fn profile_yaml_path(uid: &str) -> PathBuf {
    profiles_dir().join(format!("{}.yaml", uid))
}

/// The single runtime config that mihomo consumes. We copy the active
/// profile's body into here on activation.
pub fn runtime_yaml_path() -> PathBuf {
    data_dir().join("runtime.yaml")
}

/// Where to look for the mihomo binary, in priority order.
///
/// 1. `MIHOMO_PATH` env var (override for testing)
/// 2. `<exe-dir>/mihomo[.exe]` — bundled next to the main app binary
/// 3. `<exe-dir>/../Resources/bin/mihomo` — macOS .app layout
/// 4. `<cwd>/bin/mihomo[.exe]` — in-tree stable location
/// 5. `<cwd>/mihomo[.exe]` — legacy fallback
/// 6. `mihomo[.exe]` resolved from `PATH`
pub fn locate_mihomo() -> Option<PathBuf> {
    // On Windows the binary is mihomo.exe; on every other platform it
    // has no extension.
    let bin_name = if cfg!(target_os = "windows") {
        "mihomo.exe"
    } else {
        "mihomo"
    };

    if let Ok(p) = std::env::var("MIHOMO_PATH") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join(bin_name);
            if candidate.is_file() {
                return Some(candidate);
            }
            // macOS .app bundle layout: Contents/MacOS/../Resources/bin/
            let resources = parent.join("..").join("Resources").join("bin").join(bin_name);
            if resources.is_file() {
                return Some(resources);
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        for rel in [
            format!("bin/{bin_name}"),
            bin_name.to_string(),
        ] {
            let p = cwd.join(&rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    which(bin_name)
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
