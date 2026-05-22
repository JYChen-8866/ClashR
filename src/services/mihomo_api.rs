use std::collections::HashMap;

use anyhow::{Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

const BASE_URL: &str = "http://127.0.0.1:9090";

/// Single proxy/group entry as returned by mihomo's `/proxies` API.
///
/// Both individual nodes (Trojan, Vmess, ...) and groups (Selector, URLTest, ...)
/// share the same shape; groups have `now` and `all` populated, leaf nodes
/// have just `history`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyNode {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub now: Option<String>,
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub history: Vec<DelaySample>,
    #[serde(default)]
    pub udp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelaySample {
    #[serde(default)]
    pub time: String,
    pub delay: u32,
}

impl ProxyNode {
    /// Most recent delay sample in ms, if any. 0 means timeout/unreachable.
    pub fn last_delay(&self) -> Option<u32> {
        self.history.last().map(|s| s.delay)
    }

    pub fn is_group(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "Selector" | "URLTest" | "Fallback" | "LoadBalance" | "Relay" | "Smart"
        )
    }

    pub fn is_user_selectable(&self) -> bool {
        // Selector is the only group where the user picks the node directly.
        // URLTest/Fallback/LoadBalance pick automatically.
        self.kind == "Selector"
    }
}

#[derive(Debug, Deserialize)]
struct ProxiesResponse {
    proxies: HashMap<String, ProxyNode>,
}

fn client() -> Client {
    Client::new()
}

/// Fetch the full proxy state from mihomo. Returns a map keyed by name.
pub async fn get_proxies() -> Result<HashMap<String, ProxyNode>> {
    let url = format!("{}/proxies", BASE_URL);
    debug!(%url, "GET /proxies");

    let resp = client().get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("HTTP {}", resp.status());
    }
    let body: ProxiesResponse = resp.json().await?;
    Ok(body.proxies)
}

/// Switch the selected node in a Selector group.
pub async fn select_proxy(group: &str, node: &str) -> Result<()> {
    let url = format!("{}/proxies/{}", BASE_URL, urlencoding::encode(group));
    info!(group, node, "PUT /proxies/{group}");

    let resp = client()
        .put(&url)
        .json(&serde_json::json!({ "name": node }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        warn!(%status, %body, "select_proxy failed");
        bail!("HTTP {}", status);
    }

    Ok(())
}

/// Trigger a delay test against the given proxy. Returns the measured delay
/// in ms, or an error if the test failed.
pub async fn delay_test(node: &str, test_url: &str, timeout_ms: u32) -> Result<u32> {
    let url = format!(
        "{}/proxies/{}/delay?url={}&timeout={}",
        BASE_URL,
        urlencoding::encode(node),
        urlencoding::encode(test_url),
        timeout_ms
    );

    #[derive(Deserialize)]
    struct DelayResp {
        delay: u32,
    }

    let resp = client().get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("HTTP {}", resp.status());
    }
    let body: DelayResp = resp.json().await?;
    Ok(body.delay)
}
