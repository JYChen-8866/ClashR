use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, Icon, IconName, StyledExt as _, VirtualListScrollHandle, h_flex, v_flex,
    v_virtual_list,
    button::{Button, ButtonVariants as _},
};
use tracing::{info, warn};

use crate::runtime::spawn_on_tokio;
use crate::services::mihomo_api::{self, ProxyNode};

/// Theme-driven brand color (was: hard-coded `#7F60D3`).
fn brand_color(cx: &App) -> Hsla {
    cx.theme().primary
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

/// Service brand aliases — extra keywords (English / abbrev.) that point
/// to the same Chinese-stemmed file under `icons/`.
fn service_aliases() -> &'static [(&'static str, &'static str)] {
    &[
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
    ]
}

/// Country/region aliases → ISO 3166-1 alpha-2 file stem (e.g. "香港" → "hk",
/// matching `icons/country/hk.svg`).
///
/// Country flags are reachable ONLY through this table, never through the
/// substring-match path used for service icons — two-letter ISO codes would
/// otherwise produce false positives on common words ("discord" contains
/// "is", which is Iceland; "studio" contains "tu", etc.).
fn country_aliases() -> &'static [(&'static str, &'static str)] {
    &[
        ("hk", "hk"), ("香港", "hk"), ("hong kong", "hk"), ("hongkong", "hk"), ("🇭🇰", "hk"),
        ("us", "us"), ("usa", "us"), ("美国", "us"), ("united states", "us"), ("🇺🇸", "us"),
        ("uk", "gb"), ("gb", "gb"), ("英国", "gb"), ("england", "gb"), ("british", "gb"), ("🇬🇧", "gb"),
        ("vn", "vn"), ("越南", "vn"), ("vietnam", "vn"), ("🇻🇳", "vn"),
        ("ca", "ca"), ("加拿大", "ca"), ("canada", "ca"), ("🇨🇦", "ca"),
        ("de", "de"), ("德国", "de"), ("germany", "de"), ("🇩🇪", "de"),
        ("sg", "sg"), ("新加坡", "sg"), ("singapore", "sg"), ("🇸🇬", "sg"),
        ("jp", "jp"), ("日本", "jp"), ("japan", "jp"), ("🇯🇵", "jp"),
        ("tw", "tw"), ("台湾", "tw"), ("台灣", "tw"), ("taiwan", "tw"), ("🇹🇼", "tw"),
        ("kr", "kr"), ("韩国", "kr"), ("韓國", "kr"), ("korea", "kr"), ("🇰🇷", "kr"),
        ("fr", "fr"), ("法国", "fr"), ("france", "fr"), ("🇫🇷", "fr"),
        ("ru", "ru"), ("俄罗斯", "ru"), ("俄国", "ru"), ("russia", "ru"), ("🇷🇺", "ru"),
        ("au", "au"), ("澳大利亚", "au"), ("澳洲", "au"), ("australia", "au"), ("🇦🇺", "au"),
        ("in", "in"), ("印度", "in"), ("india", "in"), ("🇮🇳", "in"),
        ("nl", "nl"), ("荷兰", "nl"), ("netherlands", "nl"), ("🇳🇱", "nl"),
        ("th", "th"), ("泰国", "th"), ("thailand", "th"), ("🇹🇭", "th"),
        ("my", "my"), ("马来西亚", "my"), ("malaysia", "my"), ("🇲🇾", "my"),
        ("ph", "ph"), ("菲律宾", "ph"), ("philippines", "ph"), ("🇵🇭", "ph"),
        ("id", "id"), ("印尼", "id"), ("印度尼西亚", "id"), ("indonesia", "id"), ("🇮🇩", "id"),
        ("tr", "tr"), ("土耳其", "tr"), ("turkey", "tr"), ("🇹🇷", "tr"),
        ("br", "br"), ("巴西", "br"), ("brazil", "br"), ("🇧🇷", "br"),
        ("ar", "ar"), ("阿根廷", "ar"), ("argentina", "ar"), ("🇦🇷", "ar"),
        ("it", "it"), ("意大利", "it"), ("italy", "it"), ("🇮🇹", "it"),
        ("es", "es"), ("西班牙", "es"), ("spain", "es"), ("🇪🇸", "es"),
        ("ch", "ch"), ("瑞士", "ch"), ("switzerland", "ch"), ("🇨🇭", "ch"),
        ("se", "se"), ("瑞典", "se"), ("sweden", "se"), ("🇸🇪", "se"),
        ("no", "no"), ("挪威", "no"), ("norway", "no"), ("🇳🇴", "no"),
        ("fi", "fi"), ("芬兰", "fi"), ("finland", "fi"), ("🇫🇮", "fi"),
        ("dk", "dk"), ("丹麦", "dk"), ("denmark", "dk"), ("🇩🇰", "dk"),
        ("ie", "ie"), ("爱尔兰", "ie"), ("ireland", "ie"), ("🇮🇪", "ie"),
        ("at", "at"), ("奥地利", "at"), ("austria", "at"), ("🇦🇹", "at"),
        ("be", "be"), ("比利时", "be"), ("belgium", "be"), ("🇧🇪", "be"),
        ("pt", "pt"), ("葡萄牙", "pt"), ("portugal", "pt"), ("🇵🇹", "pt"),
        ("nz", "nz"), ("新西兰", "nz"), ("new zealand", "nz"), ("🇳🇿", "nz"),
        ("ae", "ae"), ("阿联酋", "ae"), ("uae", "ae"), ("🇦🇪", "ae"),
        ("za", "za"), ("南非", "za"), ("south africa", "za"), ("🇿🇦", "za"),
        ("mx", "mx"), ("墨西哥", "mx"), ("mexico", "mx"), ("🇲🇽", "mx"),
        ("cn", "cn"), ("中国", "cn"), ("china", "cn"), ("🇨🇳", "cn"),
        ("mo", "mo"), ("澳门", "mo"), ("macau", "mo"), ("🇲🇴", "mo"),
    ]
}

/// Service icons live at the root of `icons/` (e.g. `icons/苹果.svg`,
/// `icons/github.svg`). Stems are matched by direct substring against the
/// proxy/group name.
fn service_icon_index() -> &'static HashMap<String, String> {
    static INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        let icons_dir = std::env::current_dir().unwrap_or_default().join("icons");
        let Ok(entries) = std::fs::read_dir(&icons_dir) else { return map };
        for entry in entries.flatten() {
            let path = entry.path();
            // Skip subdirectories (country flags + app icons live elsewhere).
            if !path.is_file() { continue; }
            if path.extension().and_then(|e| e.to_str()) != Some("svg") { continue; }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                map.insert(stem.to_lowercase(), format!("service-icons/{}.svg", stem));
            }
        }
        map
    })
}

/// Country flag icons live under `icons/country/` (e.g. `icons/country/hk.svg`).
/// Reachable only through `country_aliases()` to avoid spurious 2-letter
/// substring hits.
fn country_icon_index() -> &'static HashMap<String, String> {
    static INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        let dir = std::env::current_dir().unwrap_or_default().join("icons").join("country");
        let Ok(entries) = std::fs::read_dir(&dir) else { return map };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }
            if path.extension().and_then(|e| e.to_str()) != Some("svg") { continue; }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                map.insert(stem.to_lowercase(), format!("service-icons/country/{}.svg", stem));
            }
        }
        map
    })
}

/// Try to find an icon for a group/node name. Matching strategy:
/// 1. Direct: lowercased name contains a service-icon stem (brands like
///    "github", "youtube", or Chinese stems like "苹果").
/// 2. Service aliases: keyword in name → service-icon stem.
/// 3. Country aliases: keyword in name → ISO-2 stem under `country/`.
/// Returns the asset path (loadable via `service-icons/...`) or None.
fn service_icon_for_name(raw: &str) -> Option<&'static str> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Option<&'static str>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));

    {
        let cache_guard = cache.lock().unwrap();
        if let Some(cached) = cache_guard.get(raw) {
            return *cached;
        }
    }

    let lower = raw.to_lowercase();
    let svc = service_icon_index();
    let country = country_icon_index();

    let resolved: Option<&'static str> = (|| {
        // 1) Direct service stem hit.
        for (stem, path) in svc {
            if lower.contains(stem) {
                return Some(string_to_static(path));
            }
        }
        // 2) Service alias hit.
        for (keyword, target) in service_aliases() {
            if lower.contains(&keyword.to_lowercase()) || raw.contains(keyword) {
                if let Some(path) = svc.get(&target.to_lowercase()) {
                    return Some(string_to_static(path));
                }
            }
        }
        // 3) Country alias hit.
        for (keyword, target) in country_aliases() {
            if lower.contains(&keyword.to_lowercase()) || raw.contains(keyword) {
                if let Some(path) = country.get(&target.to_lowercase()) {
                    return Some(string_to_static(path));
                }
            }
        }
        None
    })();

    cache.lock().unwrap().insert(raw.to_string(), resolved);
    resolved
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
    expanded_groups: HashSet<String>,
    loading: bool,
    /// Set of group names currently being tested.
    testing_groups: HashSet<String>,
    scroll_handle: VirtualListScrollHandle,
    /// Latest viewport width — drives card-grid column count.
    viewport_width: Pixels,
    _bounds_subscription: Option<gpui::Subscription>,
}

impl ProxiesPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let viewport_width = window.viewport_size().width;

        // Re-render whenever the window resizes so the card grid can
        // recompute its column count.
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            let w = window.viewport_size().width;
            if this.viewport_width != w {
                this.viewport_width = w;
                cx.notify();
            }
        });

        let mut page = Self {
            groups: Vec::new(),
            all_proxies: HashMap::new(),
            expanded_groups: HashSet::new(),
            loading: false,
            testing_groups: HashSet::new(),
            scroll_handle: VirtualListScrollHandle::new(),
            viewport_width,
            _bounds_subscription: Some(bounds_subscription),
        };
        page.refresh(cx);
        page
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
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
                                // First load: open the first group so the user
                                // immediately sees content instead of an
                                // entirely collapsed list.
                                if this.expanded_groups.is_empty() {
                                    if let Some(first) = this.groups.first() {
                                        this.expanded_groups.insert(first.name.clone());
                                    }
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

    fn toggle_group(&mut self, group: String, cx: &mut Context<Self>) {
        if !self.expanded_groups.remove(&group) {
            self.expanded_groups.insert(group);
        }
        cx.notify();
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
    fn delay_color(delay: u32, _cx: &Context<Self>) -> Hsla {
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

    fn render_group_header(
        &self,
        group: &ProxyNode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_expanded = self.expanded_groups.contains(&group.name);
        let name = group.name.clone();
        let kind = group.kind.clone();
        let now = group.now.clone().unwrap_or_default();
        let count = group.all.len();

        h_flex()
            .id(SharedString::from(format!("group-header-{}", name)))
            .w_full()
            .px_3()
            .py_2p5()
            .gap_2()
            .items_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(cx.theme().muted.opacity(0.4)))
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _ev, _w, cx| {
                    this.toggle_group(name.clone(), cx);
                }
            }))
            .child(
                Icon::new(if is_expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(14.))
                .text_color(cx.theme().muted_foreground),
            )
            .child(Self::render_icon(
                &group.name,
                icon_for_group(&group.kind),
                cx.theme().muted_foreground,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(name.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .max_w(px(180.))
                    .child(now),
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
                    .child(format!("{} • {}", kind, count)),
            )
            .child({
                let group_for_test = name.clone();
                let is_testing = self.testing_groups.contains(&name);
                Button::new(SharedString::from(format!("test-{}", name)))
                    .icon(IconName::Loader)
                    .label(if is_testing {
                        crate::i18n::t("proxies.testing")
                    } else {
                        crate::i18n::t("proxies.test")
                    })
                    .compact()
                    .primary()
                    .loading(is_testing)
                    .on_click(cx.listener(move |this, ev, _w, cx| {
                        // Don't toggle the group when clicking the inline Test button.
                        cx.stop_propagation();
                        let _ = ev;
                        this.delay_test_group(group_for_test.clone(), cx);
                    }))
            })
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

        let bg = if is_current {
            brand_color(cx).opacity(0.12)
        } else {
            cx.theme().muted.opacity(0.2)
        };
        let border = if is_current {
            brand_color(cx)
        } else {
            cx.theme().border
        };

        let group_owned = group.to_string();
        let node_owned = node_name.to_string();

        v_flex()
            .id(SharedString::from(format!("node-{}-{}", group, node_name)))
            .h(px(NODE_CARD_HEIGHT))
            .px_3()
            .py_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .when(is_selectable, |el| {
                el.cursor_pointer()
                    .when(!is_current, |el| {
                        el.hover(|s| s.bg(cx.theme().muted.opacity(0.4)))
                    })
                    .on_click(cx.listener(move |this, _ev, _w, cx| {
                        this.select_node(group_owned.clone(), node_owned.clone(), cx);
                    }))
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(Self::render_icon(
                        node_name,
                        icon_for_node(&kind),
                        if is_current {
                            brand_color(cx)
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
                                brand_color(cx)
                            } else {
                                cx.theme().foreground
                            })
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(node_name.to_string()),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(kind),
                    )
                    .child({
                        let (label, color) = match delay {
                            Some(d) if d == 0 => ("—".to_string(), Self::delay_color(0, cx)),
                            Some(d) => (format!("{} ms", d), Self::delay_color(d, cx)),
                            None => (String::new(), cx.theme().muted_foreground),
                        };
                        div()
                            .text_xs()
                            .text_color(color)
                            .flex_shrink_0()
                            .child(label)
                    }),
            )
    }
}

/// Row in the flattened virtual-list model. Group headers and rows of
/// node cards share one scrolling list. Each `NodeCardRow` packs up to
/// `cols_per_row` cards horizontally; toggling a group just re-flattens.
#[derive(Clone, Copy)]
enum Row {
    GroupHeader { group_idx: usize },
    NodeCardRow {
        group_idx: usize,
        /// First node index packed into this row.
        start: usize,
        /// Card count in this row (≤ cols_per_row; tail rows can be shorter).
        count: usize,
    },
}

const GROUP_HEADER_HEIGHT: f32 = 52.;
/// Height of a single node card; used both for fixed virtual-list row
/// height and the card itself so they line up.
const NODE_CARD_HEIGHT: f32 = 56.;
/// Vertical space occupied by one card row (card + bottom gap).
const NODE_CARD_ROW_HEIGHT: f32 = NODE_CARD_HEIGHT + 8.;
/// Target card width — column count is computed from viewport.
const NODE_CARD_TARGET_WIDTH: f32 = 220.;
/// Horizontal padding inside the scrolling area.
const GRID_HORIZONTAL_PADDING: f32 = 12.;
/// Gap between adjacent cards in a row.
const NODE_CARD_GAP: f32 = 8.;

fn columns_per_row(viewport_width: Pixels) -> usize {
    let w = f32::from(viewport_width);
    let usable = (w - GRID_HORIZONTAL_PADDING * 2.0).max(NODE_CARD_TARGET_WIDTH);
    // Solve: n cards + (n-1) gaps ≤ usable → n ≤ (usable + gap) / (card + gap)
    let n = ((usable + NODE_CARD_GAP) / (NODE_CARD_TARGET_WIDTH + NODE_CARD_GAP)).floor() as usize;
    n.max(1)
}

impl Render for ProxiesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = h_flex()
            .items_center()
            .flex_shrink_0()
            .child(div().font_bold().text_lg().child(crate::i18n::t("nav.proxies")));

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
                                    crate::i18n::t("proxies.loading")
                                } else {
                                    crate::i18n::t("proxies.empty")
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    crate::i18n::t("proxies.empty_hint"),
                                ),
                        ),
                );
        }

        // Compute current grid column count from the latest viewport width.
        // We subtract a generous chunk to account for the sidebar etc; the
        // window observer keeps `viewport_width` up-to-date and the math
        // self-corrects on the next paint.
        let cols = columns_per_row(self.viewport_width);

        // Flatten groups + their (optionally visible) card rows.
        let mut rows: Vec<Row> = Vec::with_capacity(self.groups.len());
        let mut sizes: Vec<Size<Pixels>> = Vec::with_capacity(self.groups.len());
        for (gi, group) in self.groups.iter().enumerate() {
            rows.push(Row::GroupHeader { group_idx: gi });
            sizes.push(size(px(100.), px(GROUP_HEADER_HEIGHT)));
            if self.expanded_groups.contains(&group.name) {
                let total = group.all.len();
                let mut start = 0;
                while start < total {
                    let count = (total - start).min(cols);
                    rows.push(Row::NodeCardRow {
                        group_idx: gi,
                        start,
                        count,
                    });
                    sizes.push(size(px(100.), px(NODE_CARD_ROW_HEIGHT)));
                    start += count;
                }
            }
        }

        let rows = Rc::new(rows);
        let item_sizes = Rc::new(sizes);

        let entity = cx.entity().clone();
        let rows_for_closure = rows.clone();
        let virtual_list = v_virtual_list(
            entity,
            "proxies-virtual-list",
            item_sizes,
            move |this, range, _window, cx| {
                range
                    .map(|i| match rows_for_closure[i] {
                        Row::GroupHeader { group_idx } => {
                            let group = this.groups[group_idx].clone();
                            this.render_group_header(&group, cx).into_any_element()
                        }
                        Row::NodeCardRow { group_idx, start, count } => {
                            let group = &this.groups[group_idx];
                            let group_name = group.name.clone();
                            let is_selectable = group.is_user_selectable();
                            let current = group.now.clone();
                            let mut row = h_flex()
                                .w_full()
                                .px(px(GRID_HORIZONTAL_PADDING))
                                .py(px(NODE_CARD_GAP / 2.0))
                                .gap(px(NODE_CARD_GAP))
                                .items_stretch();
                            for offset in 0..count {
                                let node_name = group.all[start + offset].clone();
                                let is_current = current.as_deref() == Some(node_name.as_str());
                                let card = this
                                    .render_node_card(
                                        &group_name,
                                        &node_name,
                                        is_current,
                                        is_selectable,
                                        cx,
                                    )
                                    .into_any_element();
                                row = row.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(card),
                                );
                            }
                            // Pad remaining columns so cards in the last row
                            // keep the same width as fully-packed rows.
                            for _ in count..cols {
                                row = row.child(div().flex_1().min_w_0());
                            }
                            row.into_any_element()
                        }
                    })
                    .collect()
            },
        )
        .track_scroll(&self.scroll_handle);

        v_flex()
            .size_full()
            .gap_3()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(virtual_list),
            )
    }
}
