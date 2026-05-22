use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::core::paths;

/// Output from a running mihomo process. Each variant maps to one event the
/// UI may want to react to.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    Started { pid: u32 },
    Log { line: String, is_stderr: bool },
    Exited { code: Option<i32> },
}

/// Wraps a spawned mihomo child + its drain task. Dropping kills the process.
pub struct CoreProcess {
    child: Arc<Mutex<Option<Child>>>,
    pid: u32,
}

impl CoreProcess {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Send SIGTERM (best-effort) and reap. Idempotent.
    pub async fn stop(&self) -> Result<()> {
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
            info!(pid = self.pid, "mihomo stopped");
        }
        Ok(())
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        // Best-effort synchronous kill if the user forgot to stop().
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(child) = guard.as_mut() {
                if let Some(id) = child.id() {
                    info!(pid = id, "killing mihomo on drop");
                }
                let _ = child.start_kill();
            }
        }
    }
}

/// Spawn `mihomo -d <data_dir> -f <runtime.yaml>` and start draining its
/// stdout/stderr into the supplied callback.
pub async fn spawn_mihomo(
    binary: PathBuf,
    on_event: impl Fn(CoreEvent) + Send + Sync + 'static,
) -> Result<CoreProcess> {
    let runtime_cfg = paths::runtime_yaml_path();
    if !runtime_cfg.exists() {
        bail!("runtime.yaml not found: activate a profile first");
    }

    let data_dir = paths::data_dir();

    info!(
        binary = %binary.display(),
        data_dir = %data_dir.display(),
        config = %runtime_cfg.display(),
        "spawning mihomo"
    );

    let mut cmd = Command::new(&binary);
    cmd.arg("-d")
        .arg(&data_dir)
        .arg("-f")
        .arg(&runtime_cfg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    #[cfg(unix)]
    {
        // Make sure children don't outlive us if we crash.
        // (On Unix, kill_on_drop sends SIGKILL when the child handle is dropped.)
        cmd.kill_on_drop(true);
    }
    #[cfg(windows)]
    {
        cmd.kill_on_drop(true);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn mihomo at {}", binary.display()))?;

    let pid = child
        .id()
        .context("mihomo child has no PID; was it started?")?;
    on_event(CoreEvent::Started { pid });

    let stdout = child
        .stdout
        .take()
        .context("failed to capture mihomo stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture mihomo stderr")?;

    let on_event = Arc::new(on_event);

    {
        let on_event = on_event.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                info!(target: "mihomo", "{line}");
                on_event(CoreEvent::Log {
                    line,
                    is_stderr: false,
                });
            }
        });
    }

    {
        let on_event = on_event.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                warn!(target: "mihomo", "{line}");
                on_event(CoreEvent::Log {
                    line,
                    is_stderr: true,
                });
            }
        });
    }

    let child = Arc::new(Mutex::new(Some(child)));

    {
        let child = child.clone();
        let on_event = on_event.clone();
        tokio::spawn(async move {
            // Wait for the process to exit, then notify.
            let mut guard = child.lock().await;
            if let Some(child) = guard.as_mut() {
                match child.wait().await {
                    Ok(status) => {
                        let code = status.code();
                        info!(pid, ?code, "mihomo exited");
                        on_event(CoreEvent::Exited { code });
                    }
                    Err(e) => {
                        error!(pid, error = %e, "failed to wait for mihomo");
                        on_event(CoreEvent::Exited { code: None });
                    }
                }
            }
        });
    }

    Ok(CoreProcess { child, pid })
}
