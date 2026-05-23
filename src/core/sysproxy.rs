//! System-proxy toggle.
//!
//! Flips the OS-level HTTP/HTTPS/SOCKS proxy so that "system" applications
//! (browsers, App Store, etc.) route through mihomo's mixed port. This is
//! how the GFW circumvention actually happens for non-TUN setups.
//!
//! macOS uses `networksetup`. Windows writes to the per-user Internet
//! Settings registry key and broadcasts a settings-change message so
//! running WinINet clients pick up the change without a restart.

use anyhow::Result;

/// Bypass list applied alongside `enable`. Mirrors what most clash-style
/// clients ship — keeps LAN/loopback traffic off the proxy.
#[cfg(any(target_os = "macos", target_os = "windows"))]
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
    imp::enable(host, port)
}

/// Turn the OS-level proxy off on every active network service.
pub fn disable() -> Result<()> {
    imp::disable()
}

/// Best-effort check: returns true if the OS-level proxy is on and
/// matches the supplied host/port.
pub fn is_enabled(host: &str, port: u16) -> bool {
    imp::is_enabled(host, port).unwrap_or(false)
}

#[cfg(target_os = "macos")]
mod imp {
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

#[cfg(target_os = "windows")]
mod imp {
    //! Windows system proxy via the per-user Internet Settings registry
    //! key. Affects WinINet/WinHTTP-aware apps (Edge, IE, Office, many
    //! desktop apps); apps with their own proxy settings (e.g. Firefox)
    //! are unaffected, which matches user expectations on Windows.

    use super::DEFAULT_BYPASS;
    use anyhow::{Context, Result};
    use tracing::{info, warn};
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

    const SETTINGS_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn open_settings(write: bool) -> Result<RegKey> {
        let access = if write {
            KEY_READ | KEY_SET_VALUE
        } else {
            KEY_READ
        };
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(SETTINGS_KEY, access)
            .with_context(|| format!(r"opening HKCU\{SETTINGS_KEY}"))
    }

    /// Tell WinINet to re-read settings so existing connections pick up
    /// the new proxy without a restart. Best-effort: we ignore failures.
    fn broadcast_change() {
        use windows_sys::Win32::Networking::WinInet::{
            INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
        };
        // Safety: both calls accept a NULL handle and a NULL buffer; the
        // OS interprets that as "broadcast to all WinINet sessions".
        unsafe {
            InternetSetOptionW(
                std::ptr::null_mut(),
                INTERNET_OPTION_SETTINGS_CHANGED,
                std::ptr::null_mut(),
                0,
            );
            InternetSetOptionW(
                std::ptr::null_mut(),
                INTERNET_OPTION_REFRESH,
                std::ptr::null_mut(),
                0,
            );
        }
    }

    pub fn enable(host: &str, port: u16) -> Result<()> {
        let key = open_settings(true)?;
        let server = format!("{host}:{port}");
        // Windows uses ';' as the separator and `<local>` to bypass
        // intranet hosts. Re-use the cross-platform list and append the
        // sentinel if missing.
        let mut bypass_parts: Vec<String> =
            DEFAULT_BYPASS.iter().map(|s| (*s).to_string()).collect();
        if !bypass_parts.iter().any(|p| p == "<local>") {
            bypass_parts.push("<local>".into());
        }
        let bypass = bypass_parts.join(";");

        key.set_value("ProxyEnable", &1u32)
            .context("set ProxyEnable=1")?;
        key.set_value("ProxyServer", &server)
            .context("set ProxyServer")?;
        key.set_value("ProxyOverride", &bypass)
            .context("set ProxyOverride")?;

        broadcast_change();
        info!(%host, port, "system proxy enabled");
        Ok(())
    }

    pub fn disable() -> Result<()> {
        let key = open_settings(true)?;
        key.set_value("ProxyEnable", &0u32)
            .context("set ProxyEnable=0")?;
        broadcast_change();
        info!("system proxy disabled");
        Ok(())
    }

    pub fn is_enabled(host: &str, port: u16) -> Result<bool> {
        let key = match open_settings(false) {
            Ok(k) => k,
            Err(e) => {
                warn!(error = %e, "open Internet Settings for read");
                return Ok(false);
            }
        };
        let on: u32 = key.get_value("ProxyEnable").unwrap_or(0);
        if on == 0 {
            return Ok(false);
        }
        let server: String = key.get_value("ProxyServer").unwrap_or_default();
        let want = format!("{host}:{port}");
        // ProxyServer can be either "host:port" (one proxy for all) or
        // "http=h:p;https=h:p;..." (per-protocol). We treat the latter
        // as "matches if HTTP entry matches" since that's what we set.
        let matches = if server.contains('=') {
            server.split(';').any(|part| {
                let part = part.trim();
                part.eq_ignore_ascii_case(&format!("http={want}"))
                    || part == want
            })
        } else {
            server == want
        };
        Ok(matches)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
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
