use std::collections::HashMap;

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, IconName, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
};
use tracing::warn;

use crate::runtime::spawn_on_tokio;
use crate::services::mihomo_api::{self, ProxyNode};

fn active_color() -> Hsla {
    hsla(0.73, 0.55, 0.6, 1.0)
}

pub struct ProxiesPage {
    groups: Vec<ProxyNode>,
    all_proxies: HashMap<String, ProxyNode>,
    selected_group: Option<String>,
    loading: bool,
}

impl ProxiesPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            groups: Vec::new(),
            all_proxies: HashMap::new(),
            selected_group: None,
            loading: false,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        cx.notify();

        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async move { mihomo_api::get_proxies().await }).await;

            cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.loading = false;
                        match result {
                            Ok(proxies) => {
                                this.all_proxies = proxies.clone();
                                let mut groups: Vec<ProxyNode> = proxies
                                    .values()
                                    .filter(|p| p.is_group())
                                    .cloned()
                                    .collect();
                                groups.sort_by(|a, b| a.name.cmp(&b.name));
                                this.groups = groups;
                                if this.selected_group.is_none() {
                                    this.selected_group = this.groups.first().map(|g| g.name.clone());
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "failed to load proxies");
                            }
                        }
                        cx.notify();
                    });
                }
            });
        }).detach();
    }

    fn select_node(&mut self, group: String, node: String, cx: &mut Context<Self>) {
        // Optimistic UI update
        if let Some(g) = self.all_proxies.get_mut(&group) {
            g.now = Some(node.clone());
        }
        if let Some(g) = self.groups.iter_mut().find(|g| g.name == group) {
            g.now = Some(node.clone());
        }
        cx.notify();

        let group_clone = group.clone();
        let node_clone = node.clone();
        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async move {
                mihomo_api::select_proxy(&group_clone, &node_clone).await
            }).await;

            if let Err(e) = result {
                warn!(group = %group, node = %node, error = %e, "select_proxy failed, refreshing");
            }

            // Refresh state from server to reconcile
            cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| this.refresh(cx));
                }
            });
        }).detach();
    }

    fn delay_color(delay: u32, cx: &Context<Self>) -> Hsla {
        if delay == 0 {
            cx.theme().muted_foreground
        } else if delay < 200 {
            hsla(0.32, 0.6, 0.45, 1.0) // green
        } else if delay < 500 {
            hsla(0.12, 0.7, 0.5, 1.0) // amber
        } else {
            hsla(0.0, 0.7, 0.5, 1.0) // red
        }
    }

    fn render_group_item(
        &self,
        group: &ProxyNode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = self.selected_group.as_deref() == Some(&group.name);
        let name = group.name.clone();
        let kind = group.kind.clone();
        let now = group.now.clone().unwrap_or_default();

        let bg = if is_selected {
            active_color().opacity(0.1)
        } else {
            cx.theme().transparent
        };
        let border = if is_selected {
            active_color()
        } else {
            cx.theme().transparent
        };

        v_flex()
            .id(SharedString::from(format!("group-{}", name)))
            .px_3()
            .py_2()
            .gap_0p5()
            .rounded_md()
            .border_l_2()
            .border_color(border)
            .bg(bg)
            .cursor_pointer()
            .when(!is_selected, |el| {
                el.hover(|s| s.bg(cx.theme().muted.opacity(0.3)))
            })
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _ev, _w, cx| {
                    this.selected_group = Some(name.clone());
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(if is_selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(group.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .child(kind),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(now),
            )
    }

    fn render_node_card(
        &self,
        group: &str,
        node_name: &str,
        is_current: bool,
        is_selectable: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let node = self.all_proxies.get(node_name).cloned();
        let kind = node
            .as_ref()
            .map(|n| n.kind.clone())
            .unwrap_or_else(|| "?".to_string());
        let delay = node.as_ref().and_then(|n| n.last_delay());

        let border_color = if is_current {
            active_color()
        } else {
            cx.theme().border
        };

        let group_owned = group.to_string();
        let node_owned = node_name.to_string();

        v_flex()
            .id(SharedString::from(format!("node-{}-{}", group, node_name)))
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .bg(cx.theme().background)
            .when(is_selectable, |el| {
                el.cursor_pointer()
                    .when(!is_current, |el| {
                        el.hover(|s| s.bg(cx.theme().muted.opacity(0.3)))
                    })
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        this.select_node(group_owned.clone(), node_owned.clone(), cx);
                    }))
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(if is_current {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if is_current {
                                active_color()
                            } else {
                                cx.theme().foreground
                            })
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(node_name.to_string()),
                    )
                    .when_some(delay, |el, d| {
                        let label = if d == 0 {
                            "—".to_string()
                        } else {
                            format!("{} ms", d)
                        };
                        el.child(
                            div()
                                .text_xs()
                                .text_color(Self::delay_color(d, cx))
                                .flex_shrink_0()
                                .child(label),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(kind),
            )
    }
}

impl Render for ProxiesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .justify_between()
            .items_center()
            .flex_shrink_0()
            .child(div().font_bold().text_lg().child("Proxies"))
            .child(
                Button::new("refresh-proxies")
                    .icon(IconName::Redo)
                    .label("Refresh")
                    .compact()
                    .ghost()
                    .on_click(cx.listener(|this, _ev, _w, cx| this.refresh(cx))),
            );

        if self.groups.is_empty() {
            return v_flex()
                .size_full()
                .gap_4()
                .child(header)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .text_base()
                                .child(if self.loading {
                                    "Loading..."
                                } else {
                                    "No proxy groups"
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "Make sure the core is running and a profile is active.",
                                ),
                        ),
                );
        }

        let selected_group_name = self.selected_group.clone();
        let selected_group: Option<ProxyNode> = selected_group_name
            .as_deref()
            .and_then(|n| self.groups.iter().find(|g| g.name == n))
            .cloned();

        let groups_panel = v_flex()
            .id("groups-list")
            .w(px(220.))
            .flex_shrink_0()
            .h_full()
            .gap_1()
            .pr_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .overflow_y_scroll()
            .children(self.groups.iter().map(|g| self.render_group_item(g, cx)));

        let nodes_panel: AnyElement = if let Some(group) = selected_group {
            let group_name = group.name.clone();
            let current = group.now.clone();
            let is_selectable = group.is_user_selectable();

            let header_row = h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .pb_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(group_name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} • {} nodes",
                                    group.kind,
                                    group.all.len()
                                )),
                        ),
                )
                .when(!is_selectable, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("auto-selected by group policy"),
                    )
                });

            let group_for_cards = group_name.clone();
            v_flex()
                .flex_1()
                .h_full()
                .min_w_0()
                .pl_4()
                .gap_2()
                .child(header_row)
                .child(
                    div()
                        .id("nodes-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .children(group.all.iter().map(|name| {
                                    let is_current = current.as_deref() == Some(name.as_str());
                                    div()
                                        .flex_basis(px(220.))
                                        .flex_grow()
                                        .max_w(px(280.))
                                        .child(self.render_node_card(
                                            &group_for_cards,
                                            name,
                                            is_current,
                                            is_selectable,
                                            cx,
                                        ))
                                })),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .pl_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Select a group on the left.")
                .into_any_element()
        };

        v_flex()
            .size_full()
            .gap_3()
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(groups_panel)
                    .child(nodes_panel),
            )
    }
}
