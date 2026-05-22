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

#[derive(Debug, Clone, Deserialize)]
struct RulesResponse {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
struct Rule {
    #[allow(dead_code)]
    index: u32,
    #[serde(rename = "type")]
    rule_type: String,
    payload: String,
    proxy: String,
}

pub struct RulesPage {
    rules: Vec<Rule>,
    loading: bool,
    error: Option<String>,
}

impl RulesPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut me = Self {
            rules: Vec::new(),
            loading: true,
            error: None,
        };
        me.refresh(cx);
        me
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();

        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            let result = spawn_on_tokio(async { fetch_rules().await }).await;
            let _ = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.loading = false;
                        match result {
                            Ok(resp) => {
                                this.rules = resp.rules;
                                this.error = None;
                            }
                            Err(e) => {
                                this.error = Some(format!("{:#}", e));
                            }
                        }
                        cx.notify();
                    });
                }
            });
        })
        .detach();
    }
}

impl Render for RulesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(
                div().font_bold().text_lg().child(t("nav.rules").to_string()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} rules", self.rules.len())),
            )
            .child(div().flex_1())
            .child(
                Button::new("rules-refresh-btn")
                    .label(t("proxies.refresh"))
                    .compact()
                    .on_click(cx.listener(|this, _e, _w, cx| this.refresh(cx))),
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
            .child(div().w(px(40.)).child("#"))
            .child(div().w(px(140.)).child(t("rules.type")))
            .child(div().flex_1().child(t("rules.payload")))
            .child(div().w(px(180.)).child(t("rules.proxy")));

        let body: AnyElement = if self.loading && self.rules.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t("rules.loading").to_string())
                .into_any_element()
        } else if let Some(ref err) = self.error {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(hsla(0.0, 0.7, 0.5, 1.0))
                .child(err.clone())
                .into_any_element()
        } else {
            let rows = self.rules
                .iter()
                .map(|r| {
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
                                .w(px(40.))
                                .text_color(cx.theme().muted_foreground)
                                .child(r.index.to_string()),
                        )
                        .child(div().w(px(140.)).child(r.rule_type.clone()))
                        .child(
                            div()
                                .flex_1()
                                .overflow_x_hidden()
                                .child(r.payload.clone()),
                        )
                        .child(
                            div()
                                .w(px(180.))
                                .overflow_x_hidden()
                                .child(r.proxy.clone()),
                        )
                })
                .collect::<Vec<_>>();

            div()
                .id("rules-rows-scroll")
                .flex_1()
                .overflow_y_scroll()
                .child(v_flex().children(rows))
                .into_any_element()
        };

        v_flex()
            .size_full()
            .gap_3()
            .child(header)
            .child(table_header)
            .child(body)
    }
}

async fn fetch_rules() -> anyhow::Result<RulesResponse> {
    let client = reqwest::Client::builder().no_proxy().build()?;
    let resp = client
        .get(format!("{}/rules", MIHOMO_BASE))
        .send()
        .await?
        .json::<RulesResponse>()
        .await?;
    Ok(resp)
}
