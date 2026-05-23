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
/// 2. `<exe-dir>/mihomo` — bundled next to the main app binary
/// 3. `<exe-dir>/../Resources/bin/mihomo` — alternative .app layout
/// 4. `<cwd>/bin/mihomo` — in-tree stable location, committed so the
///    project is self-contained and runnable on a fresh checkout
/// 5. `<cwd>/mihomo` — legacy fallback for older layouts
/// 6. `mihomo` resolved from `PATH`
pub fn locate_mihomo() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MIHOMO_PATH") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("mihomo");
            if candidate.is_file() {
                return Some(candidate);
            }
            let resources = parent.join("..").join("Resources").join("bin").join("mihomo");
            if resources.is_file() {
                return Some(resources);
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        for rel in ["bin/mihomo", "mihomo"] {
            let p = cwd.join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    which("mihomo")
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
