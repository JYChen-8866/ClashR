use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use gpui::*;
use gpui_component::{
    ActiveTheme, StyledExt as _, h_flex, v_flex,
    button::Button,
};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::i18n::t;
use crate::runtime::spawn_tokio_task;

const MIHOMO_WS: &str = "ws://127.0.0.1:9090/logs";
const MAX_LOGS: usize = 500;

#[derive(Debug, Clone, Deserialize)]
struct LogEntry {
    #[serde(rename = "type")]
    level: String,
    payload: String,
}

pub struct LogsPage {
    logs: VecDeque<LogEntry>,
    auto_scroll: bool,
    // Written by the tokio WebSocket task, drained by the gpui poll loop.
    // std::sync::Mutex so both sides can lock without an async context.
    incoming: Arc<Mutex<Vec<LogEntry>>>,
}

impl LogsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let incoming = Arc::new(Mutex::new(Vec::<LogEntry>::new()));
        let incoming_writer = incoming.clone();

        // WebSocket reader on the shared tokio runtime — no extra thread
        // or runtime needed (previously this spawned its own Runtime).
        spawn_tokio_task(async move {
            loop {
                match connect_async(MIHOMO_WS).await {
                    Ok((ws, _)) => {
                        let (_, mut read) = ws.split();
                        while let Some(Ok(Message::Text(text))) = read.next().await {
                            if let Ok(entry) = serde_json::from_str::<LogEntry>(&text) {
                                incoming_writer.lock().unwrap().push(entry);
                            }
                        }
                    }
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    }
                }
            }
        });

        // Drain the incoming queue into page state every 200 ms.
        let incoming_reader = incoming.clone();
        cx.spawn(async move |entity, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;

            let batch: Vec<LogEntry> = {
                let mut q = incoming_reader.lock().unwrap();
                std::mem::take(&mut *q)
            };
            if batch.is_empty() {
                continue;
            }

            let alive = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this: &mut LogsPage, cx| {
                        for entry in batch {
                            if this.logs.len() >= MAX_LOGS {
                                this.logs.pop_front();
                            }
                            this.logs.push_back(entry);
                        }
                        cx.notify();
                    });
                    true
                } else {
                    false
                }
            });
            if !alive {
                return;
            }
        })
        .detach();

        Self {
            logs: VecDeque::with_capacity(MAX_LOGS),
            auto_scroll: true,
            incoming,
        }
    }

    fn toggle_auto_scroll(&mut self, cx: &mut Context<Self>) {
        self.auto_scroll = !self.auto_scroll;
        cx.notify();
    }

    fn clear_logs(&mut self, cx: &mut Context<Self>) {
        self.logs.clear();
        cx.notify();
    }
}

impl Render for LogsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logs = &self.logs;

        let header = h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(div().font_bold().text_lg().child(t("nav.logs").to_string()))
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
