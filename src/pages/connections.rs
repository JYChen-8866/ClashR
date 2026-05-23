use std::{rc::Rc, time::Duration};

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, InteractiveElementExt as _, StyledExt as _, VirtualListScrollHandle, h_flex,
    v_flex, v_virtual_list,
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
    scroll_handle: VirtualListScrollHandle,
    /// Cell content shown in the full-text overlay; None when overlay is closed.
    expanded: Option<String>,
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
            scroll_handle: VirtualListScrollHandle::new(),
            expanded: None,
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

        // Use virtual list for better performance with many connections.
        // ROW_H must match the row's own .h(...) below — the virtual list
        // positions rows by this size hint, so a too-small value here
        // makes successive rows overlap.
        const ROW_H: f32 = 32.;
        let item_count = self.connections.len();
        let item_sizes = Rc::new(vec![size(px(100.), px(ROW_H)); item_count]);

        let entity = cx.entity().clone();
        let body = v_virtual_list(
            entity,
            "connections-virtual-list",
            item_sizes,
            move |this, range, _window, cx| {
                range
                    .map(|i| {
                        let conn = &this.connections[i];
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
                        let network = format!("{}/{}", conn.metadata.network, conn.metadata.conn_type);
                        let process = conn.metadata.process.clone();

                        h_flex()
                            .w_full()
                            .h(px(ROW_H))
                            .flex_shrink_0()
                            .px_3()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.5))
                            .child(cell(cx, ("conn-host", i), px(200.), host))
                            .child(cell(cx, ("conn-net", i), px(60.), network))
                            .child(cell(cx, ("conn-proc", i), px(120.), process))
                            .child(cell(cx, ("conn-rule", i), px(80.), rule_display))
                            .child(cell(cx, ("conn-chain", i), px(150.), chain))
                            .child(div().w(px(70.)).child(format_bytes(conn.upload)))
                            .child(div().w(px(70.)).child(format_bytes(conn.download)))
                    })
                    .collect()
            },
        )
        .track_scroll(&self.scroll_handle);

        v_flex()
            .id("connections-scroll")
            .size_full()
            .relative()
            .gap_3()
            .child(header)
            .child(table_header)
            .child(body)
            .when_some(self.expanded.clone(), |this, text| {
                this.child(expanded_overlay(cx, text))
            })
    }
}

/// A fixed-width cell that truncates with an ellipsis when content
/// overflows. Double-click opens the full-text overlay so the user can
/// read clipped content without resizing the column.
fn cell(
    cx: &mut Context<ConnectionsPage>,
    id: (&'static str, usize),
    width: Pixels,
    text: String,
) -> impl IntoElement {
    let full = text.clone();
    div()
        .id(SharedString::from(format!("{}-{}", id.0, id.1)))
        .w(width)
        .overflow_hidden()
        .whitespace_nowrap()
        .truncate()
        .child(text)
        .on_double_click(cx.listener(move |this, _e, _w, cx| {
            this.expanded = Some(full.clone());
            cx.notify();
        }))
}

fn expanded_overlay(cx: &Context<ConnectionsPage>, text: String) -> impl IntoElement {
    div()
        .id("conn-expanded-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(black().opacity(0.4))
        // Click outside the card dismisses the overlay.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _e, _w, cx| {
                this.expanded = None;
                cx.notify();
            }),
        )
        .child(
            div()
                .id("conn-expanded-card")
                .max_w(px(640.))
                .max_h(px(420.))
                .min_w(px(280.))
                .p_4()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .text_sm()
                .overflow_y_scroll()
                .child(text)
                // Don't dismiss when the user clicks inside the card.
                .on_mouse_down(MouseButton::Left, |_e, _w, cx| {
                    cx.stop_propagation();
                }),
        )
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
