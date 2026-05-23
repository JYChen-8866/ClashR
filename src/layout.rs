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
                    .icon(p.icon())
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

        // Two-column layout: sidebar + content
        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                // Left: sidebar
                Sidebar::new("main-sidebar")
                    .collapsible(SidebarCollapsible::Icon)
                    .collapsed(self.sidebar_collapsed)
                    .w(px(200.))
                    .header(div().bg(gpui::yellow()).h(px(36.)))
                    .pt(px(36.))
                    .header(
                        SidebarHeader::new().child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    img("icons/app-icon.png")
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
                    .child(
                        SidebarGroup::new(crate::i18n::t("nav.navigation"))
                            .child(SidebarMenu::new().children(menu_items)),
                    )
                    .footer(
                        SidebarFooter::new().child(
                            v_flex()
                                .w_full()
                                .gap_1()
                                .child({
                                    let (label, color) = self.status_label();
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(div().size(px(7.)).rounded_full().bg(color))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(label),
                                        )
                                })
                                .child({
                                    let (up, down) = self.home_page.read(cx).speeds();
                                    h_flex()
                                        .gap_2()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{} ↑", up))
                                        .child(format!("{} ↓", down))
                                }),
                        ),
                    ),
            )
            .child(
                // Right: main content with collapse button
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .child(
                        // Top bar with collapse button
                        div()
                            .w_full()
                            .h(px(40.))
                            .px_3()
                            .flex()
                            .items_center()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
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
                                    .on_click(
                                        cx.listener(|this, _e, _w, cx| this.toggle_sidebar(cx)),
                                    ),
                            ),
                    )
                    .child(
                        // Main content
                        div()
                            .flex_1()
                            .min_h_0()
                            .p_4()
                            .overflow_hidden()
                            .child(match current {
                                Page::Profiles => {
                                    div().size_full().child(self.profiles_page.clone())
                                }
                                Page::Proxies => div().size_full().child(self.proxies_page.clone()),
                                Page::Settings => {
                                    div().size_full().child(self.settings_page.clone())
                                }
                                Page::Home => div().size_full().child(self.home_page.clone()),
                                Page::Connections => {
                                    div().size_full().child(self.connections_page.clone())
                                }
                                Page::Rules => div().size_full().child(self.rules_page.clone()),
                                Page::Logs => div().size_full().child(self.logs_page.clone()),
                            }),
                    ),
            )
    }
}
