use anyhow::{Result, bail};
use base64::Engine;
use reqwest::Proxy;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubscriptionInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: u64,
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub body: String,
    pub name: Option<String>,
    pub extra: Option<SubscriptionInfo>,
}

pub async fn fetch_subscription(url: &str, proxy_port: u16) -> Result<FetchResult> {
    info!(url = %mask_url(url), proxy_port, "fetching subscription via proxy");

    let proxy_url = format!("http://127.0.0.1:{}", proxy_port);
    let proxy = Proxy::all(&proxy_url)?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .danger_accept_invalid_certs(false)
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("ClashR/0.1.0")
        .build()?;

    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "subscription request failed");
            return Err(e.into());
        }
    };

    let status = resp.status();
    if !status.is_success() {
        warn!(%status, "subscription returned non-success status");
        bail!("HTTP {}", status);
    }

    let headers = resp.headers().clone();
    let extra = parse_subscription_userinfo(&headers);
    let name = parse_profile_title(&headers).or_else(|| parse_content_disposition(&headers));

    let body = resp.text().await?;
    let body_len = body.len();
    let body = decode_body(&body);

    debug!(
        body_len,
        decoded_len = body.len(),
        has_extra = extra.is_some(),
        has_name = name.is_some(),
        "subscription fetched"
    );

    Ok(FetchResult { body, name, extra })
}

pub async fn fetch_subscription_direct(url: &str) -> Result<FetchResult> {
    info!(url = %mask_url(url), "fetching subscription directly");

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(false)
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("ClashR/0.1.0")
        .build()?;

    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        bail!("HTTP {}", resp.status());
    }

    let headers = resp.headers().clone();
    let extra = parse_subscription_userinfo(&headers);
    let name = parse_profile_title(&headers).or_else(|| parse_content_disposition(&headers));

    let body = resp.text().await?;
    let body = decode_body(&body);

    Ok(FetchResult { body, name, extra })
}

fn decode_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.starts_with('{') || trimmed.starts_with("proxies:") || trimmed.contains("proxy-groups:") {
        return body.to_string();
    }

    let engine = base64::engine::general_purpose::STANDARD;
    match engine.decode(trimmed.replace('\n', "").replace('\r', "")) {
        Ok(decoded) => String::from_utf8(decoded).unwrap_or_else(|_| body.to_string()),
        Err(_) => body.to_string(),
    }
}

fn parse_subscription_userinfo(headers: &reqwest::header::HeaderMap) -> Option<SubscriptionInfo> {
    for (key, value) in headers.iter() {
        let key_lower = key.as_str().to_ascii_lowercase();
        if key_lower.ends_with("subscription-userinfo") {
            let info_str = value.to_str().unwrap_or("");
            return Some(SubscriptionInfo {
                upload: parse_field(info_str, "upload"),
                download: parse_field(info_str, "download"),
                total: parse_field(info_str, "total"),
                expire: parse_field(info_str, "expire"),
            });
        }
    }
    None
}

fn parse_content_disposition(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let value = headers.get("content-disposition")?.to_str().ok()?;
    if let Some(pos) = value.find("filename=") {
        let rest = &value[pos + 9..];
        let name = rest.trim_matches('"').split(';').next().unwrap_or("");
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Many subscription providers (mihomo/clash) send a `Profile-Title` header
/// containing the airport/provider's display name (often base64-encoded UTF-8
/// because HTTP headers are ASCII-only).
fn parse_profile_title(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let value = headers.get("profile-title")?.to_str().ok()?;
    let value = value.trim();

    if let Some(b64) = value.strip_prefix("base64:") {
        let engine = base64::engine::general_purpose::STANDARD;
        if let Ok(bytes) = engine.decode(b64) {
            if let Ok(decoded) = String::from_utf8(bytes) {
                return Some(decoded);
            }
        }
        return None;
    }

    if !value.is_empty() {
        Some(value.to_string())
    } else {
        None
    }
}

fn parse_field(s: &str, key: &str) -> u64 {
    for part in s.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(key) {
            let val = val.trim_start_matches('=').trim();
            if let Ok(n) = val.parse::<u64>() {
                return n;
            }
        }
    }
    0
}

/// Mask sensitive query params (e.g. token=...) in URLs before logging.
fn mask_url(url: &str) -> String {
    if let Some(q_idx) = url.find('?') {
        format!("{}?<masked>", &url[..q_idx])
    } else {
        url.to_string()
    }
}
