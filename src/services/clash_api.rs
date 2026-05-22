use serde::Deserialize;

pub struct ClashApi {
    base_url: String,
    secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProxyGroup {
    pub name: String,
    pub r#type: String,
    pub now: Option<String>,
    pub all: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProxyItem {
    pub name: String,
    pub r#type: String,
    pub history: Vec<ProxyHistory>,
}

#[derive(Debug, Deserialize)]
pub struct ProxyHistory {
    pub delay: u32,
}

#[derive(Debug, Deserialize)]
pub struct Connection {
    pub id: String,
    pub metadata: ConnectionMetadata,
    pub upload: u64,
    pub download: u64,
    pub chains: Vec<String>,
    pub rule: String,
}

#[derive(Debug, Deserialize)]
pub struct ConnectionMetadata {
    pub host: String,
    pub network: String,
    pub r#type: String,
    #[serde(rename = "destinationPort")]
    pub destination_port: String,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub r#type: String,
    pub payload: String,
    pub proxy: String,
}

#[derive(Debug, Deserialize)]
pub struct TrafficData {
    pub up: u64,
    pub down: u64,
}

impl ClashApi {
    pub fn new(base_url: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            secret,
        }
    }

    fn client(&self) -> reqwest::Client {
        let mut builder = reqwest::Client::builder();
        if let Some(ref secret) = self.secret {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                "Authorization",
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", secret))
                    .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
            );
            builder = builder.default_headers(headers);
        }
        builder.build().unwrap_or_default()
    }

    pub async fn get_proxies(&self) -> Result<Vec<ProxyGroup>, reqwest::Error> {
        let resp: serde_json::Value = self
            .client()
            .get(format!("{}/proxies", self.base_url))
            .send()
            .await?
            .json()
            .await?;

        let groups = resp["proxies"]
            .as_object()
            .map(|obj| {
                obj.values()
                    .filter_map(|v| serde_json::from_value::<ProxyGroup>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(groups)
    }

    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<(), reqwest::Error> {
        self.client()
            .put(format!("{}/proxies/{}", self.base_url, group))
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn get_connections(&self) -> Result<Vec<Connection>, reqwest::Error> {
        let resp: serde_json::Value = self
            .client()
            .get(format!("{}/connections", self.base_url))
            .send()
            .await?
            .json()
            .await?;

        let conns = resp["connections"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<Connection>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(conns)
    }

    pub async fn close_connection(&self, id: &str) -> Result<(), reqwest::Error> {
        self.client()
            .delete(format!("{}/connections/{}", self.base_url, id))
            .send()
            .await?;
        Ok(())
    }

    pub async fn close_all_connections(&self) -> Result<(), reqwest::Error> {
        self.client()
            .delete(format!("{}/connections", self.base_url))
            .send()
            .await?;
        Ok(())
    }

    pub async fn get_rules(&self) -> Result<Vec<Rule>, reqwest::Error> {
        let resp: serde_json::Value = self
            .client()
            .get(format!("{}/rules", self.base_url))
            .send()
            .await?
            .json()
            .await?;

        let rules = resp["rules"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<Rule>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(rules)
    }

    pub async fn patch_configs(&self, config: serde_json::Value) -> Result<(), reqwest::Error> {
        self.client()
            .patch(format!("{}/configs", self.base_url))
            .json(&config)
            .send()
            .await?;
        Ok(())
    }

    pub async fn delay_test(
        &self,
        proxy_name: &str,
        url: &str,
        timeout: u32,
    ) -> Result<u32, reqwest::Error> {
        let resp: serde_json::Value = self
            .client()
            .get(format!(
                "{}/proxies/{}/delay?url={}&timeout={}",
                self.base_url, proxy_name, url, timeout
            ))
            .send()
            .await?
            .json()
            .await?;

        Ok(resp["delay"].as_u64().unwrap_or(0) as u32)
    }
}
