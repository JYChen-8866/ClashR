use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use once_cell::sync::OnceCell;
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::core::paths;
use crate::core::process::{self, CoreEvent, CoreProcess};
use crate::runtime::spawn_on_tokio;

/// Coarse state surface that the UI watches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreStatus {
    Stopped,
    Starting,
    Running { pid: u32 },
    Stopping,
    Failed { reason: String },
}

impl Default for CoreStatus {
    fn default() -> Self {
        CoreStatus::Stopped
    }
}

type EventListener = Arc<dyn Fn(CoreEvent) + Send + Sync>;

#[derive(Default)]
struct Inner {
    status: CoreStatus,
    process: Option<CoreProcess>,
    listeners: Vec<EventListener>,
}

pub struct CoreManager {
    inner: Mutex<Inner>,
}

impl CoreManager {
    pub fn global() -> &'static CoreManager {
        static INSTANCE: OnceCell<CoreManager> = OnceCell::new();
        INSTANCE.get_or_init(|| CoreManager {
            inner: Mutex::new(Inner::default()),
        })
    }

    /// Subscribe to core events. The listener is called from the tokio runtime
    /// and must not block.
    pub async fn subscribe(&self, listener: impl Fn(CoreEvent) + Send + Sync + 'static) {
        let mut inner = self.inner.lock().await;
        inner.listeners.push(Arc::new(listener));
    }

    pub async fn status(&self) -> CoreStatus {
        self.inner.lock().await.status.clone()
    }

    /// Write a profile's body to disk under `data/profiles/<uid>.yaml`.
    /// Returns the path written.
    pub fn save_profile(&self, uid: &str, body: &str) -> Result<PathBuf> {
        let path = paths::profile_yaml_path(uid);
        std::fs::write(&path, body)?;
        info!(uid, path = %path.display(), "profile saved to disk");
        Ok(path)
    }

    /// Copy a saved profile into `data/runtime.yaml`, injecting system-level
    /// settings (ports, external controller) that the subscription doesn't
    /// provide. The next core start / reload will use this.
    pub fn activate_profile(&self, uid: &str) -> Result<()> {
        let src = paths::profile_yaml_path(uid);
        if !src.exists() {
            bail!("profile not on disk: {}", src.display());
        }

        let raw = std::fs::read_to_string(&src)?;
        let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw)
            .unwrap_or_else(|_| serde_yaml::Value::Mapping(Default::default()));

        // Inject system settings that mihomo needs to function.
        if let serde_yaml::Value::Mapping(ref mut map) = doc {
            let set_if_missing = |map: &mut serde_yaml::Mapping, key: &str, val: serde_yaml::Value| {
                let k = serde_yaml::Value::String(key.to_string());
                if !map.contains_key(&k) {
                    map.insert(k, val);
                }
            };
            set_if_missing(map, "mixed-port", serde_yaml::Value::Number(7890.into()));
            set_if_missing(map, "external-controller", serde_yaml::Value::String("127.0.0.1:9090".into()));
            set_if_missing(map, "mode", serde_yaml::Value::String("rule".into()));
            set_if_missing(map, "log-level", serde_yaml::Value::String("info".into()));
            set_if_missing(map, "allow-lan", serde_yaml::Value::Bool(false));

            // Default TUN block — disabled at start. The Home page's TUN
            // toggle flips `enable` via mihomo's external controller, so
            // these defaults define the *shape* of TUN (stack, DNS hijack,
            // routing) for when it's switched on.
            let tun_key = serde_yaml::Value::String("tun".into());
            if !map.contains_key(&tun_key) {
                let mut tun = serde_yaml::Mapping::new();
                tun.insert(serde_yaml::Value::String("enable".into()), serde_yaml::Value::Bool(false));
                tun.insert(serde_yaml::Value::String("stack".into()), serde_yaml::Value::String("gvisor".into()));
                tun.insert(
                    serde_yaml::Value::String("dns-hijack".into()),
                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("any:53".into())]),
                );
                tun.insert(serde_yaml::Value::String("auto-route".into()), serde_yaml::Value::Bool(true));
                tun.insert(serde_yaml::Value::String("auto-detect-interface".into()), serde_yaml::Value::Bool(true));
                map.insert(tun_key, serde_yaml::Value::Mapping(tun));
            }
        }

        let dst = paths::runtime_yaml_path();
        let output = serde_yaml::to_string(&doc)?;
        std::fs::write(&dst, output)?;
        info!(uid, "profile activated as runtime.yaml (with injected settings)");
        Ok(())
    }

    /// Start mihomo with the current `runtime.yaml`. Errors if no runtime
    /// config has been activated yet, or if the binary can't be located.
    ///
    /// Routing: if the clashr-service daemon socket is present we delegate
    /// the spawn there (mihomo runs as root, TUN works). Otherwise we
    /// fall back to spawning mihomo as the current user.
    pub async fn start(&self) -> Result<()> {
        if crate::services::service_client::is_socket_present() {
            return self.start_via_service().await;
        }
        self.start_directly().await
    }

    async fn start_via_service(&self) -> Result<()> {
        // Defensive cleanup: if the app previously direct-spawned mihomo
        // (e.g. before the daemon was installed), kill that child before
        // asking the daemon to start its own — otherwise we'd have two
        // mihomos fighting over port 7890.
        let stale = {
            let mut inner = self.inner.lock().await;
            inner.process.take()
        };
        if let Some(proc) = stale {
            warn!("found stale local mihomo, killing before delegating to service");
            let _ = spawn_on_tokio(async move { proc.stop().await }).await;
        }

        let mut inner = self.inner.lock().await;
        if matches!(inner.status, CoreStatus::Running { .. } | CoreStatus::Starting) {
            // Reset; we just killed the stale child above (or there was
            // never one). Anything previously running on the daemon side
            // will be replaced by the new StartCore call.
            inner.status = CoreStatus::Stopped;
        }
        inner.status = CoreStatus::Starting;
        drop(inner);

        let binary = paths::locate_mihomo().ok_or_else(|| {
            anyhow!("mihomo binary not found. Set MIHOMO_PATH env var, place ./mihomo next to ClashR, or install mihomo on PATH")
        })?;
        let config = paths::runtime_yaml_path();

        let bin_str = binary.to_string_lossy().into_owned();

        // macOS TCC blocks root from reading ~/Downloads (and other
        // user-protected dirs). Copy runtime.yaml to a shared temp
        // location that the root-owned daemon can access.
        let shared_dir = std::path::PathBuf::from("/tmp/clashr");
        std::fs::create_dir_all(&shared_dir)?;
        let shared_config = shared_dir.join("runtime.yaml");
        std::fs::copy(&config, &shared_config).map_err(|e| {
            anyhow!(
                "copy {} -> {}: {}",
                config.display(),
                shared_config.display(),
                e
            )
        })?;

        // Also copy geodata (*.mmdb, *.dat, *.metadb) so mihomo doesn't
        // re-download them on every restart. These can be 5-10 MB each.
        // Look in our own data dir first, then fall back to Clash Verge's
        // data dir (which likely already has them downloaded).
        let data_dir = paths::data_dir();
        let fallback_dir = dirs_next::data_dir()
            .map(|d| d.join("io.github.clash-verge-rev.clash-verge-rev"));
        let geo_sources: Vec<std::path::PathBuf> = [Some(data_dir.clone()), fallback_dir]
            .into_iter()
            .flatten()
            .collect();

        let geo_exts = ["mmdb", "dat", "metadb"];
        for src_dir in &geo_sources {
            if let Ok(entries) = std::fs::read_dir(src_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if geo_exts.contains(&ext) {
                            if let Some(name) = path.file_name() {
                                let dst = shared_dir.join(name);
                                if !dst.exists() {
                                    let _ = std::fs::copy(&path, &dst);
                                }
                            }
                        }
                    }
                }
            }
        }

        let cfg_str = shared_config.to_string_lossy().into_owned();

        // Use RestartCore so a previous daemon-owned mihomo (if any) is
        // killed first — keeps `start()` idempotent on the daemon path.
        let bin_clone = bin_str.clone();
        let cfg_clone = cfg_str.clone();
        let result = spawn_on_tokio(async move {
            crate::services::service_client::restart_core(bin_clone, cfg_clone).await
        })
        .await;

        let mut inner = self.inner.lock().await;
        match result {
            Ok(state) => {
                inner.status = ipc_state_to_status(state);
                info!(?inner.status, "core started via service");
                Ok(())
            }
            Err(e) => {
                let reason = format!("{:#}", e);
                warn!(error = %reason, "core start via service failed");
                inner.status = CoreStatus::Failed { reason };
                Err(e)
            }
        }
    }

    async fn start_directly(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;

        if matches!(inner.status, CoreStatus::Running { .. } | CoreStatus::Starting) {
            return Ok(());
        }

        let binary = paths::locate_mihomo().ok_or_else(|| {
            anyhow!("mihomo binary not found. Set MIHOMO_PATH env var, place ./mihomo next to ClashR, or install mihomo on PATH")
        })?;

        inner.status = CoreStatus::Starting;
        let listeners = inner.listeners.clone();
        drop(inner);

        let listeners_for_spawn = listeners.clone();
        let result = spawn_on_tokio(async move {
            process::spawn_mihomo(binary, move |event| {
                for l in &listeners_for_spawn {
                    l(event.clone());
                }
            })
            .await
        })
        .await;

        let mut inner = self.inner.lock().await;
        match result {
            Ok(proc) => {
                let pid = proc.pid();
                inner.status = CoreStatus::Running { pid };
                inner.process = Some(proc);

                // Watch for unexpected exits and update status.
                let manager_listener: EventListener = Arc::new(|event| {
                    if let CoreEvent::Exited { code } = event {
                        // We update status from a listener registered separately.
                        warn!(?code, "mihomo exited (will update status)");
                    }
                });
                inner.listeners.push(manager_listener);
                info!(pid, "core running");
                Ok(())
            }
            Err(e) => {
                let reason = format!("{:#}", e);
                warn!(error = %reason, "core start failed");
                inner.status = CoreStatus::Failed { reason };
                Err(e)
            }
        }
    }

    pub async fn stop(&self) -> Result<()> {
        // Always kill any locally-owned child first. This matters during
        // the migration window where the user installed the daemon
        // *after* the app had already direct-spawned mihomo: the daemon
        // doesn't know about that child, but we still do.
        let local_proc = {
            let mut inner = self.inner.lock().await;
            inner.process.take()
        };
        if let Some(proc) = local_proc {
            let _ = spawn_on_tokio(async move { proc.stop().await }).await;
        }

        // If the daemon is up, tell it to stop too — covers the normal
        // case where the daemon owns the live mihomo.
        if crate::services::service_client::is_socket_present() {
            let _ = spawn_on_tokio(async {
                crate::services::service_client::stop_core().await
            })
            .await;
        }

        let mut inner = self.inner.lock().await;
        inner.status = CoreStatus::Stopped;
        info!("core stopped");
        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        self.stop().await?;
        self.start().await
    }

    /// Hot-reload the runtime config via mihomo's External Controller API.
    /// This is much faster than restart and doesn't drop active connections.
    /// Falls back to restart if the API call fails.
    pub async fn reload_config(&self) -> Result<()> {
        let inner = self.inner.lock().await;
        if !matches!(inner.status, CoreStatus::Running { .. }) {
            drop(inner);
            return self.start().await;
        }
        drop(inner);

        let runtime_path = paths::runtime_yaml_path();
        let mut abs_path = std::fs::canonicalize(&runtime_path)
            .unwrap_or(runtime_path)
            .to_string_lossy()
            .to_string();

        // On Windows, canonicalize() adds a \\?\ prefix for long paths.
        // Strip it because mihomo's API doesn't recognize it.
        #[cfg(windows)]
        if abs_path.starts_with(r"\\?\") {
            abs_path = abs_path[4..].to_string();
        }

        info!(path = %abs_path, "reloading config via API");

        let client = Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| Client::new());
        let resp = client
            .put("http://127.0.0.1:9090/configs")
            .json(&serde_json::json!({
                "path": abs_path
            }))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                info!("config reloaded via API");
                Ok(())
            }
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                warn!(%status, %body, "reload API returned error, falling back to restart");
                self.restart().await
            }
            Err(e) => {
                warn!(error = %e, "reload API unreachable, falling back to restart");
                self.restart().await
            }
        }
    }

    /// Combined helper: persist profile body, activate it, and (re)start the core.
    pub async fn switch_to_profile(&self, uid: &str, body: &str) -> Result<()> {
        self.save_profile(uid, body)?;
        self.activate_profile(uid)?;
        self.reload_config().await
    }

    /// Mark the core as exited from outside (called by listeners that observe
    /// CoreEvent::Exited).
    pub async fn mark_exited(&self, code: Option<i32>) {
        let mut inner = self.inner.lock().await;
        if matches!(inner.status, CoreStatus::Stopping | CoreStatus::Stopped) {
            inner.status = CoreStatus::Stopped;
        } else {
            inner.status = CoreStatus::Failed {
                reason: format!("mihomo exited unexpectedly (code={:?})", code),
            };
        }
        inner.process = None;
    }
}

/// Translate the daemon's wire-format state into our local enum.
fn ipc_state_to_status(state: clashr_ipc::CoreState) -> CoreStatus {
    use clashr_ipc::CoreState;
    match state {
        CoreState::Stopped => CoreStatus::Stopped,
        CoreState::Starting => CoreStatus::Starting,
        CoreState::Running { pid } => CoreStatus::Running { pid },
        CoreState::Stopping => CoreStatus::Stopping,
        CoreState::Failed { reason } => CoreStatus::Failed { reason },
    }
}
