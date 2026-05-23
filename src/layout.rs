use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, sidebar::*, v_flex, ActiveTheme, IconName, StyledExt as _, TitleBar};

use crate::core::{CoreManager, CoreStatus};
use crate::pages::{
    ConnectionsPage, HomePage, LogsPage, ProfilesPage, ProxiesPage, RulesPage, SettingsPage,
};
use crate::runtime::spawn_on_tokio;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Proxies,
    Profiles,
    Connections,
    Rules,
    Logs,
    Settings,
}

impl Page {
    pub fn label(&self) -> &'static str {
        match self {
            Page::Home => crate::i18n::t("nav.home"),
            Page::Proxies => crate::i18n::t("nav.proxies"),
            Page::Profiles => crate::i18n::t("nav.profiles"),
            Page::Connections => crate::i18n::t("nav.connections"),
            Page::Rules => crate::i18n::t("nav.rules"),
            Page::Logs => crate::i18n::t("nav.logs"),
            Page::Settings => crate::i18n::t("nav.settings"),
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Page::Home => IconName::LayoutDashboard,
            Page::Proxies => IconName::Globe,
            Page::Profiles => IconName::File,
            Page::Connections => IconName::Network,
            Page::Rules => IconName::Map,
            Page::Logs => IconName::SquareTerminal,
            Page::Settings => IconName::Settings,
        }
    }

    pub fn all() -> &'static [Page] {
        &[
            Page::Home,
            Page::Proxies,
            Page::Profiles,
            Page::Connections,
            Page::Rules,
            Page::Logs,
            Page::Settings,
        ]
    }
}

pub struct AppLayout {
    current_page: Page,
    home_page: Entity<HomePage>,
    profiles_page: Entity<ProfilesPage>,
    proxies_page: Entity<ProxiesPage>,
    connections_page: Entity<ConnectionsPage>,
    rules_page: Entity<RulesPage>,
    logs_page: Entity<LogsPage>,
    settings_page: Entity<SettingsPage>,
    core_status: CoreStatus,
    sidebar_collapsed: bool,
}

impl AppLayout {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let home_page = cx.new(|cx| HomePage::new(window, cx));
        let profiles_page = cx.new(|cx| ProfilesPage::new(window, cx));
        let proxies_page = cx.new(|cx| ProxiesPage::new(window, cx));
        let connections_page = cx.new(|cx| ConnectionsPage::new(window, cx));
        let rules_page = cx.new(|cx| RulesPage::new(window, cx));
        let logs_page = cx.new(|cx| LogsPage::new(window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(window, cx));

        // Poll core status periodically so the indicator stays in sync.
        cx.spawn(async move |entity, cx| loop {
            let status = spawn_on_tokio(async { CoreManager::global().status().await }).await;

            let updated = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this: &mut AppLayout, cx| {
                        if this.core_status != status {
                            this.core_status = status;
                            cx.notify();
                        }
                    });
                    true
                } else {
                    false
                }
            });

            if !updated {
                break;
            }

            cx.background_executor()
                .timer(std::time::Duration::from_millis(800))
                .await;
        })
        .detach();

        Self {
            current_page: Page::Home,
            home_page,
            profiles_page,
            proxies_page,
            connections_page,
            rules_page,
            logs_page,
            settings_page,
            core_status: CoreStatus::Stopped,
            sidebar_collapsed: false,
        }
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    fn status_label(&self) -> (&'static str, Hsla) {
        match &self.core_status {
            CoreStatus::Stopped => (crate::i18n::t("status.stopped"), hsla(0.0, 0.0, 0.6, 1.0)),
            CoreStatus::Starting => (crate::i18n::t("status.starting"), hsla(0.12, 0.7, 0.5, 1.0)),
            CoreStatus::Running { .. } => {
                (crate::i18n::t("status.running"), hsla(0.32, 0.6, 0.45, 1.0))
            }
            CoreStatus::Stopping => (crate::i18n::t("status.stopping"), hsla(0.12, 0.7, 0.5, 1.0)),
            CoreStatus::Failed { .. } => {
                (crate::i18n::t("status.failed"), hsla(0.0, 0.7, 0.5, 1.0))
            }
        }
    }
}

impl Render for AppLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current_page;

        let menu_items: Vec<SidebarMenuItem> = Page::all()
            .iter()
            .map(|page| {
                let p = *page;
                SidebarMenuItem::new(p.label())
                    // Nudge nav icons up to 20px so they sit closer in
                    // size to the 32px collapsed header icon — keeps the
                    // collapsed sidebar visually balanced.
                    .icon(gpui_component::Icon::new(p.icon()).size(px(20.)))
                    .active(current == p)
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        let was = this.current_page;
                        this.current_page = p;
                        // Refresh dynamic pages when entering them so stale
                        // state (proxy list after profile switch, etc.) is
                        // reconciled with mihomo's runtime view.
                        if p == Page::Proxies && was != Page::Proxies {
                            this.proxies_page.update(cx, |page, cx| {
                                page.refresh(cx);
                            });
                        }
                        cx.notify();
                    }))
            })
            .collect();

        let toggle_button = div()
            .id("sidebar-toggle")
            .cursor_pointer()
            .p_1()
            .rounded_md()
            .hover(|this| this.bg(cx.theme().muted))
            .child(
                gpui_component::Icon::new(if self.sidebar_collapsed {
                    IconName::PanelLeft
                } else {
                    IconName::PanelLeftClose
                })
                .size_4()
                .text_color(cx.theme().muted_foreground),
            )
            .on_click(cx.listener(|this, _e, _w, cx| this.toggle_sidebar(cx)));

        let (status_label, status_color) = self.status_label();
        let (up, down) = self.home_page.read(cx).speeds();

        // Sidebar's lib-internal width is 48 (collapsed) / 200 (expanded).
        // We wrap it in a host that is at least as wide as the macOS
        // traffic-light cluster (~80px) so the wrapper's right edge — and
        // the matching TitleBar seam — never bisects the lights. The
        // sidebar sits centred inside the wrapper.
        let sidebar_inner_w = if self.sidebar_collapsed {
            px(48.)
        } else {
            px(200.)
        };

        // The lib reserves a fixed left padding for the macOS traffic
        // lights. We don't fight it; we let it consume the first chunk of
        // the bar (the lights live there), and our visible left segment
        // starts after it. That way the segment's right edge — and our
        // colour seam — lines up exactly with the sidebar's edge below.
        #[cfg(target_os = "macos")]
        let lib_left_pad = px(80.);
        #[cfg(not(target_os = "macos"))]
        let lib_left_pad = px(12.);

        // In fullscreen on macOS the lib adds an extra `pl_3()` (12px) to
        // the inner bar where our children live, on top of the 80px outer
        // pad. Subtract that here so the seam doesn't drift right by 12px
        // when entering fullscreen.
        #[cfg(target_os = "macos")]
        let lib_extra_pad = if window.is_fullscreen() {
            px(12.)
        } else {
            px(0.)
        };
        #[cfg(not(target_os = "macos"))]
        let lib_extra_pad = px(0.);

        // Minimum sidebar wrapper width — when the sidebar is narrower
        // than this we pad it out so the right edge clears the traffic
        // lights with a small gap. In fullscreen on macOS the traffic
        // lights vanish, but the lib's effective left pad grows by 12px
        // (extra `pl_3()` on the inner bar), so the wrapper must be at
        // least that wide for the seam to stay aligned.
        #[cfg(target_os = "macos")]
        let min_sidebar_w = if window.is_fullscreen() {
            lib_left_pad + lib_extra_pad + px(10.)
        } else {
            px(90.)
        };
        #[cfg(not(target_os = "macos"))]
        let min_sidebar_w = lib_left_pad;

        // Sidebar wrapper width: the larger of the intrinsic width and
        // the minimum, so the seam never bisects the lights.
        let sidebar_w = sidebar_inner_w.max(min_sidebar_w);
        // Visible width of the left segment inside the title bar (after
        // the lib's outer padding has been subtracted).
        let left_seg_visible_w = sidebar_w - lib_left_pad - lib_extra_pad;

        // Title bar split visually into two segments by background colour:
        //   left  — sidebar bg (matches the sidebar below)
        //   right — page bg (matches the content area)
        // The colour seam itself acts as the divider, which lines up with
        // the sidebar's right edge perfectly without us drawing an extra
        // border (an extra line read as too dark in dark themes).
        //
        // Outer bg is platform-conditional: on macOS the lib reserves
        // ~80px on the left for traffic lights and we want that area to
        // paint in sidebar bg. On Windows/Linux the lib appends its own
        // window-control buttons (min/max/close) on the right, and we
        // want that area to read as content bg, so we flip the outer.
        #[cfg(target_os = "macos")]
        let outer_bg = cx.theme().sidebar;
        #[cfg(not(target_os = "macos"))]
        let outer_bg = cx.theme().background;

        let title_bar = TitleBar::new()
            // Trim the bar height a little — the lib default (34px) feels
            // bulky above a sidebar with no visible header chrome.
            .h(px(28.))
            .bg(outer_bg)
            // TitleBar paints a 1px bottom border by default. Override the
            // width to 0 so the seam between bar and body sits flush.
            .border_b_0()
            .child(
                h_flex()
                    .h_full()
                    .w_full()
                    .items_center()
                    .child(
                        // Left segment — sidebar-coloured. Explicit bg so
                        // it overrides the outer when the outer is the
                        // content colour (Windows/Linux).
                        h_flex()
                            .h_full()
                            .w(left_seg_visible_w)
                            .flex_shrink_0()
                            .items_center()
                            .pr_2()
                            .bg(cx.theme().sidebar)
                            // Continue the sidebar's right border up through
                            // the title bar so the seam reads as one
                            // continuous vertical line.
                            .border_r_1()
                            .border_color(cx.theme().sidebar_border),
                    )
                    .child(
                        // Right segment — content-coloured
                        h_flex()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .px_3()
                            .gap_3()
                            .bg(cx.theme().background)
                            .child(toggle_button)
                            .child(div().flex_1())
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(div().size(px(7.)).rounded_full().bg(status_color))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(status_label),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("↑ {}", up))
                                    .child(format!("↓ {}", down)),
                            ),
                    ),
            );

        // Body: sidebar + content. Sits below the TitleBar.
        // The sidebar is wrapped so we can pad its overall width up to the
        // traffic-light cluster on macOS without having to fork the lib's
        // hardcoded 48px collapsed width. Sidebar's own right border is
        // hidden (border_color transparent) and we redraw it on the
        // wrapper at our chosen width.
        let body = h_flex()
            .flex_1()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .h_full()
                    .w(sidebar_w)
                    .flex_shrink_0()
                    .justify_center()
                    .bg(cx.theme().sidebar)
                    .border_r_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        Sidebar::new("main-sidebar")
                            .collapsible(SidebarCollapsible::Icon)
                            .collapsed(self.sidebar_collapsed)
                            .w(px(200.))
                            // Drop the lib's right border entirely; the
                            // wrapper draws the only vertical seam.
                            .border_r_0()
                            .header(
                                SidebarHeader::new().p_0().child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            img("icons/app-icon.svg")
                                                .size_8()
                                                .flex_shrink_0()
                                                .rounded(cx.theme().radius),
                                        )
                                        .child(
                                            v_flex()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(cx.theme().sidebar_foreground)
                                                        .child("ClashR"),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child("v0.1.0"),
                                                ),
                                        ),
                                ),
                            )
                            .child(SidebarMenu::new().children(menu_items)),
                    ),
            )
            .child(
                v_flex().flex_1().h_full().min_w_0().child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .p_4()
                        .overflow_hidden()
                        .child(match current {
                            Page::Profiles => div().size_full().child(self.profiles_page.clone()),
                            Page::Proxies => div().size_full().child(self.proxies_page.clone()),
                            Page::Settings => div().size_full().child(self.settings_page.clone()),
                            Page::Home => div().size_full().child(self.home_page.clone()),
                            Page::Connections => {
                                div().size_full().child(self.connections_page.clone())
                            }
                            Page::Rules => div().size_full().child(self.rules_page.clone()),
                            Page::Logs => div().size_full().child(self.logs_page.clone()),
                        }),
                ),
            );

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(title_bar)
            .child(body)
    }
}
