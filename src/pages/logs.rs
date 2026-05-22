use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use gpui::*;
use gpui_component::{
    ActiveTheme, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::StreamExt;

use crate::i18n::t;

const MIHOMO_WS: &str = "ws://127.0.0.1:9090/logs";
const MAX_LOGS: usize = 500;

#[derive(Debug, Clone, Deserialize)]
struct LogEntry {
    #[serde(rename = "type")]
    level: String,
    payload: String,
}

pub struct LogsPage {
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
    auto_scroll: bool,
}

impl LogsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let logs = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOGS)));
        let logs_clone = logs.clone();

        // Spawn WebSocket reader in background thread with its own tokio runtime
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                loop {
                    match connect_async(MIHOMO_WS).await {
                        Ok((ws_stream, _)) => {
                            let (_, mut read) = ws_stream.split();
                            while let Some(msg) = read.next().await {
                                if let Ok(Message::Text(text)) = msg {
                                    if let Ok(entry) = serde_json::from_str::<LogEntry>(&text) {
                                        let mut logs = logs_clone.lock().unwrap();
                                        if logs.len() >= MAX_LOGS {
                                            logs.pop_front();
                                        }
                                        logs.push_back(entry);
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        }
                    }
                }
            });
        });

        Self {
            logs,
            auto_scroll: true,
        }
    }

    fn toggle_auto_scroll(&mut self, cx: &mut Context<Self>) {
        self.auto_scroll = !self.auto_scroll;
        cx.notify();
    }

    fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.logs.lock().unwrap().clear();
        cx.notify();
    }
}

impl Render for LogsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logs = self.logs.lock().unwrap().iter().cloned().collect::<Vec<_>>();

        let header = h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(
                div().font_bold().text_lg().child(t("nav.logs").to_string()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} lines", logs.len())),
            )
            .child(div().flex_1())
            .child(
                Button::new("logs-auto-scroll-btn")
                    .label(if self.auto_scroll {
                        t("logs.auto_scroll_on")
                    } else {
                        t("logs.auto_scroll_off")
                    })
                    .compact()
                    .on_click(cx.listener(|this, _e, _w, cx| this.toggle_auto_scroll(cx))),
            )
            .child(
                Button::new("logs-clear-btn")
                    .label(t("logs.clear"))
                    .compact()
                    .on_click(cx.listener(|this, _e, _w, cx| this.clear_logs(cx))),
            );

        let rows = logs.iter().map(|entry| {
            let color = match entry.level.as_str() {
                "error" => hsla(0.0, 0.7, 0.5, 1.0),
                "warning" => hsla(0.12, 0.8, 0.5, 1.0),
                "info" => cx.theme().muted_foreground,
                _ => cx.theme().foreground,
            };

            h_flex()
                .w_full()
                .px_2()
                .py_0p5()
                .gap_2()
                .text_xs()
                .font_family("monospace")
                .child(
                    div()
                        .w(px(60.))
                        .flex_shrink_0()
                        .text_color(color)
                        .child(entry.level.to_uppercase()),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(cx.theme().foreground)
                        .child(entry.payload.clone()),
                )
        });

        v_flex()
            .size_full()
            .gap_2()
            .child(header)
            .child(
                div()
                    .id("logs-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(v_flex().children(rows)),
            )
    }
}
