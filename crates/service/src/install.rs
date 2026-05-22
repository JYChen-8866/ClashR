//! Service install/uninstall — write a launchd plist that supervises the
//! daemon and `launchctl load/unload` it.
//!
//! Must be run as root. The flow on macOS is:
//!   sudo /Applications/.../clashr-service install
//!   → writes /Library/LaunchDaemons/com.clashr.service.plist
//!   → launchctl load -w that path
//!   → launchd respawns the daemon forever
//!
//! Uninstall does the inverse: launchctl unload -w + remove the plist,
//! and sends Shutdown to the daemon for good measure.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "macos")]
use clashr_ipc::{LAUNCHD_LABEL, LAUNCHD_PLIST_PATH};

#[cfg(target_os = "macos")]
pub fn install() -> Result<()> {
    require_root()?;

    let exe = std::env::current_exe().context("locate current exe")?;
    let exe_str = exe.to_string_lossy();

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/var/log/clashr-service.out.log</string>
    <key>StandardErrorPath</key>
    <string>/var/log/clashr-service.err.log</string>
</dict>
</plist>
"#,
    );

    std::fs::write(LAUNCHD_PLIST_PATH, plist)
        .with_context(|| format!("write {LAUNCHD_PLIST_PATH}"))?;

    // chmod 644 + chown root:wheel — launchd requires this.
    let status = Command::new("chown")
        .arg("root:wheel")
        .arg(LAUNCHD_PLIST_PATH)
        .status()
        .context("chown plist")?;
    if !status.success() {
        bail!("chown plist failed");
    }
    let status = Command::new("chmod")
        .arg("644")
        .arg(LAUNCHD_PLIST_PATH)
        .status()
        .context("chmod plist")?;
    if !status.success() {
        bail!("chmod plist failed");
    }

    let status = Command::new("launchctl")
        .args(["load", "-w", LAUNCHD_PLIST_PATH])
        .status()
        .context("launchctl load")?;
    if !status.success() {
        bail!("launchctl load failed (exit {:?})", status.code());
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    require_root()?;

    if Path::new(LAUNCHD_PLIST_PATH).exists() {
        // Best-effort unload; if launchctl errors we still try to remove
        // the plist so a future install can succeed.
        let _ = Command::new("launchctl")
            .args(["unload", "-w", LAUNCHD_PLIST_PATH])
            .status();
        std::fs::remove_file(LAUNCHD_PLIST_PATH).ok();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn is_installed() -> bool {
    Path::new(LAUNCHD_PLIST_PATH).exists()
}

#[cfg(not(target_os = "macos"))]
pub fn install() -> Result<()> {
    bail!("install is not implemented on this platform yet")
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> Result<()> {
    bail!("uninstall is not implemented on this platform yet")
}

#[cfg(not(target_os = "macos"))]
pub fn is_installed() -> bool {
    false
}

#[cfg(unix)]
fn require_root() -> Result<()> {
    let euid = unsafe { libc_geteuid() };
    if euid != 0 {
        bail!("must run as root (current euid={euid})");
    }
    Ok(())
}

#[cfg(unix)]
extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}
