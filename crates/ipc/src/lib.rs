//! Wire protocol between ClashR (the app, runs as user) and clashr-service
//! (the daemon, runs as root). Length-prefixed JSON over Unix socket on
//! macOS/Linux, named pipe on Windows (Windows path TBD).
//!
//! Framing: each message is a 4-byte big-endian unsigned length, then that
//! many bytes of UTF-8 JSON. No streaming responses — every request gets
//! exactly one response.
//!
//! Versioning: `Hello` carries protocol version on both sides. The daemon
//! refuses requests until it sees a Hello with a compatible version.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[cfg(unix)]
pub const SOCKET_PATH: &str = "/var/run/clashr/clashr-service.sock";

/// Where the daemon's plist/unit/service file lives.
#[cfg(target_os = "macos")]
pub const LAUNCHD_PLIST_PATH: &str = "/Library/LaunchDaemons/com.clashr.service.plist";

#[cfg(target_os = "macos")]
pub const LAUNCHD_LABEL: &str = "com.clashr.service";

/// All requests the app can make to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Handshake — must be the first request on a new connection. Daemon
    /// validates protocol version and replies with its own version.
    Hello { version: u32 },

    /// Spawn mihomo with the given absolute paths. If a core is already
    /// running, returns an error — caller should `RestartCore` instead.
    StartCore {
        binary: String,
        config: String,
    },

    /// Hot-reload mihomo's config without dropping connections. mihomo's
    /// own external controller does the actual reload; the daemon just
    /// proxies the request because the controller is on a privileged
    /// loopback.
    RestartCore {
        binary: String,
        config: String,
    },

    /// Kill mihomo if it's running. Idempotent — a no-op when stopped.
    StopCore,

    /// Get current daemon-tracked state (running? pid? last error?).
    Status,

    /// Tell the daemon to exit. Used by uninstall flow only.
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    /// Hello reply — daemon's protocol version.
    Hello { version: u32 },

    /// Generic OK.
    Ok,

    /// Status reply.
    Status(CoreState),

    /// Generic failure with a human-readable reason.
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CoreState {
    Stopped,
    Starting,
    Running { pid: u32 },
    Stopping,
    Failed { reason: String },
}

#[cfg(feature = "async")]
pub mod transport {
    //! Length-prefixed JSON framing on top of any tokio AsyncRead/AsyncWrite.

    use anyhow::{Context as _, Result, bail};
    use serde::{de::DeserializeOwned, Serialize};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const MAX_FRAME: u32 = 16 * 1024 * 1024; // 16 MiB

    pub async fn write_frame<W, T>(w: &mut W, msg: &T) -> Result<()>
    where
        W: AsyncWriteExt + Unpin,
        T: Serialize,
    {
        let bytes = serde_json::to_vec(msg).context("serialize frame")?;
        if bytes.len() as u64 > MAX_FRAME as u64 {
            bail!("frame too large: {} bytes", bytes.len());
        }
        let len = (bytes.len() as u32).to_be_bytes();
        w.write_all(&len).await?;
        w.write_all(&bytes).await?;
        w.flush().await?;
        Ok(())
    }

    pub async fn read_frame<R, T>(r: &mut R) -> Result<T>
    where
        R: AsyncReadExt + Unpin,
        T: DeserializeOwned,
    {
        let mut len_buf = [0u8; 4];
        r.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf);
        if len > MAX_FRAME {
            bail!("frame too large in header: {} bytes", len);
        }
        let mut buf = vec![0u8; len as usize];
        r.read_exact(&mut buf).await?;
        serde_json::from_slice(&buf).context("deserialize frame")
    }
}
