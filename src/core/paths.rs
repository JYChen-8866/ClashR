use std::path::PathBuf;

/// Returns true when running inside a macOS .app bundle.
fn is_app_bundle() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.contains(".app/Contents/MacOS/")))
        .unwrap_or(false)
}

/// Base directory for user data (profiles, runtime config).
/// - .app bundle: ~/Library/Application Support/ClashR/
/// - dev / CLI:   <cwd>/data/
pub fn data_dir() -> PathBuf {
    let dir = if is_app_bundle() {
        dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ClashR")
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("data")
    };
    std::fs::create_dir_all(&dir).ok();

    // One-time migration: if running as .app and profiles dir is empty,
    // copy data from the dev/CLI location so existing users don't lose
    // their subscriptions after installing the .app.
    if is_app_bundle() {
        let profiles = dir.join("profiles");
        if !profiles.exists() || profiles.read_dir().map(|mut d| d.next().is_none()).unwrap_or(true) {
            if let Ok(cwd) = std::env::current_dir() {
                let old_profiles = cwd.join("data/profiles");
                if old_profiles.exists() {
                    let _ = copy_dir_all(&old_profiles, &profiles);
                }
                let old_runtime = cwd.join("data/runtime.yaml");
                if old_runtime.exists() {
                    let _ = std::fs::copy(&old_runtime, dir.join("runtime.yaml"));
                }
            }
        }
    }

    dir
}

fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

/// Log directory.
/// - .app bundle: ~/Library/Logs/ClashR/
/// - dev / CLI:   <cwd>/logs/
pub fn log_dir() -> PathBuf {
    let dir = if is_app_bundle() {
        dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Logs/ClashR")
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("logs")
    };
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

/// The single runtime config that mihomo consumes.
pub fn runtime_yaml_path() -> PathBuf {
    data_dir().join("runtime.yaml")
}

/// Base directory for bundled assets (icons, themes).
/// - .app bundle: Contents/Resources/
/// - dev / CLI:   <cwd>/
pub fn resources_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        // Contents/MacOS/<exe> → Contents/Resources/
        if let Some(macos) = exe.parent() {
            let resources = macos.join("../Resources");
            if resources.exists() {
                return resources.canonicalize().unwrap_or(resources);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Where to look for the mihomo binary, in priority order.
pub fn locate_mihomo() -> Option<PathBuf> {
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
