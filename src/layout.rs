use gpui::*;
use gpui_component::{ActiveTheme, IconName, StyledExt as _, h_flex, v_flex, sidebar::*};

use crate::pages::ProfilesPage;

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
            Page::Home => "Home",
            Page::Proxies => "Proxies",
            Page::Profiles => "Profiles",
            Page::Connections => "Connections",
            Page::Rules => "Rules",
            Page::Logs => "Logs",
            Page::Settings => "Settings",
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
    profiles_page: Entity<ProfilesPage>,
}

impl AppLayout {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let profiles_page = cx.new(|cx| ProfilesPage::new(window, cx));
        Self {
            current_page: Page::Home,
            profiles_page,
        }
    }
}

impl Render for AppLayout {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current_page;

        let menu_items: Vec<SidebarMenuItem> = Page::all()
            .iter()
            .map(|page| {
                let p = *page;
                SidebarMenuItem::new(p.label())
                    .icon(p.icon())
                    .active(current == p)
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.current_page = p;
                        cx.notify();
                    }))
            })
            .collect();

        h_flex()
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
                                        .child(div().font_bold().child("ClashR"))
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
                        SidebarGroup::new("Navigation")
                            .child(SidebarMenu::new().children(menu_items)),
                    )
                    .footer(
                        SidebarFooter::new().child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("0 B/s ↑")
                                .child("0 B/s ↓"),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .p_4()
                    .overflow_hidden()
                    .child(match current {
                        Page::Profiles => div().size_full().child(self.profiles_page.clone()),
                        Page::Home => div().child("Home - Traffic stats, proxy mode, system proxy controls"),
                        Page::Proxies => div().child("Proxies - Proxy groups and node selection"),
                        Page::Connections => div().child("Connections - Active connection list"),
                        Page::Rules => div().child("Rules - Routing rules"),
                        Page::Logs => div().child("Logs - Real-time log stream"),
                        Page::Settings => div().child("Settings - App configuration"),
                    }),
            )
    }
}
