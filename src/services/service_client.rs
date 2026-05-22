//! Thin IPC client for talking to the clashr-service daemon.
//!
//! Each high-level operation opens a fresh connection, does Hello +
//! request + response, then drops it. This is simpler than pooling and
//! per-connection cost is negligible — Unix socket connect is microseconds.

use anyhow::{Context, Result, bail};
use clashr_ipc::{CoreState, PROTOCOL_VERSION, Request, Response, SOCKET_PATH, transport};
use std::path::Path;
use tokio::net::UnixStream;
use tracing::warn;

pub async fn ping() -> Result<()> {
    let mut stream = handshake().await?;
    transport::write_frame(&mut stream, &Request::Status).await?;
    let _: Response = transport::read_frame(&mut stream).await?;
    Ok(())
}

pub fn is_socket_present() -> bool {
    Path::new(SOCKET_PATH).exists()
}

pub async fn start_core(binary: String, config: String) -> Result<CoreState> {
    let mut stream = handshake().await?;
    transport::write_frame(&mut stream, &Request::StartCore { binary, config }).await?;
    parse_status(transport::read_frame(&mut stream).await?)
}

pub async fn restart_core(binary: String, config: String) -> Result<CoreState> {
    let mut stream = handshake().await?;
    transport::write_frame(&mut stream, &Request::RestartCore { binary, config }).await?;
    parse_status(transport::read_frame(&mut stream).await?)
}

pub async fn stop_core() -> Result<CoreState> {
    let mut stream = handshake().await?;
    transport::write_frame(&mut stream, &Request::StopCore).await?;
    parse_status(transport::read_frame(&mut stream).await?)
}

pub async fn status() -> Result<CoreState> {
    let mut stream = handshake().await?;
    transport::write_frame(&mut stream, &Request::Status).await?;
    parse_status(transport::read_frame(&mut stream).await?)
}

async fn handshake() -> Result<UnixStream> {
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .await
        .with_context(|| format!("connect {SOCKET_PATH}"))?;
    transport::write_frame(
        &mut stream,
        &Request::Hello {
            version: PROTOCOL_VERSION,
        },
    )
    .await?;
    let resp: Response = transport::read_frame(&mut stream).await?;
    match resp {
        Response::Hello { version } if version == PROTOCOL_VERSION => Ok(stream),
        Response::Hello { version } => {
            warn!(peer = version, ours = PROTOCOL_VERSION, "version mismatch");
            bail!(
                "service version mismatch: daemon={version}, app={PROTOCOL_VERSION}"
            );
        }
        Response::Error { message } => bail!("daemon rejected handshake: {message}"),
        other => bail!("unexpected handshake response: {:?}", other),
    }
}

fn parse_status(resp: Response) -> Result<CoreState> {
    match resp {
        Response::Status(state) => Ok(state),
        Response::Ok => Ok(CoreState::Stopped),
        Response::Error { message } => bail!("{message}"),
        other => bail!("unexpected response: {:?}", other),
    }
}
