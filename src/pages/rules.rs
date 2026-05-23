use std::rc::Rc;

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, InteractiveElementExt as _, StyledExt as _, VirtualListScrollHandle, h_flex,
    v_flex, v_virtual_list,
    button::Button,
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
    scroll_handle: VirtualListScrollHandle,
    /// Cell content shown in the full-text overlay; None when overlay is closed.
    expanded: Option<String>,
}

impl RulesPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut me = Self {
            rules: Vec::new(),
            loading: true,
            error: None,
            scroll_handle: VirtualListScrollHandle::new(),
            expanded: None,
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
                    .label(t("rules.refresh"))
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
            // Use virtual list for better performance with large rule sets.
            // ROW_H must match the row's own .h(...) below — the virtual
            // list positions rows by this size hint, so a too-small value
            // here makes successive rows overlap.
            const ROW_H: f32 = 32.;
            let item_count = self.rules.len();
            let item_sizes = Rc::new(vec![size(px(100.), px(ROW_H)); item_count]);

            let entity = cx.entity().clone();
            v_virtual_list(
                entity,
                "rules-virtual-list",
                item_sizes,
                move |this, range, _window, cx| {
                    range
                        .map(|i| {
                            let r = &this.rules[i];
                            let payload = r.payload.clone();
                            let proxy = r.proxy.clone();
                            let rule_type = r.rule_type.clone();

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
                                .child(
                                    div()
                                        .w(px(40.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(r.index.to_string()),
                                )
                                .child(fixed_cell(cx, ("rules-type", i), px(140.), rule_type))
                                .child(flex_cell(cx, ("rules-payload", i), payload))
                                .child(fixed_cell(cx, ("rules-proxy", i), px(180.), proxy))
                        })
                        .collect()
                },
            )
            .track_scroll(&self.scroll_handle)
            .into_any_element()
        };

        v_flex()
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

/// Fixed-width cell that truncates with ellipsis. Double-click opens the
/// full-text overlay so the user can read clipped content without
/// resizing the column.
fn fixed_cell(
    cx: &mut Context<RulesPage>,
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

/// Flex-1 cell variant — same truncation behaviour but consumes
/// remaining row width via the flex layout. `min_w_0` is required so
/// the flex item can shrink below its content's intrinsic size and let
/// `truncate` actually clip.
fn flex_cell(
    cx: &mut Context<RulesPage>,
    id: (&'static str, usize),
    text: String,
) -> impl IntoElement {
    let full = text.clone();
    div()
        .id(SharedString::from(format!("{}-{}", id.0, id.1)))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .truncate()
        .child(text)
        .on_double_click(cx.listener(move |this, _e, _w, cx| {
            this.expanded = Some(full.clone());
            cx.notify();
        }))
}

fn expanded_overlay(cx: &Context<RulesPage>, text: String) -> impl IntoElement {
    div()
        .id("rules-expanded-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(black().opacity(0.4))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _e, _w, cx| {
                this.expanded = None;
                cx.notify();
            }),
        )
        .child(
            div()
                .id("rules-expanded-card")
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
                .on_mouse_down(MouseButton::Left, |_e, _w, cx| {
                    cx.stop_propagation();
                }),
        )
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
