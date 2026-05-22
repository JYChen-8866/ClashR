//! Daemon entry point: open the Unix socket, accept connections, dispatch
//! IPC requests to the core lifecycle module.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clashr_ipc::{CoreState, PROTOCOL_VERSION, Request, Response, SOCKET_PATH, transport};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::process::{self, SharedCore};

const LOG_PATH: &str = "/var/log/clashr-service.log";

pub fn run() -> Result<()> {
    init_logging()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    runtime.block_on(run_async())
}

fn init_logging() -> Result<()> {
    // /var/log is writable by root (which we are). Ignore failure and
    // fall back to stderr if for some reason it's not — the daemon must
    // not refuse to start just because logging is broken.
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("clashr_service=info,info"));

    match log_file {
        Ok(file) => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
        }
        Err(e) => {
            tracing_subscriber::fmt().with_env_filter(env_filter).init();
            warn!(error = %e, "could not open {LOG_PATH}, logging to stderr");
        }
    }
    Ok(())
}

async fn run_async() -> Result<()> {
    info!(version = PROTOCOL_VERSION, "clashr-service starting");

    let listener = bind_socket(SOCKET_PATH).await?;
    let core = process::new_shared();

    // SIGTERM/SIGINT handler so launchctl unload can stop us cleanly
    // and we kill mihomo on the way out.
    let core_for_shutdown = core.clone();
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        let mut int = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("install SIGINT handler");
        tokio::select! {
            _ = term.recv() => info!("SIGTERM received"),
            _ = int.recv() => info!("SIGINT received"),
        }
        let _ = process::stop(&core_for_shutdown).await;
        // Remove the socket so a clean restart can re-bind without
        // hitting "address already in use".
        let _ = std::fs::remove_file(SOCKET_PATH);
        std::process::exit(0);
    });

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let core = core.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, core).await {
                        warn!(error = %e, "connection ended with error");
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "accept failed");
            }
        }
    }
}

async fn bind_socket(path: &str) -> Result<UnixListener> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create socket dir {}", parent.display()))?;
    }
    if path.exists() {
        // Stale socket from a previous run.
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path).context("bind unix socket")?;

    // 0660 + root:admin so any admin (sudo-capable) user can connect.
    // Dedicated clashr group would be tidier but adds an install step.
    let perms = std::fs::Permissions::from_mode(0o660);
    std::fs::set_permissions(path, perms).context("chmod socket")?;
    if let Err(e) = chown_to_admin_group(path) {
        warn!(error = %e, "could not chown socket to admin group; non-root callers may fail");
    }

    info!(path = %path.display(), "listening on socket");
    Ok(listener)
}

#[cfg(target_os = "macos")]
fn chown_to_admin_group(path: &Path) -> Result<()> {
    use std::ffi::CString;
    let cpath = CString::new(path.as_os_str().to_string_lossy().as_bytes())?;
    // Root uid (0), admin gid (80 on macOS — the "admin" group).
    let r = unsafe { libc_chown(cpath.as_ptr(), 0, 80) };
    if r != 0 {
        bail!("chown failed: errno {}", std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn chown_to_admin_group(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
extern "C" {
    #[link_name = "chown"]
    fn libc_chown(path: *const std::os::raw::c_char, owner: u32, group: u32) -> std::os::raw::c_int;
}

async fn handle_connection(mut stream: UnixStream, core: SharedCore) -> Result<()> {
    // Mandatory handshake: peer says Hello first, we reply with our
    // version, then we can serve requests on this connection.
    let hello: Request = transport::read_frame(&mut stream).await?;
    match hello {
        Request::Hello { version } => {
            if version != PROTOCOL_VERSION {
                let resp = Response::Error {
                    message: format!(
                        "protocol version mismatch: peer={version}, daemon={PROTOCOL_VERSION}"
                    ),
                };
                let _ = transport::write_frame(&mut stream, &resp).await;
                bail!("rejected client with incompatible version {version}");
            }
            transport::write_frame(
                &mut stream,
                &Response::Hello {
                    version: PROTOCOL_VERSION,
                },
            )
            .await?;
        }
        other => {
            let resp = Response::Error {
                message: "first request must be Hello".into(),
            };
            let _ = transport::write_frame(&mut stream, &resp).await;
            bail!("first request was {:?}, not Hello", other);
        }
    }

    loop {
        let req: Request = match transport::read_frame(&mut stream).await {
            Ok(r) => r,
            Err(_) => return Ok(()), // peer disconnected
        };
        let resp = dispatch(req, &core).await;
        if let Err(e) = transport::write_frame(&mut stream, &resp).await {
            warn!(error = %e, "failed to write response");
            return Ok(());
        }
    }
}

async fn dispatch(req: Request, core: &SharedCore) -> Response {
    match req {
        Request::Hello { .. } => Response::Error {
            message: "Hello already exchanged on this connection".into(),
        },
        Request::StartCore { binary, config } => match process::start(core, &binary, &config).await
        {
            Ok(pid) => {
                info!(pid, "core started");
                Response::Status(CoreState::Running { pid })
            }
            Err(e) => Response::Error {
                message: format!("start failed: {e:#}"),
            },
        },
        Request::RestartCore { binary, config } => {
            let _ = process::stop(core).await;
            match process::start(core, &binary, &config).await {
                Ok(pid) => Response::Status(CoreState::Running { pid }),
                Err(e) => Response::Error {
                    message: format!("restart failed: {e:#}"),
                },
            }
        }
        Request::StopCore => match process::stop(core).await {
            Ok(()) => Response::Status(CoreState::Stopped),
            Err(e) => Response::Error {
                message: format!("stop failed: {e:#}"),
            },
        },
        Request::Status => {
            let guard = core.lock().await;
            match (&guard.child, &guard.last_failure) {
                (Some(_), _) => Response::Status(CoreState::Running {
                    pid: guard.pid().unwrap_or(0),
                }),
                (None, Some(reason)) => Response::Status(CoreState::Failed {
                    reason: reason.clone(),
                }),
                (None, None) => Response::Status(CoreState::Stopped),
            }
        }
        Request::Shutdown => {
            info!("shutdown requested");
            let _ = process::stop(core).await;
            let _ = std::fs::remove_file(SOCKET_PATH);
            // Give the response a moment to flush.
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                std::process::exit(0);
            });
            Response::Ok
        }
    }
}

#[allow(dead_code)]
fn _path_into_string(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}
