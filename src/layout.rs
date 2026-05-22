use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, IconName, StyledExt as _, TitleBar, h_flex, v_flex, sidebar::*,
};

use crate::core::{CoreManager, CoreStatus};
use crate::pages::{ConnectionsPage, HomePage, LogsPage, ProfilesPage, ProxiesPage, RulesPage, SettingsPage};
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
        cx.spawn(async move |entity, cx| {
            loop {
                let status = spawn_on_tokio(async {
                    CoreManager::global().status().await
                }).await;

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

                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
            }
        }).detach();

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
        }
    }

    fn status_label(&self) -> (&'static str, Hsla) {
        match &self.core_status {
            CoreStatus::Stopped => (crate::i18n::t("status.stopped"), hsla(0.0, 0.0, 0.6, 1.0)),
            CoreStatus::Starting => (crate::i18n::t("status.starting"), hsla(0.12, 0.7, 0.5, 1.0)),
            CoreStatus::Running { .. } => (crate::i18n::t("status.running"), hsla(0.32, 0.6, 0.45, 1.0)),
            CoreStatus::Stopping => (crate::i18n::t("status.stopping"), hsla(0.12, 0.7, 0.5, 1.0)),
            CoreStatus::Failed { .. } => (crate::i18n::t("status.failed"), hsla(0.0, 0.7, 0.5, 1.0)),
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

        // In fullscreen, macOS hides the traffic lights, leaving the
        // TitleBar's 80px left padding as wasted space. Pull our content
        // back into that area with a negative left margin.
        let fullscreen = window.is_fullscreen();

        let title_bar = TitleBar::new().child(
            h_flex()
                .w_full()
                .px_2()
                .gap_2()
                .items_center()
                .when(fullscreen, |this| this.ml(-px(80.)))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child("ClashR"),
                )
                .child({
                    let (label, color) = self.status_label();
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(div().size(px(6.)).rounded_full().bg(color))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(label),
                        )
                }),
        );

        let body = h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("main-sidebar")
                    .collapsible(SidebarCollapsible::Icon)
                    .collapsed(false)
                    .w(px(200.))
                    .header(
                        SidebarHeader::new().child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .size_8()
                                        .flex_shrink_0()
                                        .rounded(cx.theme().radius)
                                        .bg(cx.theme().sidebar_primary)
                                        .text_color(cx.theme().sidebar_primary_foreground)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(gpui_component::Icon::new(IconName::Globe)),
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
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
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
                                Page::Connections => div().size_full().child(self.connections_page.clone()),
                                Page::Rules => div().size_full().child(self.rules_page.clone()),
                                Page::Logs => div().size_full().child(self.logs_page.clone()),
                            }),
                    ),
            );

        v_flex()
            .size_full()
            .child(title_bar)
            .child(body)
    }
}

