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
    ActiveTheme, IconName, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
    chart::AreaChart,
    popover::Popover,
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
    // ipinfo.io → "country" (ISO-2); ipapi.co → "country" (ISO-2) +
    // "country_name" (full); ip.sb → "country" (full name) +
    // "country_code" (ISO-2). We capture both so we can resolve the flag
    // by ISO code regardless of which endpoint answered.
    #[serde(default, alias = "country_name")]
    country: Option<String>,
    #[serde(default, alias = "countryCode", alias = "country_code_iso2")]
    country_code: Option<String>,
    // ipinfo.io → "org"; ip.sb → "asn_organization" or "isp"; ipapi.co → "org"
    #[serde(default, alias = "asn_organization", alias = "isp", alias = "organization")]
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
        // Prefer the explicit ISO-2 field; fall back to `country` if it
        // already looks like an ISO-2 code (ipinfo.io / ipapi.co give us
        // the code there; ip.sb gives us the full name and we read the
        // code from `country_code`).
        let iso2 = info
            .country_code
            .as_deref()
            .or_else(|| info.country.as_deref().filter(|s| s.trim().len() == 2))
            .map(|s| s.trim().to_uppercase())
            .unwrap_or_default();

        let flag_path = country_flag_path(&iso2);
        let country_label = if !iso2.is_empty() {
            country_display_name(&iso2)
        } else {
            info.country
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "—".to_string())
        };

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
                            .child(country_label),
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

/// Resolve `icons/country/<iso2>.svg`. Input must already be an ISO-2
/// code (uppercase or lowercase); the function lowercases it to match
/// the on-disk file naming. Returns `None` if the file isn't shipped.
fn country_flag_path(iso2: &str) -> Option<PathBuf> {
    let code = iso2.trim();
    if code.len() != 2 {
        return None;
    }
    let p = std::env::current_dir()
        .unwrap_or_default()
        .join("icons/country")
        .join(format!("{}.svg", code.to_lowercase()));
    if p.exists() { Some(p) } else { None }
}

/// Localised display name for an ISO-2 country code. Falls back to the
/// uppercase code itself if we don't have a translation — better than
/// hiding the country entirely.
fn country_display_name(iso2: &str) -> String {
    let code = iso2.trim().to_uppercase();
    if code.is_empty() {
        return "—".to_string();
    }
    let zh = crate::i18n::current_locale() == "zh";
    let name = lookup_country_name(&code, zh);
    name.map(|s| s.to_string()).unwrap_or(code)
}

fn lookup_country_name(code: &str, zh: bool) -> Option<&'static str> {
    // ISO 3166-1 alpha-2 → (English, Chinese). Keep this list in sync
    // with the flag SVGs in `icons/country/`.
    const TABLE: &[(&str, &str, &str)] = &[
        ("AD", "Andorra", "安道尔"),
        ("AE", "United Arab Emirates", "阿联酋"),
        ("AF", "Afghanistan", "阿富汗"),
        ("AG", "Antigua and Barbuda", "安提瓜和巴布达"),
        ("AI", "Anguilla", "安圭拉"),
        ("AL", "Albania", "阿尔巴尼亚"),
        ("AM", "Armenia", "亚美尼亚"),
        ("AO", "Angola", "安哥拉"),
        ("AQ", "Antarctica", "南极洲"),
        ("AR", "Argentina", "阿根廷"),
        ("AS", "American Samoa", "美属萨摩亚"),
        ("AT", "Austria", "奥地利"),
        ("AU", "Australia", "澳大利亚"),
        ("AW", "Aruba", "阿鲁巴"),
        ("AX", "Åland Islands", "奥兰群岛"),
        ("AZ", "Azerbaijan", "阿塞拜疆"),
        ("BA", "Bosnia and Herzegovina", "波黑"),
        ("BB", "Barbados", "巴巴多斯"),
        ("BD", "Bangladesh", "孟加拉国"),
        ("BE", "Belgium", "比利时"),
        ("BF", "Burkina Faso", "布基纳法索"),
        ("BG", "Bulgaria", "保加利亚"),
        ("BH", "Bahrain", "巴林"),
        ("BI", "Burundi", "布隆迪"),
        ("BJ", "Benin", "贝宁"),
        ("BL", "Saint Barthélemy", "圣巴泰勒米"),
        ("BM", "Bermuda", "百慕大"),
        ("BN", "Brunei", "文莱"),
        ("BO", "Bolivia", "玻利维亚"),
        ("BQ", "Caribbean Netherlands", "荷兰加勒比区"),
        ("BR", "Brazil", "巴西"),
        ("BS", "Bahamas", "巴哈马"),
        ("BT", "Bhutan", "不丹"),
        ("BV", "Bouvet Island", "布韦岛"),
        ("BW", "Botswana", "博茨瓦纳"),
        ("BY", "Belarus", "白俄罗斯"),
        ("BZ", "Belize", "伯利兹"),
        ("CA", "Canada", "加拿大"),
        ("CC", "Cocos Islands", "科科斯群岛"),
        ("CD", "DR Congo", "刚果（金）"),
        ("CF", "Central African Republic", "中非共和国"),
        ("CG", "Republic of the Congo", "刚果（布）"),
        ("CH", "Switzerland", "瑞士"),
        ("CI", "Côte d'Ivoire", "科特迪瓦"),
        ("CK", "Cook Islands", "库克群岛"),
        ("CL", "Chile", "智利"),
        ("CM", "Cameroon", "喀麦隆"),
        ("CN", "China", "中国"),
        ("CO", "Colombia", "哥伦比亚"),
        ("CR", "Costa Rica", "哥斯达黎加"),
        ("CU", "Cuba", "古巴"),
        ("CV", "Cape Verde", "佛得角"),
        ("CW", "Curaçao", "库拉索"),
        ("CX", "Christmas Island", "圣诞岛"),
        ("CY", "Cyprus", "塞浦路斯"),
        ("CZ", "Czechia", "捷克"),
        ("DE", "Germany", "德国"),
        ("DJ", "Djibouti", "吉布提"),
        ("DK", "Denmark", "丹麦"),
        ("DM", "Dominica", "多米尼克"),
        ("DO", "Dominican Republic", "多米尼加"),
        ("DZ", "Algeria", "阿尔及利亚"),
        ("EC", "Ecuador", "厄瓜多尔"),
        ("EE", "Estonia", "爱沙尼亚"),
        ("EG", "Egypt", "埃及"),
        ("EH", "Western Sahara", "西撒哈拉"),
        ("ER", "Eritrea", "厄立特里亚"),
        ("ES", "Spain", "西班牙"),
        ("ET", "Ethiopia", "埃塞俄比亚"),
        ("FI", "Finland", "芬兰"),
        ("FJ", "Fiji", "斐济"),
        ("FK", "Falkland Islands", "福克兰群岛"),
        ("FM", "Micronesia", "密克罗尼西亚"),
        ("FO", "Faroe Islands", "法罗群岛"),
        ("FR", "France", "法国"),
        ("GA", "Gabon", "加蓬"),
        ("GB", "United Kingdom", "英国"),
        ("GD", "Grenada", "格林纳达"),
        ("GE", "Georgia", "格鲁吉亚"),
        ("GF", "French Guiana", "法属圭亚那"),
        ("GG", "Guernsey", "根西"),
        ("GH", "Ghana", "加纳"),
        ("GI", "Gibraltar", "直布罗陀"),
        ("GL", "Greenland", "格陵兰"),
        ("GM", "Gambia", "冈比亚"),
        ("GN", "Guinea", "几内亚"),
        ("GP", "Guadeloupe", "瓜德罗普"),
        ("GQ", "Equatorial Guinea", "赤道几内亚"),
        ("GR", "Greece", "希腊"),
        ("GS", "South Georgia", "南乔治亚"),
        ("GT", "Guatemala", "危地马拉"),
        ("GU", "Guam", "关岛"),
        ("GW", "Guinea-Bissau", "几内亚比绍"),
        ("GY", "Guyana", "圭亚那"),
        ("HK", "Hong Kong", "香港"),
        ("HM", "Heard Island", "赫德岛"),
        ("HN", "Honduras", "洪都拉斯"),
        ("HR", "Croatia", "克罗地亚"),
        ("HT", "Haiti", "海地"),
        ("HU", "Hungary", "匈牙利"),
        ("ID", "Indonesia", "印度尼西亚"),
        ("IE", "Ireland", "爱尔兰"),
        ("IL", "Israel", "以色列"),
        ("IM", "Isle of Man", "马恩岛"),
        ("IN", "India", "印度"),
        ("IO", "British Indian Ocean Territory", "英属印度洋领地"),
        ("IQ", "Iraq", "伊拉克"),
        ("IR", "Iran", "伊朗"),
        ("IS", "Iceland", "冰岛"),
        ("IT", "Italy", "意大利"),
        ("JE", "Jersey", "泽西"),
        ("JM", "Jamaica", "牙买加"),
        ("JO", "Jordan", "约旦"),
        ("JP", "Japan", "日本"),
        ("KE", "Kenya", "肯尼亚"),
        ("KG", "Kyrgyzstan", "吉尔吉斯斯坦"),
        ("KH", "Cambodia", "柬埔寨"),
        ("KI", "Kiribati", "基里巴斯"),
        ("KM", "Comoros", "科摩罗"),
        ("KN", "Saint Kitts and Nevis", "圣基茨和尼维斯"),
        ("KP", "North Korea", "朝鲜"),
        ("KR", "South Korea", "韩国"),
        ("KW", "Kuwait", "科威特"),
        ("KY", "Cayman Islands", "开曼群岛"),
        ("KZ", "Kazakhstan", "哈萨克斯坦"),
        ("LA", "Laos", "老挝"),
        ("LB", "Lebanon", "黎巴嫩"),
        ("LC", "Saint Lucia", "圣卢西亚"),
        ("LI", "Liechtenstein", "列支敦士登"),
        ("LK", "Sri Lanka", "斯里兰卡"),
        ("LR", "Liberia", "利比里亚"),
        ("LS", "Lesotho", "莱索托"),
        ("LT", "Lithuania", "立陶宛"),
        ("LU", "Luxembourg", "卢森堡"),
        ("LV", "Latvia", "拉脱维亚"),
        ("LY", "Libya", "利比亚"),
        ("MA", "Morocco", "摩洛哥"),
        ("MC", "Monaco", "摩纳哥"),
        ("MD", "Moldova", "摩尔多瓦"),
        ("ME", "Montenegro", "黑山"),
        ("MF", "Saint Martin", "法属圣马丁"),
        ("MG", "Madagascar", "马达加斯加"),
        ("MH", "Marshall Islands", "马绍尔群岛"),
        ("MK", "North Macedonia", "北马其顿"),
        ("ML", "Mali", "马里"),
        ("MM", "Myanmar", "缅甸"),
        ("MN", "Mongolia", "蒙古"),
        ("MO", "Macao", "澳门"),
        ("MP", "Northern Mariana Islands", "北马里亚纳群岛"),
        ("MQ", "Martinique", "马提尼克"),
        ("MR", "Mauritania", "毛里塔尼亚"),
        ("MS", "Montserrat", "蒙特塞拉特"),
        ("MT", "Malta", "马耳他"),
        ("MU", "Mauritius", "毛里求斯"),
        ("MV", "Maldives", "马尔代夫"),
        ("MW", "Malawi", "马拉维"),
        ("MX", "Mexico", "墨西哥"),
        ("MY", "Malaysia", "马来西亚"),
        ("MZ", "Mozambique", "莫桑比克"),
        ("NA", "Namibia", "纳米比亚"),
        ("NC", "New Caledonia", "新喀里多尼亚"),
        ("NE", "Niger", "尼日尔"),
        ("NF", "Norfolk Island", "诺福克岛"),
        ("NG", "Nigeria", "尼日利亚"),
        ("NI", "Nicaragua", "尼加拉瓜"),
        ("NL", "Netherlands", "荷兰"),
        ("NO", "Norway", "挪威"),
        ("NP", "Nepal", "尼泊尔"),
        ("NR", "Nauru", "瑙鲁"),
        ("NU", "Niue", "纽埃"),
        ("NZ", "New Zealand", "新西兰"),
        ("OM", "Oman", "阿曼"),
        ("PA", "Panama", "巴拿马"),
        ("PE", "Peru", "秘鲁"),
        ("PF", "French Polynesia", "法属波利尼西亚"),
        ("PG", "Papua New Guinea", "巴布亚新几内亚"),
        ("PH", "Philippines", "菲律宾"),
        ("PK", "Pakistan", "巴基斯坦"),
        ("PL", "Poland", "波兰"),
        ("PM", "Saint Pierre and Miquelon", "圣皮埃尔和密克隆"),
        ("PN", "Pitcairn Islands", "皮特凯恩群岛"),
        ("PR", "Puerto Rico", "波多黎各"),
        ("PS", "Palestine", "巴勒斯坦"),
        ("PT", "Portugal", "葡萄牙"),
        ("PW", "Palau", "帕劳"),
        ("PY", "Paraguay", "巴拉圭"),
        ("QA", "Qatar", "卡塔尔"),
        ("RE", "Réunion", "留尼汪"),
        ("RO", "Romania", "罗马尼亚"),
        ("RS", "Serbia", "塞尔维亚"),
        ("RU", "Russia", "俄罗斯"),
        ("RW", "Rwanda", "卢旺达"),
        ("SA", "Saudi Arabia", "沙特阿拉伯"),
        ("SB", "Solomon Islands", "所罗门群岛"),
        ("SC", "Seychelles", "塞舌尔"),
        ("SD", "Sudan", "苏丹"),
        ("SE", "Sweden", "瑞典"),
        ("SG", "Singapore", "新加坡"),
        ("SH", "Saint Helena", "圣赫勒拿"),
        ("SI", "Slovenia", "斯洛文尼亚"),
        ("SJ", "Svalbard and Jan Mayen", "斯瓦尔巴和扬马延"),
        ("SK", "Slovakia", "斯洛伐克"),
        ("SL", "Sierra Leone", "塞拉利昂"),
        ("SM", "San Marino", "圣马力诺"),
        ("SN", "Senegal", "塞内加尔"),
        ("SO", "Somalia", "索马里"),
        ("SR", "Suriname", "苏里南"),
        ("SS", "South Sudan", "南苏丹"),
        ("ST", "São Tomé and Príncipe", "圣多美和普林西比"),
        ("SV", "El Salvador", "萨尔瓦多"),
        ("SX", "Sint Maarten", "荷属圣马丁"),
        ("SY", "Syria", "叙利亚"),
        ("SZ", "Eswatini", "斯威士兰"),
        ("TC", "Turks and Caicos Islands", "特克斯和凯科斯群岛"),
        ("TD", "Chad", "乍得"),
        ("TF", "French Southern Territories", "法属南方领地"),
        ("TG", "Togo", "多哥"),
        ("TH", "Thailand", "泰国"),
        ("TJ", "Tajikistan", "塔吉克斯坦"),
        ("TK", "Tokelau", "托克劳"),
        ("TL", "Timor-Leste", "东帝汶"),
        ("TM", "Turkmenistan", "土库曼斯坦"),
        ("TN", "Tunisia", "突尼斯"),
        ("TO", "Tonga", "汤加"),
        ("TR", "Türkiye", "土耳其"),
        ("TT", "Trinidad and Tobago", "特立尼达和多巴哥"),
        ("TV", "Tuvalu", "图瓦卢"),
        ("TW", "Taiwan", "台湾"),
        ("TZ", "Tanzania", "坦桑尼亚"),
        ("UA", "Ukraine", "乌克兰"),
        ("UG", "Uganda", "乌干达"),
        ("UM", "U.S. Outlying Islands", "美国本土外小岛屿"),
        ("US", "United States", "美国"),
        ("UY", "Uruguay", "乌拉圭"),
        ("UZ", "Uzbekistan", "乌兹别克斯坦"),
        ("VA", "Vatican City", "梵蒂冈"),
        ("VC", "Saint Vincent and the Grenadines", "圣文森特和格林纳丁斯"),
        ("VE", "Venezuela", "委内瑞拉"),
        ("VG", "British Virgin Islands", "英属维尔京群岛"),
        ("VI", "U.S. Virgin Islands", "美属维尔京群岛"),
        ("VN", "Vietnam", "越南"),
        ("VU", "Vanuatu", "瓦努阿图"),
        ("WF", "Wallis and Futuna", "瓦利斯和富图纳"),
        ("WS", "Samoa", "萨摩亚"),
        ("XK", "Kosovo", "科索沃"),
        ("YE", "Yemen", "也门"),
        ("YT", "Mayotte", "马约特"),
        ("ZA", "South Africa", "南非"),
        ("ZM", "Zambia", "赞比亚"),
        ("ZW", "Zimbabwe", "津巴布韦"),
    ];
    TABLE
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, en, zh_name)| if zh { *zh_name } else { *en })
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

    // Try multiple providers in order. ipinfo.io has a strict free tier
    // (50k/month per IP, easy to trip when sharing a proxy egress IP and
    // returning 429 too many requests), so we lead with looser providers
    // and fall back through the list. First successful JSON parse wins.
    const ENDPOINTS: &[&str] = &[
        "https://api.ip.sb/geoip",
        "https://ipapi.co/json/",
        "https://ipinfo.io/json",
    ];

    let mut last_err: Option<anyhow::Error> = None;
    for url in ENDPOINTS {
        match client.get(*url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    last_err = Some(anyhow::anyhow!("{} → HTTP {}", url, status));
                    continue;
                }
                match resp.json::<IpInfo>().await {
                    Ok(info) => return Ok(info),
                    Err(e) => last_err = Some(anyhow::anyhow!("{} → parse: {}", url, e)),
                }
            }
            Err(e) => last_err = Some(anyhow::anyhow!("{} → {}", url, e)),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no IP info endpoint succeeded")))
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
    // First, update the runtime.yaml file to persist the TUN state
    let runtime_path = crate::core::paths::runtime_yaml_path();
    if let Ok(content) = std::fs::read_to_string(&runtime_path) {
        if let Ok(mut doc) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            if let serde_yaml::Value::Mapping(ref mut map) = doc {
                let tun_key = serde_yaml::Value::String("tun".into());
                if let Some(serde_yaml::Value::Mapping(ref mut tun_map)) = map.get_mut(&tun_key) {
                    let enable_key = serde_yaml::Value::String("enable".into());
                    tun_map.insert(enable_key, serde_yaml::Value::Bool(enable));

                    // Write back to file
                    if let Ok(output) = serde_yaml::to_string(&doc) {
                        let _ = std::fs::write(&runtime_path, output);
                        tracing::info!(enable, "TUN state persisted to config file");
                    }
                }
            }
        }
    }

    // Then, apply the change via mihomo API
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
        tracing::error!(status = %status, body = %body, "TUN toggle API failed");
        anyhow::bail!("HTTP {} — {}", status, body);
    }
    tracing::info!(enable, "TUN toggle API succeeded");
    Ok(())
}
