use std::time::Duration;

use gpui::*;
use gpui_component::{
    ActiveTheme, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
};
use serde::Deserialize;

use crate::i18n::t;
use crate::runtime::spawn_on_tokio;

const MIHOMO_BASE: &str = "http://127.0.0.1:9090";
const POLL_INTERVAL_MS: u64 = 2000;

#[derive(Debug, Clone, Deserialize)]
struct ConnResponse {
    connections: Option<Vec<Connection>>,
    #[serde(rename = "downloadTotal")]
    download_total: u64,
    #[serde(rename = "uploadTotal")]
    upload_total: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct Connection {
    id: String,
    metadata: ConnMetadata,
    upload: u64,
    download: u64,
    #[allow(dead_code)]
    start: String,
    chains: Vec<String>,
    rule: String,
    #[serde(rename = "rulePayload")]
    rule_payload: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConnMetadata {
    network: String,
    #[serde(rename = "type")]
    conn_type: String,
    host: String,
    #[serde(rename = "destinationPort")]
    destination_port: String,
    process: String,
    #[serde(rename = "sourceIP")]
    #[allow(dead_code)]
    source_ip: String,
    #[serde(rename = "sourcePort")]
    #[allow(dead_code)]
    source_port: String,
}

pub struct ConnectionsPage {
    connections: Vec<Connection>,
    download_total: u64,
    upload_total: u64,
}

impl ConnectionsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            loop {
                let result = spawn_on_tokio(async { fetch_connections().await }).await;
                let alive = cx.update(|cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            if let Ok(resp) = result {
                                this.connections = resp.connections.unwrap_or_default();
                                this.download_total = resp.download_total;
                                this.upload_total = resp.upload_total;
                                cx.notify();
                            }
                        });
                        true
                    } else {
                        false
                    }
                });
                if !alive {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(POLL_INTERVAL_MS))
                    .await;
            }
        })
        .detach();

        Self {
            connections: Vec::new(),
            download_total: 0,
            upload_total: 0,
        }
    }

    fn close_all(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |_e, _cx| {
            let _ = spawn_on_tokio(async move {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .unwrap();
                let _ = client
                    .delete(format!("{}/connections", MIHOMO_BASE))
                    .send()
                    .await;
            })
            .await;
        })
        .detach();
    }
}

impl Render for ConnectionsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().font_bold().text_lg().child(t("nav.connections").to_string()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} {} | ↑ {} | ↓ {}",
                                self.connections.len(),
                                t("conn.active"),
                                format_bytes(self.upload_total),
                                format_bytes(self.download_total),
                            )),
                    ),
            )
            .child(
                Button::new("close-all-btn")
                    .label(t("conn.close_all"))
                    .compact()
                    .on_click(cx.listener(|this, _e, _w, cx| this.close_all(cx))),
            );

        let table_header = h_flex()
            .w_full()
            .px_3()
            .py_1p5()
            .gap_2()
            .bg(cx.theme().muted.opacity(0.3))
            .rounded_md()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(div().w(px(200.)).child(t("conn.host")))
            .child(div().w(px(60.)).child(t("conn.network")))
            .child(div().w(px(120.)).child(t("conn.process")))
            .child(div().w(px(80.)).child(t("conn.rule")))
            .child(div().w(px(150.)).child(t("conn.chains")))
            .child(div().w(px(70.)).child("↑"))
            .child(div().w(px(70.)).child("↓"));

        let rows = self.connections.iter().map(|conn| {
            let host = if conn.metadata.host.is_empty() {
                format!("{}:{}", conn.metadata.source_ip, conn.metadata.destination_port)
            } else {
                format!("{}:{}", conn.metadata.host, conn.metadata.destination_port)
            };
            let chain = conn.chains.first().cloned().unwrap_or_default();
            let rule_display = if conn.rule_payload.is_empty() {
                conn.rule.clone()
            } else {
                format!("{} ({})", conn.rule, conn.rule_payload)
            };

            h_flex()
                .w_full()
                .px_3()
                .py_1p5()
                .gap_2()
                .text_xs()
                .border_b_1()
                .border_color(cx.theme().border.opacity(0.5))
                .child(
                    div()
                        .w(px(200.))
                        .overflow_x_hidden()
                        .child(host),
                )
                .child(
                    div()
                        .w(px(60.))
                        .child(format!("{}/{}", conn.metadata.network, conn.metadata.conn_type)),
                )
                .child(
                    div()
                        .w(px(120.))
                        .overflow_x_hidden()
                        .child(conn.metadata.process.clone()),
                )
                .child(
                    div()
                        .w(px(80.))
                        .overflow_x_hidden()
                        .child(rule_display),
                )
                .child(
                    div()
                        .w(px(150.))
                        .overflow_x_hidden()
                        .child(chain),
                )
                .child(div().w(px(70.)).child(format_bytes(conn.upload)))
                .child(div().w(px(70.)).child(format_bytes(conn.download)))
        });

        v_flex()
            .id("connections-scroll")
            .size_full()
            .gap_3()
            .child(header)
            .child(table_header)
            .child(
                div()
                    .id("conn-rows-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(v_flex().children(rows)),
            )
    }
}

async fn fetch_connections() -> anyhow::Result<ConnResponse> {
    let client = reqwest::Client::builder().no_proxy().build()?;
    let resp = client
        .get(format!("{}/connections", MIHOMO_BASE))
        .send()
        .await?
        .json::<ConnResponse>()
        .await?;
    Ok(resp)
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
