use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, Icon, IconName, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
};
use tracing::{info, warn};

use crate::runtime::spawn_on_tokio;
use crate::services::mihomo_api::{self, ProxyNode};

fn active_color() -> Hsla {
    hsla(0.73, 0.55, 0.6, 1.0)
}

/// Pick a representative icon for a proxy group's policy.
fn icon_for_group(kind: &str) -> IconName {
    match kind {
        "Selector" => IconName::CircleUser,
        "URLTest" => IconName::ChartPie,
        "LoadBalance" => IconName::ChartPie,
        "Fallback" => IconName::Heart,
        "Relay" => IconName::Network,
        "Smart" => IconName::Bot,
        _ => IconName::Folder,
    }
}

/// Pick an icon for a leaf proxy/node based on its protocol.
fn icon_for_node(kind: &str) -> IconName {
    match kind {
        "Direct" | "DIRECT" => IconName::ArrowRight,
        "Reject" | "REJECT" => IconName::CircleX,
        _ => IconName::Globe,
    }
}

/// Return a Unicode flag emoji for a country/region name. (Currently unused;
/// kept for future fallback if real SVG flag rendering breaks.)
#[allow(dead_code)]
fn country_emoji_for_name(raw: &str) -> Option<&'static str> {
    let lower = raw.to_lowercase();
    let matches = |needles: &[&str]| -> bool {
        needles.iter().any(|n| lower.contains(&n.to_lowercase()) || raw.contains(n))
    };

    if matches(&["香港", "hongkong", "hong kong", "🇭🇰"]) || raw.contains("HK") {
        return Some("🇭🇰");
    }
    if matches(&["美国", "united states", "🇺🇸", "usa"]) || raw.contains("US") {
        return Some("🇺🇸");
    }
    if matches(&["英国", "england", "british", "🇬🇧"]) || raw.contains("UK") {
        return Some("🇬🇧");
    }
    if matches(&["越南", "vietnam", "🇻🇳", "vn"]) {
        return Some("🇻🇳");
    }
    if matches(&["加拿大", "canada", "🇨🇦"]) || raw.contains("CA") {
        return Some("🇨🇦");
    }
    if matches(&["德国", "germany", "🇩🇪"]) || raw.contains("DE") {
        return Some("🇩🇪");
    }
    if matches(&["新加坡", "singapore", "🇸🇬"]) || raw.contains("SG") {
        return Some("🇸🇬");
    }
    if matches(&["日本", "japan", "🇯🇵"]) || raw.contains("JP") {
        return Some("🇯🇵");
    }
    if matches(&["台湾", "taiwan", "🇹🇼"]) || raw.contains("TW") {
        return Some("🇹🇼");
    }
    if matches(&["韩国", "korea", "🇰🇷"]) || raw.contains("KR") {
        return Some("🇰🇷");
    }

    None
}

/// Static keyword aliases — pointing extra names (English / emoji / abbrev.)
/// to the same icon stem we already have a file for.
fn aliases() -> &'static [(&'static str, &'static str)] {
    &[
        // 服务别名 → 文件名 stem
        ("apple", "苹果"),
        ("icloud", "苹果"),
        ("appstore", "苹果"),
        ("bilibili", "哔哩哔哩"),
        ("b站", "哔哩哔哩"),
        ("哔哩", "哔哩哔哩"),
        ("google", "谷歌"),
        ("youtube", "谷歌"),
        ("gmail", "谷歌"),
        ("油管", "谷歌"),
        ("microsoft", "微软"),
        ("msn", "微软"),
        ("outlook", "微软"),
        ("office", "微软"),
        ("azure", "微软"),
        ("tiktok", "抖音"),
        ("twitter", "推特"),
        ("x.com", "推特"),
        ("telegram", "电报"),
        ("tg", "电报"),
        ("whatsapp", "WhatsApp"),

        // 国家/地区
        ("hk", "香港"),
        ("hong kong", "香港"),
        ("hongkong", "香港"),
        ("🇭🇰", "香港"),
        ("us", "美国"),
        ("usa", "美国"),
        ("united states", "美国"),
        ("🇺🇸", "美国"),
        ("uk", "英国"),
        ("england", "英国"),
        ("british", "英国"),
        ("🇬🇧", "英国"),
        ("vn", "越南"),
        ("vietnam", "越南"),
        ("🇻🇳", "越南"),
        ("ca", "加拿大"),
        ("canada", "加拿大"),
        ("🇨🇦", "加拿大"),
        ("de", "德国"),
        ("germany", "德国"),
        ("🇩🇪", "德国"),
        ("sg", "新加坡"),
        ("singapore", "新加坡"),
        ("🇸🇬", "新加坡"),
        ("jp", "日本"),
        ("japan", "日本"),
        ("🇯🇵", "日本"),
    ]
}

/// stem → asset path (e.g. "苹果" → "service-icons/苹果.svg")
fn icon_index() -> &'static HashMap<String, String> {
    static INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        let icons_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("icons");
        scan_dir(&icons_dir, "service-icons", &mut map);
        map
    })
}

fn scan_dir(dir: &Path, prefix: &str, out: &mut HashMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let new_prefix = format!("{}/{}", prefix, name);
            scan_dir(&path, &new_prefix, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("svg") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let asset = format!("{}/{}.svg", prefix, stem);
                out.insert(stem.to_lowercase(), asset);
            }
        }
    }
}

/// Try to find an icon for a group/node name. Matching strategy:
/// 1. Direct: name lowercased contains any registered file stem
/// 2. Aliases: name contains an alias keyword → resolve to its target stem
/// Returns the asset path (loadable via `service-icons/...`) or None.
fn service_icon_for_name(raw: &str) -> Option<&'static str> {
    let index = icon_index();
    if index.is_empty() {
        return None;
    }
    let lower = raw.to_lowercase();

    // Direct stem hit (works for Chinese stems like "苹果").
    for (stem, path) in index {
        if lower.contains(stem) {
            // SAFETY: paths in the index live for the program lifetime.
            return Some(string_to_static(path));
        }
    }

    // Alias hit — keyword in `raw`, target a known stem.
    for (keyword, target) in aliases() {
        if lower.contains(&keyword.to_lowercase()) || raw.contains(keyword) {
            if let Some(path) = index.get(&target.to_lowercase()) {
                return Some(string_to_static(path));
            }
        }
    }

    None
}

/// `OnceLock<HashMap>` lives forever; convert &String into &'static str.
fn string_to_static(s: &str) -> &'static str {
    // Safety: the strings live in icon_index()'s OnceLock-backed map for the
    // process lifetime, so transmuting their lifetime to 'static is sound.
    unsafe { std::mem::transmute::<&str, &'static str>(s) }
}

pub struct ProxiesPage {
    groups: Vec<ProxyNode>,
    all_proxies: HashMap<String, ProxyNode>,
    selected_group: Option<String>,
    loading: bool,
    /// Set of group names currently being tested.
    testing_groups: std::collections::HashSet<String>,
}

impl ProxiesPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            groups: Vec::new(),
            all_proxies: HashMap::new(),
            selected_group: None,
            loading: false,
            testing_groups: std::collections::HashSet::new(),
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

    /// Color thresholds for displayed delay.
    ///   <100ms   → green
    ///   100-300  → amber
    ///   300+     → orange
    ///   0 (fail) → red
    fn delay_color(delay: u32, cx: &Context<Self>) -> Hsla {
        if delay == 0 {
            hsla(0.0, 0.7, 0.5, 1.0) // red — request failed / timed out
        } else if delay < 100 {
            hsla(0.32, 0.6, 0.45, 1.0) // green
        } else if delay <= 300 {
            hsla(0.13, 0.85, 0.5, 1.0) // amber/yellow
        } else {
            hsla(0.07, 0.9, 0.55, 1.0) // orange
        }
    }

    /// Color for the "未测试" placeholder before any history sample exists.
    fn unknown_delay_color(cx: &Context<Self>) -> Hsla {
        cx.theme().muted_foreground
    }

    /// Run a delay test for every node in `group` in parallel. Results are
    /// pushed back into `all_proxies[node].history` so the UI shows them.
    fn delay_test_group(&mut self, group: String, cx: &mut Context<Self>) {
        if self.testing_groups.contains(&group) {
            return;
        }
        let group_node = match self.groups.iter().find(|g| g.name == group).cloned() {
            Some(g) => g,
            None => return,
        };

        let nodes = group_node.all.clone();
        if nodes.is_empty() {
            return;
        }

        info!(group = %group, count = nodes.len(), "starting group delay test");
        self.testing_groups.insert(group.clone());
        cx.notify();

        let group_for_async = group.clone();
        cx.spawn(async move |entity, cx| {
            let results = spawn_on_tokio(async move {
                let mut handles = Vec::new();
                for node in nodes {
                    let n = node.clone();
                    handles.push(tokio::spawn(async move {
                        let res = mihomo_api::delay_test(
                            &n,
                            "https://www.gstatic.com/generate_204",
                            5000,
                        )
                        .await;
                        // 0 means "failed/timeout" in the rest of our UI.
                        let delay = res.unwrap_or(0);
                        (n, delay)
                    }));
                }
                let mut results = Vec::new();
                for h in handles {
                    if let Ok(r) = h.await {
                        results.push(r);
                    }
                }
                results
            }).await;

            cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        for (name, delay) in results {
                            if let Some(node) = this.all_proxies.get_mut(&name) {
                                node.history.push(crate::services::mihomo_api::DelaySample {
                                    time: String::new(),
                                    delay,
                                });
                            }
                        }
                        this.testing_groups.remove(&group_for_async);
                        cx.notify();
                    });
                }
            });
        }).detach();
    }

    /// Render a 14px icon for a proxy/group:
    ///   1) Service-matched → `img()` from icons/ (preserves multi-color SVGs
    ///      like flags, brand logos)
    ///   2) Otherwise → kind-based fallback IconName (single color via svg())
    fn render_icon(name: &str, fallback: IconName, color: Hsla) -> AnyElement {
        if let Some(path) = service_icon_for_name(name) {
            return img(path)
                .size(px(14.))
                .into_any_element();
        }
        Icon::new(fallback)
            .size(px(14.))
            .text_color(color)
            .into_any_element()
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
                    .child(Self::render_icon(
                        &group.name,
                        icon_for_group(&group.kind),
                        if is_selected {
                            active_color()
                        } else {
                            cx.theme().muted_foreground
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(if is_selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .overflow_hidden()
                            .whitespace_nowrap()
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
                            .flex_shrink_0()
                            .child(kind),
                    ),
            )
            .child(
                div()
                    .pl_5()
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
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(Self::render_icon(
                                node_name,
                                icon_for_node(&kind),
                                if is_current {
                                    active_color()
                                } else {
                                    cx.theme().muted_foreground
                                },
                            ))
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
                            ),
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
                    .pl_5()
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

            let is_testing = self.testing_groups.contains(&group_name);
            let group_for_test = group_name.clone();

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
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .when(!is_selectable, |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("auto-selected"),
                            )
                        })
                        .child(
                            Button::new(SharedString::from(format!("test-{}", group_name)))
                                .icon(IconName::Loader)
                                .label(if is_testing { "Testing…" } else { "Test" })
                                .compact()
                                .ghost()
                                .loading(is_testing)
                                .on_click(cx.listener(move |this, _ev, _w, cx| {
                                    this.delay_test_group(group_for_test.clone(), cx);
                                })),
                        ),
                );

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
