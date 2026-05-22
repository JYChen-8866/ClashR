//! Helper service install/uninstall flow.
//!
//! On macOS we shell out to `osascript` and request admin privileges,
//! which pops the native "ClashR is requesting your password" dialog.
//! That elevated shell runs `clashr-service install` (or uninstall).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Locate the `clashr-service` binary.
///
/// Priority:
///   1. `CLASHR_SERVICE_PATH` env var
///   2. Same directory as the running ClashR binary
///   3. `<cwd>/target/release/clashr-service` (dev convenience)
///   4. `<cwd>/target/debug/clashr-service`
pub fn locate_service_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLASHR_SERVICE_PATH") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("clashr-service");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let cwd = std::env::current_dir().ok()?;
    for rel in ["target/release/clashr-service", "target/debug/clashr-service"] {
        let p = cwd.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub fn install_with_admin_prompt() -> Result<()> {
    let binary = locate_service_binary()
        .ok_or_else(|| anyhow::anyhow!("clashr-service binary not found"))?;
    run_admin_subcommand(&binary, "install")
}

#[cfg(target_os = "macos")]
pub fn uninstall_with_admin_prompt() -> Result<()> {
    let binary = locate_service_binary()
        .ok_or_else(|| anyhow::anyhow!("clashr-service binary not found"))?;
    run_admin_subcommand(&binary, "uninstall")
}

#[cfg(target_os = "macos")]
fn run_admin_subcommand(binary: &std::path::Path, subcommand: &str) -> Result<()> {
    let bin_str = binary.to_string_lossy();
    // The shell string has to be self-contained — osascript runs it as
    // a single shell invocation with elevated privileges. Quoting the
    // path keeps spaces safe.
    let shell = format!("'{bin_str}' {subcommand}");
    let prompt = format!("ClashR needs admin privileges to {subcommand} its helper service.");
    let script = format!(
        r#"do shell script "{shell}" with administrator privileges with prompt "{prompt}""#
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .context("running osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // osascript exits 1 with this stderr if the user cancels the
        // password dialog — treat that as a clean cancel, not an error.
        if stderr.contains("User canceled") {
            bail!("user cancelled");
        }
        bail!("{subcommand} failed: {stderr}");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn install_with_admin_prompt() -> Result<()> {
    bail!("service install is only implemented on macOS for now")
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall_with_admin_prompt() -> Result<()> {
    bail!("service uninstall is only implemented on macOS for now")
}

/// True if the daemon is reachable. Cheap (just a file-exists check on
/// the socket path) so it's safe to call on every render.
pub fn is_service_running() -> bool {
    crate::services::service_client::is_socket_present()
}

/// True if the launchd plist (or equivalent) is in place. Strictly
/// "installed" — could be installed but currently not started.
#[cfg(target_os = "macos")]
pub fn is_service_installed() -> bool {
    std::path::Path::new(clashr_ipc::LAUNCHD_PLIST_PATH).exists()
}

#[cfg(not(target_os = "macos"))]
pub fn is_service_installed() -> bool {
    false
}
