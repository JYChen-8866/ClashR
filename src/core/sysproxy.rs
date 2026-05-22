//! System-proxy toggle.
//!
//! Flips the OS-level HTTP/HTTPS/SOCKS proxy so that "system" applications
//! (browsers, App Store, etc.) route through mihomo's mixed port. This is
//! how the GFW circumvention actually happens for non-TUN setups.
//!
//! Currently only macOS is implemented (via `networksetup`). Windows and
//! Linux are stubs that return `Err` so callers can surface the limitation.

use anyhow::Result;

/// Bypass list applied alongside `enable`. Mirrors what most clash-style
/// clients ship — keeps LAN/loopback traffic off the proxy.
#[cfg(target_os = "macos")]
const DEFAULT_BYPASS: &[&str] = &[
    "127.0.0.1",
    "192.168.0.0/16",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "localhost",
    "*.local",
    "*.crashlytics.com",
    "<local>",
];

/// Turn the OS-level proxy on, pointing every active network service at
/// `host:port` for HTTP, HTTPS, and SOCKS.
pub fn enable(host: &str, port: u16) -> Result<()> {
    macos::enable(host, port)
}

/// Turn the OS-level proxy off on every active network service.
pub fn disable() -> Result<()> {
    macos::disable()
}

/// Best-effort check: returns true if at least one active network service
/// has HTTP proxy enabled and pointed at the given host/port.
pub fn is_enabled(host: &str, port: u16) -> bool {
    macos::is_enabled(host, port).unwrap_or(false)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::DEFAULT_BYPASS;
    use anyhow::{Context, Result, bail};
    use std::process::Command;
    use tracing::{debug, info, warn};

    /// Enumerate every "active" (non-disabled) network service. macOS
    /// prefixes disabled services with an asterisk in the listing — we
    /// drop those.
    fn active_services() -> Result<Vec<String>> {
        let output = Command::new("networksetup")
            .arg("-listallnetworkservices")
            .output()
            .context("running networksetup -listallnetworkservices")?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).into_owned();
            bail!("networksetup -listallnetworkservices failed: {err}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut services = Vec::new();
        for (i, line) in stdout.lines().enumerate() {
            if i == 0 {
                // First line is a banner ("An asterisk (*) denotes...").
                continue;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('*') {
                continue;
            }
            services.push(trimmed.to_string());
        }
        Ok(services)
    }

    fn run(args: &[&str]) -> Result<()> {
        let output = Command::new("networksetup")
            .args(args)
            .output()
            .with_context(|| format!("running networksetup {args:?}"))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).into_owned();
            bail!("networksetup {args:?} failed: {err}");
        }
        Ok(())
    }

    pub fn enable(host: &str, port: u16) -> Result<()> {
        let services = active_services()?;
        if services.is_empty() {
            warn!("no active network services found; system proxy not applied");
            return Ok(());
        }

        let port_s = port.to_string();
        let bypass: Vec<&str> = DEFAULT_BYPASS.to_vec();

        for svc in &services {
            // HTTP
            if let Err(e) = run(&["-setwebproxy", svc, host, &port_s]) {
                warn!(service = %svc, error = %e, "setwebproxy failed");
                continue;
            }
            let _ = run(&["-setwebproxystate", svc, "on"]);

            // HTTPS
            let _ = run(&["-setsecurewebproxy", svc, host, &port_s]);
            let _ = run(&["-setsecurewebproxystate", svc, "on"]);

            // SOCKS
            let _ = run(&["-setsocksfirewallproxy", svc, host, &port_s]);
            let _ = run(&["-setsocksfirewallproxystate", svc, "on"]);

            // Bypass list (all three categories share one list on macOS).
            let mut bypass_args: Vec<&str> = vec!["-setproxybypassdomains", svc];
            bypass_args.extend(bypass.iter().copied());
            let _ = run(&bypass_args);

            debug!(service = %svc, %host, port, "system proxy enabled");
        }

        info!(services = services.len(), %host, port, "system proxy enabled");
        Ok(())
    }

    pub fn disable() -> Result<()> {
        let services = active_services()?;
        for svc in &services {
            let _ = run(&["-setwebproxystate", svc, "off"]);
            let _ = run(&["-setsecurewebproxystate", svc, "off"]);
            let _ = run(&["-setsocksfirewallproxystate", svc, "off"]);
            debug!(service = %svc, "system proxy disabled");
        }
        info!(services = services.len(), "system proxy disabled");
        Ok(())
    }

    pub fn is_enabled(host: &str, port: u16) -> Result<bool> {
        let services = active_services()?;
        for svc in &services {
            let output = Command::new("networksetup")
                .args(["-getwebproxy", svc])
                .output()
                .context("running networksetup -getwebproxy")?;
            if !output.status.success() {
                continue;
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut on = false;
            let mut server_match = false;
            let mut port_match = false;
            for line in stdout.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("Enabled:") {
                    on = v.trim().eq_ignore_ascii_case("yes");
                } else if let Some(v) = line.strip_prefix("Server:") {
                    server_match = v.trim() == host;
                } else if let Some(v) = line.strip_prefix("Port:") {
                    port_match = v.trim().parse::<u16>().ok() == Some(port);
                }
            }
            if on && server_match && port_match {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    use anyhow::{Result, bail};

    pub fn enable(_host: &str, _port: u16) -> Result<()> {
        bail!("system proxy toggle is not implemented on this platform yet")
    }
    pub fn disable() -> Result<()> {
        bail!("system proxy toggle is not implemented on this platform yet")
    }
    pub fn is_enabled(_host: &str, _port: u16) -> Result<bool> {
        Ok(false)
    }
}
