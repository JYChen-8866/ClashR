//! Home page — real-time traffic chart, summary stat cards, website
//! latency tester, and IP info.
//!
//! Data flow:
//! - Every 1 s the page polls mihomo's `/connections` and `/memory` HTTP
//!   endpoints. `/connections` gives us the cumulative up/down totals (we
//!   diff them to derive instantaneous speed) plus the active connection
//!   count; `/memory` gives kernel memory.
//! - The website latency tester fires HTTP HEAD requests through
//!   `127.0.0.1:7890` (mihomo's mixed port) on demand.
//! - The IP info card hits `https://ipinfo.io/json` through the same
//!   proxy, so the answer reflects the egress IP that mihomo's selected
//!   node currently presents.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, IconName, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
    chart::AreaChart,
    popover::Popover,
    switch::Switch,
};
use serde::Deserialize;
use tracing::{info, warn};

use crate::core::sysproxy;
use crate::runtime::spawn_on_tokio;
use crate::theming::Preferences;

const TRAFFIC_SAMPLE_LIMIT: usize = 600; // 10 min at 1 Hz
const POLL_INTERVAL_MS: u64 = 1000;
const PROXY_URL: &str = "http://127.0.0.1:7890";
const MIHOMO_BASE: &str = "http://127.0.0.1:9090";
const SYS_PROXY_HOST: &str = "127.0.0.1";
const SYS_PROXY_PORT: u16 = 7890;

#[derive(Clone, Copy, Default)]
struct TrafficSample {
    seq: u32,
    up: u64,
    down: u64,
}

#[derive(Clone)]
struct SiteTest {
    name: &'static str,
    url: &'static str,
    icon_stem: Option<&'static str>,
    state: TestState,
}

#[derive(Clone)]
enum TestState {
    Idle,
    Testing,
    Ok(u32),
    Failed,
}

#[derive(Clone)]
enum IpState {
    Loading,
    Loaded(IpInfo),
    Failed(String),
}

#[derive(Clone, Debug, Deserialize, Default)]
struct IpInfo {
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
}

pub struct HomePage {
    seq: u32,
    samples: VecDeque<TrafficSample>,
    last_total: Option<(u64, u64)>,

    pub cur_up_speed: u64,
    pub cur_down_speed: u64,
    pub cur_up_total: u64,
    pub cur_down_total: u64,
    pub cur_connections: u32,
    pub cur_memory: u64,

    mode: Option<String>,
    global_now: Option<String>,
    global_all: Vec<String>,
    tun_enabled: bool,
    system_proxy_on: bool,

    sites: Vec<SiteTest>,
    ip_state: IpState,
    show_ip: bool,
}

impl HomePage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sites = vec![
            SiteTest { name: "Apple",   url: "https://www.apple.com",   icon_stem: Some("苹果"),   state: TestState::Idle },
            SiteTest { name: "GitHub",  url: "https://github.com",      icon_stem: None,            state: TestState::Idle },
            SiteTest { name: "Google",  url: "https://www.google.com",  icon_stem: Some("谷歌"),   state: TestState::Idle },
            SiteTest { name: "YouTube", url: "https://www.youtube.com", icon_stem: Some("谷歌"),   state: TestState::Idle },
        ];

        // Kick off the polling loop. We tick once per second.
        cx.spawn(async move |entity, cx| {
            loop {
                let stats = spawn_on_tokio(async { fetch_stats().await }).await;
                let mode_global = spawn_on_tokio(async { fetch_mode_and_global().await }).await;

                let alive = cx.update(|cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this: &mut HomePage, cx| {
                            this.ingest_stats(stats);
                            if let Ok((mode, now, all, tun)) = mode_global {
                                this.mode = Some(mode);
                                this.global_now = now;
                                this.global_all = all;
                                this.tun_enabled = tun;
                            }
                            cx.notify();
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

        // Fire IP lookup after a delay — mihomo needs a few seconds to
        // fully start and accept proxy connections.
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            // Wait for mihomo's external controller to be reachable.
            spawn_on_tokio(async {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_millis(500))
                    .build()
                    .unwrap();
                for _ in 0..15 {
                    if client
                        .get(format!("{}/configs", MIHOMO_BASE))
                        .send()
                        .await
                        .is_ok()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
            .await;

            let result = spawn_on_tokio(async { fetch_ip_info().await }).await;
            let _ = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.ip_state = match result {
                            Ok(info) => IpState::Loaded(info),
                            Err(e) => IpState::Failed(format!("{:#}", e)),
                        };
                        cx.notify();
                    });
                }
            });
        })
        .detach();

        Self {
            seq: 0,
            samples: VecDeque::with_capacity(TRAFFIC_SAMPLE_LIMIT),
            last_total: None,
            cur_up_speed: 0,
            cur_down_speed: 0,
            cur_up_total: 0,
            cur_down_total: 0,
            cur_connections: 0,
            cur_memory: 0,
            mode: None,
            global_now: None,
            global_all: Vec::new(),
            tun_enabled: false,
            system_proxy_on: Preferences::load().system_proxy_enabled,
            sites,
            ip_state: IpState::Loading,
            show_ip: false,
        }
    }

    pub fn speeds(&self) -> (String, String) {
        (format_speed(self.cur_up_speed), format_speed(self.cur_down_speed))
    }

    fn ingest_stats(&mut self, result: Result<RawStats, String>) {
        let stats = match result {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "stats poll failed");
                return;
            }
        };

        // Log the first successful sample so it's obvious in the log when
        // polling is alive vs. silently dead.
        if self.last_total.is_none() {
            info!(
                up = stats.upload_total,
                down = stats.download_total,
                conn = stats.connections_count,
                mem = stats.memory_inuse,
                "first stats poll succeeded"
            );
        }

        // Speed is the delta between successive cumulative-total samples.
        // First poll has nothing to compare against, so we just record and
        // wait for the next.
        let speed = if let Some((last_up, last_down)) = self.last_total {
            (
                stats.upload_total.saturating_sub(last_up),
                stats.download_total.saturating_sub(last_down),
            )
        } else {
            (0, 0)
        };

        self.last_total = Some((stats.upload_total, stats.download_total));
        self.cur_up_speed = speed.0;
        self.cur_down_speed = speed.1;
        self.cur_up_total = stats.upload_total;
        self.cur_down_total = stats.download_total;
        self.cur_connections = stats.connections_count;
        self.cur_memory = stats.memory_inuse;

        self.seq = self.seq.wrapping_add(1);
        if self.samples.len() >= TRAFFIC_SAMPLE_LIMIT {
            self.samples.pop_front();
        }
        self.samples.push_back(TrafficSample {
            seq: self.seq,
            up: speed.0,
            down: speed.1,
        });
    }

    fn refresh_ip(&mut self, cx: &mut Context<Self>) {
        self.ip_state = IpState::Loading;
        cx.notify();
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            let result = spawn_on_tokio(async { fetch_ip_info().await }).await;
            let _ = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.ip_state = match result {
                            Ok(info) => IpState::Loaded(info),
                            Err(e) => IpState::Failed(format!("{:#}", e)),
                        };
                        cx.notify();
                    });
                }
            });
        })
        .detach();
    }

    fn set_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        if self.mode.as_deref() == Some(mode) {
            return;
        }
        info!(mode, "switching mihomo mode");
        // Optimistic update so the segmented control is responsive.
        self.mode = Some(mode.to_string());
        cx.notify();

        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            let result = spawn_on_tokio(async move { patch_mode(mode).await }).await;
            if let Err(e) = result {
                warn!(error = %e, "set_mode failed; reconciling from server");
                let _ = cx.update(|cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, _cx| {
                            // Force a re-fetch on next tick by clearing.
                            this.mode = None;
                        });
                    }
                });
            }
        })
        .detach();
    }

    fn set_global_node(&mut self, name: String, cx: &mut Context<Self>) {
        if self.global_now.as_deref() == Some(&name) {
            return;
        }
        info!(node = %name, "switching GLOBAL node");
        self.global_now = Some(name.clone());
        cx.notify();
        cx.spawn(async move |_e, _cx| {
            if let Err(e) = spawn_on_tokio(async move { put_global_node(&name).await }).await {
                warn!(error = %e, "set_global_node failed");
            }
        })
        .detach();
    }

    fn toggle_system_proxy(&mut self, on: bool, cx: &mut Context<Self>) {
        self.system_proxy_on = on;
        let prefs = Preferences {
            system_proxy_enabled: on,
            ..Preferences::load()
        };
        prefs.save();
        cx.notify();

        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            let result = spawn_on_tokio(async move {
                if on {
                    sysproxy::enable(SYS_PROXY_HOST, SYS_PROXY_PORT)
                } else {
                    sysproxy::disable()
                }
            })
            .await;

            if let Err(e) = result {
                warn!(error = %e, on, "system proxy toggle failed");
                let _ = cx.update(|cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.system_proxy_on = !on;
                            let prefs = Preferences {
                                system_proxy_enabled: !on,
                                ..Preferences::load()
                            };
                            prefs.save();
                            cx.notify();
                        });
                    }
                });
            }
        })
        .detach();
    }

    fn toggle_tun(&mut self, on: bool, cx: &mut Context<Self>) {
        info!(on, "toggling TUN mode");
        self.tun_enabled = on;
        cx.notify();
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            let result = spawn_on_tokio(async move { patch_tun(on).await }).await;
            if let Err(e) = result {
                warn!(error = %e, "TUN toggle failed; reconciling on next poll");
                let _ = cx.update(|cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.tun_enabled = !on;
                            cx.notify();
                        });
                    }
                });
            }
        })
        .detach();
    }

    fn set_network_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        match mode {
            "off" => {
                if self.tun_enabled {
                    self.toggle_tun(false, cx);
                }
                if self.system_proxy_on {
                    self.toggle_system_proxy(false, cx);
                }
            }
            "sysproxy" => {
                if self.tun_enabled {
                    self.toggle_tun(false, cx);
                }
                if !self.system_proxy_on {
                    self.toggle_system_proxy(true, cx);
                }
            }
            "tun" => {
                if self.system_proxy_on {
                    self.toggle_system_proxy(false, cx);
                }
                if !self.tun_enabled {
                    self.toggle_tun(true, cx);
                }
            }
            _ => {}
        }
    }

    fn run_site_test(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.sites.len() {
            return;
        }
        self.sites[idx].state = TestState::Testing;
        let url = self.sites[idx].url.to_string();
        cx.notify();

        let entity = cx.entity().downgrade();
        cx.spawn(async move |_e, cx| {
            let result = spawn_on_tokio(async move { measure_latency(&url).await }).await;
            let _ = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        if let Some(site) = this.sites.get_mut(idx) {
                            site.state = match result {
                                Ok(ms) => TestState::Ok(ms),
                                Err(_) => TestState::Failed,
                            };
                            cx.notify();
                        }
                    });
                }
            });
        })
        .detach();
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top_row = h_flex()
            .gap_4()
            .items_stretch()
            .child(div().flex_1().child(self.mode_section(cx)))
            .child(div().flex_1().child(self.network_section(cx)));

        v_flex()
            .id("home-scroll")
            .size_full()
            .gap_4()
            .overflow_y_scroll()
            .child(top_row)
            .child(self.traffic_section(cx))
            .child(self.sites_section(cx))
            .child(self.ip_section(cx))
    }
}

impl HomePage {
    fn mode_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let current = self.mode.as_deref().unwrap_or("");
        let mode_btn = |key: &'static str, label: &'static str| {
            let active = current == key;
            let primary = cx.theme().primary;
            v_flex()
                .id(SharedString::from(format!("mode-{}", key)))
                .px_4()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(active, |el| {
                    el.bg(primary).text_color(cx.theme().primary_foreground)
                })
                .when(!active, |el| {
                    el.text_color(cx.theme().foreground)
                        .hover(|s| s.bg(cx.theme().muted.opacity(0.5)))
                })
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _ev, _w, cx| {
                    this.set_mode(key, cx);
                }))
        };

        let segmented = h_flex()
            .gap_1()
            .p_1()
            .rounded_md()
            .bg(cx.theme().muted.opacity(0.4))
            .child(mode_btn("rule", crate::i18n::t("home.mode_rule")))
            .child(mode_btn("global", crate::i18n::t("home.mode_global")))
            .child(mode_btn("direct", crate::i18n::t("home.mode_direct")));

        // Show the GLOBAL group node picker only in global mode — that's the
        // only mode where this selection actually routes traffic.
        let body: AnyElement = if current == "global" {
            v_flex()
                .gap_3()
                .child(segmented)
                .child(self.global_node_picker(cx))
                .into_any_element()
        } else {
            segmented.into_any_element()
        };

        section_card(cx, crate::i18n::t("home.proxy_mode"), body)
    }

    fn network_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let current = if self.tun_enabled {
            "tun"
        } else if self.system_proxy_on {
            "sysproxy"
        } else {
            "off"
        };

        let net_btn = |key: &'static str, label: &'static str| {
            let active = current == key;
            let primary = cx.theme().primary;
            v_flex()
                .id(SharedString::from(format!("net-{}", key)))
                .px_4()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(active, |el| {
                    el.bg(primary).text_color(cx.theme().primary_foreground)
                })
                .when(!active, |el| {
                    el.text_color(cx.theme().foreground)
                        .hover(|s| s.bg(cx.theme().muted.opacity(0.5)))
                })
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _ev, _w, cx| {
                    this.set_network_mode(key, cx);
                }))
        };

        let segmented = h_flex()
            .gap_1()
            .p_1()
            .rounded_md()
            .bg(cx.theme().muted.opacity(0.4))
            .child(net_btn("off", crate::i18n::t("home.net_off")))
            .child(net_btn("sysproxy", crate::i18n::t("home.net_sysproxy")))
            .child(net_btn("tun", crate::i18n::t("home.net_tun")));

        section_card(cx, crate::i18n::t("home.network"), segmented.into_any_element())
    }

    fn global_node_picker(&self, cx: &Context<Self>) -> impl IntoElement {
        let now = self.global_now.clone().unwrap_or_else(|| "—".into());
        let nodes = self.global_all.clone();

        let trigger = Button::new("global-node-picker")
            .label(SharedString::from(now.clone()))
            .icon(IconName::ChevronDown)
            .compact();

        let popover = Popover::new("global-node-popover")
            .trigger(trigger)
            .content(move |_state, _w, cx| {
                let nodes = nodes.clone();
                let now = now.clone();
                div()
                    .id("global-node-list")
                    .py_1()
                    .min_w(px(260.))
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(v_flex().children(nodes.into_iter().map(|name| {
                        let is_current = name == now;
                        let label = name.clone();
                        let n = name.clone();
                        h_flex()
                            .id(SharedString::from(format!("gnode-{}", name)))
                            .px_3()
                            .py_1p5()
                            .gap_2()
                            .items_center()
                            .cursor_pointer()
                            .when(is_current, |el| el.bg(cx.theme().accent.opacity(0.15)))
                            .hover(|s| s.bg(cx.theme().accent.opacity(0.10)))
                            .on_mouse_down(MouseButton::Left, {
                                let n = n.clone();
                                move |_ev, _w, cx| {
                                    let target = n.clone();
                                    cx.spawn(async move |_cx| {
                                        if let Err(e) = spawn_on_tokio(async move {
                                            put_global_node(&target).await
                                        })
                                        .await
                                        {
                                            warn!(error = %e, "popover set GLOBAL failed");
                                        }
                                    })
                                    .detach();
                                }
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .when(is_current, |el| {
                                        el.font_weight(FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().accent)
                                    })
                                    .child(label),
                            )
                            .when(is_current, |el| {
                                el.child(
                                    gpui_component::Icon::new(IconName::Check)
                                        .size(px(14.))
                                        .text_color(cx.theme().accent),
                                )
                            })
                    })))
            });

        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("GLOBAL 节点"),
            )
            .child(popover)
    }

    fn traffic_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let cards = h_flex()
            .gap_3()
            .flex_wrap()
            .child(self.stat_card(crate::i18n::t("home.upload_speed"), &format_speed(self.cur_up_speed), cx))
            .child(self.stat_card(crate::i18n::t("home.download_speed"), &format_speed(self.cur_down_speed), cx))
            .child(self.stat_card(crate::i18n::t("home.active_conn"), &self.cur_connections.to_string(), cx))
            .child(self.stat_card(crate::i18n::t("home.upload_total"), &format_bytes(self.cur_up_total), cx))
            .child(self.stat_card(crate::i18n::t("home.download_total"), &format_bytes(self.cur_down_total), cx))
            .child(self.stat_card(crate::i18n::t("home.memory"), &format_bytes(self.cur_memory), cx));

        let chart = self.traffic_chart(cx);

        section_card(
            cx,
            crate::i18n::t("home.traffic"),
            v_flex().gap_4().child(cards).child(chart).into_any_element(),
        )
    }

    fn stat_card(&self, label: &'static str, value: &str, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .min_w(px(120.))
            .flex_1()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().muted.opacity(0.4))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child(value.to_string()),
            )
    }

    fn traffic_chart(&self, cx: &Context<Self>) -> AnyElement {
        // Need at least two samples so we can draw a non-degenerate line.
        // Padding the front with zeros looks worse than just showing
        // "Collecting…", so we bail out early below the threshold.
        if self.samples.len() < 2 {
            return div()
                .h(px(180.))
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .text_sm()
                .child("Collecting traffic…")
                .into_any_element();
        }

        // AreaChart's linear Y scale degenerates when domain min == max
        // (i.e. every sample is 0), producing NaN ticks and a blank chart.
        // Inject a synthetic 1 KB ceiling so a flat baseline still paints
        // visibly at the bottom.
        let max = self
            .samples
            .iter()
            .map(|s| s.up.max(s.down))
            .max()
            .unwrap_or(0);
        let ceiling: f64 = if max == 0 { 1024.0 } else { 0.0 };

        let data: Vec<TrafficSample> = self.samples.iter().copied().collect();
        let stroke_up = cx.theme().chart_1;
        let fill_up = cx.theme().chart_1.opacity(0.18);
        let stroke_down = cx.theme().chart_2;
        let fill_down = cx.theme().chart_2.opacity(0.18);

        let chart = AreaChart::new(data)
            .x(|s: &TrafficSample| s.seq.to_string())
            .y(|s: &TrafficSample| s.down as f64)
            .y(|s: &TrafficSample| s.up as f64)
            // Synthetic ceiling series so the linear Y scale has a real
            // domain even when every real sample is zero. Painted with
            // transparent stroke/fill so it doesn't show.
            .y(move |_s: &TrafficSample| ceiling)
            .stroke(stroke_down)
            .fill(fill_down)
            .stroke(stroke_up)
            .fill(fill_up)
            .stroke(gpui::transparent_black())
            .fill(gpui::transparent_black())
            .natural()
            .x_axis(false)
            .tick_margin(usize::MAX);

        let legend = h_flex()
            .gap_4()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(legend_swatch("下载", cx.theme().chart_2, cx))
            .child(legend_swatch("上传", cx.theme().chart_1, cx));

        v_flex()
            .gap_2()
            .child(legend)
            .child(div().h(px(180.)).w_full().child(chart))
            .into_any_element()
    }

    fn sites_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let row = h_flex().gap_3().flex_wrap().children(
            self.sites
                .iter()
                .enumerate()
                .map(|(i, site)| self.render_site_card(i, site, cx)),
        );

        section_card(cx, "网站测试", row.into_any_element())
    }

    fn render_site_card(&self, idx: usize, site: &SiteTest, cx: &Context<Self>) -> AnyElement {
        let icon_path = site
            .icon_stem
            .and_then(|stem| service_icon_path(stem));

        let (status_text, status_color) = match &site.state {
            TestState::Idle => (String::new(), cx.theme().muted_foreground),
            TestState::Testing => ("测试中…".to_string(), cx.theme().muted_foreground),
            TestState::Ok(ms) => (format!("{} ms", ms), latency_color(*ms, cx)),
            TestState::Failed => ("失败".to_string(), hsla(0.0, 0.7, 0.5, 1.0)),
        };

        let busy = matches!(site.state, TestState::Testing);

        v_flex()
            .id(SharedString::from(format!("site-{}", idx)))
            .min_w(px(160.))
            .flex_1()
            .gap_2()
            .px_3()
            .py_3()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .when_some(icon_path, |el, p| {
                        el.child(img(p).w(px(20.)).h(px(20.)))
                    })
                    .when(site.icon_stem.is_none() || icon_path_for(site).is_none(), |el| {
                        el.child(gpui_component::Icon::new(IconName::Globe).size(px(20.)))
                    })
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(site.name.to_string()),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_color)
                            .child(status_text),
                    )
                    .child(
                        Button::new(SharedString::from(format!("site-test-{}", idx)))
                            .label(if busy { crate::i18n::t("home.testing") } else { crate::i18n::t("home.test") })
                            .compact()
                            .ghost()
                            .on_click(cx.listener(move |this, _e, _w, cx| {
                                this.run_site_test(idx, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn ip_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let body = match &self.ip_state {
            IpState::Loading => v_flex()
                .items_center()
                .justify_center()
                .py_6()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("查询中…"),
                )
                .into_any_element(),
            IpState::Failed(err) => v_flex()
                .gap_2()
                .py_4()
                .child(
                    div()
                        .text_sm()
                        .text_color(hsla(0.0, 0.7, 0.5, 1.0))
                        .child(crate::i18n::t("home.query_failed")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(err.clone()),
                )
                .into_any_element(),
            IpState::Loaded(info) => self.render_ip_loaded(info, cx).into_any_element(),
        };

        let header = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::i18n::t("home.ip_info")),
            )
            .child(
                Button::new("ip-refresh")
                    .icon(IconName::Redo)
                    .compact()
                    .ghost()
                    .on_click(cx.listener(|this, _e, _w, cx| this.refresh_ip(cx))),
            );

        v_flex()
            .gap_2()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(header)
            .child(div().pt_2().child(body))
    }

    fn render_ip_loaded(&self, info: &IpInfo, cx: &Context<Self>) -> impl IntoElement {
        let country_name = info.country.clone().unwrap_or_default();
        let flag_path = country_flag_path(&country_name);

        let location = match (&info.city, &info.region) {
            (Some(c), Some(r)) if c != r => format!("{}, {}", c, r),
            (Some(c), _) => c.clone(),
            (_, Some(r)) => r.clone(),
            _ => "—".to_string(),
        };

        let displayed_ip = match info.ip.as_deref() {
            Some(ip) if self.show_ip => ip.to_string(),
            Some(_) => "••••••••".to_string(),
            None => "—".to_string(),
        };

        h_flex()
            .gap_6()
            .items_start()
            .child(
                v_flex()
                    .gap_2()
                    .items_center()
                    .min_w(px(160.))
                    .child(match flag_path {
                        Some(p) => img(p).w(px(96.)).h(px(72.)).into_any_element(),
                        None => div()
                            .w(px(96.))
                            .h(px(72.))
                            .rounded_md()
                            .bg(cx.theme().muted.opacity(0.5))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(country_display_name(&country_name)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_2()
                    .child(self.kv_row(
                        "IP",
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_sm().child(displayed_ip))
                            .child(
                                Button::new("ip-toggle-vis")
                                    .icon(if self.show_ip {
                                        IconName::EyeOff
                                    } else {
                                        IconName::Eye
                                    })
                                    .compact()
                                    .ghost()
                                    .on_click(cx.listener(|this, _e, _w, cx| {
                                        this.show_ip = !this.show_ip;
                                        cx.notify();
                                    })),
                            )
                            .into_any_element(),
                        cx,
                    ))
                    .child(self.kv_row(
                        "服务商",
                        div()
                            .text_sm()
                            .child(info.org.clone().unwrap_or_else(|| "—".into()))
                            .into_any_element(),
                        cx,
                    ))
                    .child(self.kv_row(
                        "位置",
                        div().text_sm().child(location).into_any_element(),
                        cx,
                    ))
                    .child(self.kv_row(
                        "时区",
                        div()
                            .text_sm()
                            .child(info.timezone.clone().unwrap_or_else(|| "—".into()))
                            .into_any_element(),
                        cx,
                    )),
            )
    }

    fn kv_row(&self, label: &str, value: AnyElement, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(
                div()
                    .min_w(px(72.))
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_string()),
            )
            .child(div().flex_1().child(value))
    }
}

fn legend_swatch(label: &'static str, color: Hsla, cx: &Context<HomePage>) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .child(div().size(px(8.)).rounded_full().bg(color))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
}

fn section_card(cx: &Context<HomePage>, title: &str, body: AnyElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
        .child(div().pt_2().child(body))
}

fn icon_path_for(site: &SiteTest) -> Option<PathBuf> {
    site.icon_stem.and_then(service_icon_path)
}

fn service_icon_path(stem: &str) -> Option<PathBuf> {
    let p = std::env::current_dir()
        .unwrap_or_default()
        .join("icons")
        .join(format!("{}.svg", stem));
    if p.exists() { Some(p) } else { None }
}

fn country_flag_path(country: &str) -> Option<PathBuf> {
    let stem = match country.to_uppercase().as_str() {
        "HK" => "香港",
        "US" | "USA" => "美国",
        "GB" | "UK" => "英国",
        "JP" => "日本",
        "SG" => "新加坡",
        "DE" => "德国",
        "CA" => "加拿大",
        "VN" => "越南",
        _ => return None,
    };
    let p = std::env::current_dir()
        .unwrap_or_default()
        .join("icons/country")
        .join(format!("{}.svg", stem));
    if p.exists() { Some(p) } else { None }
}

fn country_display_name(code: &str) -> String {
    match code.to_uppercase().as_str() {
        "HK" => "香港".to_string(),
        "US" | "USA" => "美国".to_string(),
        "GB" | "UK" => "英国".to_string(),
        "JP" => "日本".to_string(),
        "SG" => "新加坡".to_string(),
        "DE" => "德国".to_string(),
        "CA" => "加拿大".to_string(),
        "VN" => "越南".to_string(),
        other if !other.is_empty() => other.to_string(),
        _ => "—".to_string(),
    }
}

fn latency_color(ms: u32, cx: &Context<HomePage>) -> Hsla {
    let _ = cx;
    if ms < 100 {
        hsla(0.32, 0.6, 0.45, 1.0)
    } else if ms < 300 {
        hsla(0.13, 0.85, 0.5, 1.0)
    } else {
        hsla(0.07, 0.9, 0.55, 1.0)
    }
}

fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ---- Async helpers ---------------------------------------------------------

#[derive(Debug, Clone)]
struct RawStats {
    upload_total: u64,
    download_total: u64,
    connections_count: u32,
    memory_inuse: u64,
}

async fn fetch_stats() -> Result<RawStats, String> {
    #[derive(Deserialize)]
    struct ConnResp {
        #[serde(rename = "uploadTotal")]
        upload_total: u64,
        #[serde(rename = "downloadTotal")]
        download_total: u64,
        #[serde(default)]
        connections: Option<Vec<serde_json::Value>>,
        // mihomo embeds memory directly in /connections; the separate
        // /memory endpoint reports 0 on this build, so we read it here.
        #[serde(default)]
        memory: u64,
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(900))
        .build()
        .map_err(|e| e.to_string())?;

    let conn = client
        .get(format!("{}/connections", MIHOMO_BASE))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<ConnResp>()
        .await
        .map_err(|e| e.to_string())?;

    Ok(RawStats {
        upload_total: conn.upload_total,
        download_total: conn.download_total,
        connections_count: conn.connections.as_ref().map(|v| v.len() as u32).unwrap_or(0),
        memory_inuse: conn.memory,
    })
}

async fn measure_latency(url: &str) -> anyhow::Result<u32> {
    // HEAD request through mihomo's mixed port. We deliberately use a short
    // timeout — anything >2s is "slow enough that we don't need a precise
    // measurement", we just want a status badge.
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(PROXY_URL)?)
        .timeout(Duration::from_millis(2500))
        .build()?;

    let started = Instant::now();
    let resp = client.head(url).send().await?;
    let elapsed = started.elapsed().as_millis() as u32;

    if !resp.status().is_success() && !resp.status().is_redirection() {
        warn!(url, status = %resp.status(), "site test non-success");
    }
    Ok(elapsed)
}

async fn fetch_ip_info() -> anyhow::Result<IpInfo> {
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(PROXY_URL)?)
        .timeout(Duration::from_secs(8))
        .build()?;

    let resp = client.get("https://ipinfo.io/json").send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let info: IpInfo = resp.json().await?;
    Ok(info)
}

/// Fetch the current proxy mode (`rule|global|direct`), the GLOBAL group's
/// `now` and `all` list, and the TUN enable flag. Returns
/// `(mode, global_now, global_all, tun_enabled)`.
async fn fetch_mode_and_global() -> anyhow::Result<(String, Option<String>, Vec<String>, bool)> {
    #[derive(Deserialize)]
    struct Tun {
        #[serde(default)]
        enable: bool,
    }
    #[derive(Deserialize)]
    struct Cfg {
        mode: String,
        #[serde(default)]
        tun: Option<Tun>,
    }
    #[derive(Deserialize)]
    struct GlobalGroup {
        #[serde(default)]
        now: Option<String>,
        #[serde(default)]
        all: Vec<String>,
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(900))
        .build()?;

    let (cfg_res, global_res) = tokio::join!(
        client.get(format!("{}/configs", MIHOMO_BASE)).send(),
        client.get(format!("{}/proxies/GLOBAL", MIHOMO_BASE)).send(),
    );

    let cfg: Cfg = cfg_res?.json().await?;
    let tun_enabled = cfg.tun.map(|t| t.enable).unwrap_or(false);
    let (now, all) = match global_res {
        Ok(r) if r.status().is_success() => {
            let g: GlobalGroup = r.json().await?;
            (g.now, g.all)
        }
        _ => (None, Vec::new()),
    };

    Ok((cfg.mode, now, all, tun_enabled))
}

async fn patch_mode(mode: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let resp = client
        .patch(format!("{}/configs", MIHOMO_BASE))
        .json(&serde_json::json!({ "mode": mode }))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    Ok(())
}

async fn put_global_node(name: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let resp = client
        .put(format!("{}/proxies/GLOBAL", MIHOMO_BASE))
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    Ok(())
}

async fn patch_tun(enable: bool) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let resp = client
        .patch(format!("{}/configs", MIHOMO_BASE))
        .json(&serde_json::json!({ "tun": { "enable": enable } }))
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {} — {}", status, body);
    }
    Ok(())
}
