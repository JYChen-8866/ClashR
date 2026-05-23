//! Mihomo subprocess management — spawn / kill / track exit. Mirrors
//! ClashR's existing `core::process` but lives in the daemon so the
//! mihomo PID belongs to root.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Tracks the live mihomo child. Wrapping in Arc<Mutex<_>> lets the IPC
/// handlers and the watchdog task share access.
pub type SharedCore = Arc<Mutex<CoreSlot>>;

#[derive(Default)]
pub struct CoreSlot {
    pub child: Option<Child>,
    pub last_failure: Option<String>,
}

impl CoreSlot {
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.id())
    }
}

pub fn new_shared() -> SharedCore {
    Arc::new(Mutex::new(CoreSlot::default()))
}

/// Spawn mihomo with `-d <dir> -f <config>`. The directory is taken as
/// the parent of the config so mihomo's relative-path lookups (rule
/// providers, etc.) resolve correctly.
pub async fn start(slot: &SharedCore, binary: &str, config: &str) -> Result<u32> {
    let mut guard = slot.lock().await;
    if guard.child.is_some() {
        anyhow::bail!("core is already running");
    }

    let config_path = std::path::PathBuf::from(config);
    let dir = config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    info!(binary, %config, "spawning mihomo");

    let mut child = Command::new(binary)
        .arg("-d")
        .arg(&dir)
        .arg("-f")
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {binary}"))?;

    let pid = child.id().context("spawned child has no pid")?;

    // Drain stdout/stderr into the daemon log so we can debug from
    // /var/log/clashr-service.log when something goes sideways.
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                info!(target: "mihomo", "{line}");
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                warn!(target: "mihomo", "{line}");
            }
        });
    }

    guard.child = Some(child);
    guard.last_failure = None;
    Ok(pid)
}

pub async fn stop(slot: &SharedCore) -> Result<()> {
    let mut guard = slot.lock().await;
    if let Some(mut child) = guard.child.take() {
        info!("stopping mihomo");

        // Send SIGTERM first so mihomo can clean up TUN interfaces and
        // routes. SIGKILL bypasses cleanup and leaves stale routes that
        // cause "file exists" errors on the next TUN enable.
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            // Give mihomo up to 4 seconds to clean up TUN and exit.
            match tokio::time::timeout(Duration::from_secs(4), child.wait()).await {
                Ok(_) => {
                    info!("mihomo exited cleanly after SIGTERM");
                    return Ok(());
                }
                Err(_) => {
                    warn!("mihomo did not exit after SIGTERM, sending SIGKILL");
                }
            }
        }

        // Fallback: force-kill if SIGTERM didn't work (or on non-Unix).
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    Ok(())
}

pub async fn is_running(slot: &SharedCore) -> bool {
    let guard = slot.lock().await;
    guard.child.is_some()
}
