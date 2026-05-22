use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, IconName, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
    popover::Popover,
    switch::Switch,
};
use tracing::{info, warn};

use crate::core::{CoreManager, CoreStatus, sysproxy};
use crate::runtime::spawn_on_tokio;
use crate::theming::{self, Preferences};

const MIHOMO_HOST: &str = "127.0.0.1";
const MIHOMO_MIXED_PORT: u16 = 7890;

pub struct SettingsPage {
    theme_names: Vec<String>,
    system_proxy_on: bool,
}

impl SettingsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Preferences::load();
        // Trust the stored value. We could verify against the OS state, but
        // that costs a `networksetup` shell-out per launch — we instead
        // re-apply on startup if the user had it enabled (see `main.rs`).
        Self {
            theme_names: theming::list_theme_names(cx),
            system_proxy_on: prefs.system_proxy_enabled,
        }
    }

    fn restart_core(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        info!("user requested core restart");
        cx.spawn(async move |_entity, _cx| {
            let result = spawn_on_tokio(async move {
                let mgr = CoreManager::global();
                match mgr.status().await {
                    CoreStatus::Stopped | CoreStatus::Failed { .. } => mgr.start().await,
                    _ => mgr.restart().await,
                }
            })
            .await;
            if let Err(e) = result {
                warn!(error = %e, "core restart failed");
            }
        })
        .detach();
    }

    fn stop_core(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        info!("user requested core stop");
        // Drop the OS proxy alongside the core. If we leave it pointed at a
        // dead 7890 the user's apps would just fail to connect anywhere.
        let was_on = self.system_proxy_on;
        self.system_proxy_on = false;
        let prefs = Preferences {
            system_proxy_enabled: false,
            ..Preferences::load()
        };
        prefs.save();
        cx.notify();
        cx.spawn(async move |_entity, _cx| {
            let _ = spawn_on_tokio(async move {
                if was_on {
                    let _ = sysproxy::disable();
                }
                CoreManager::global().stop().await
            })
            .await;
        })
        .detach();
    }

    fn toggle_system_proxy(&mut self, on: bool, _window: &mut Window, cx: &mut Context<Self>) {
        // Optimistic UI: flip immediately, then push to OS off-thread. If the
        // shell-out fails we revert and notify.
        self.system_proxy_on = on;
        let prefs = Preferences {
            system_proxy_enabled: on,
            ..Preferences::load()
        };
        prefs.save();
        cx.notify();

        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async move {
                if on {
                    sysproxy::enable(MIHOMO_HOST, MIHOMO_MIXED_PORT)
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

    fn pick_theme(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = cx.theme().theme_name().to_string();
        if name == current {
            return;
        }
        if !theming::apply_theme_by_name(&name, window, cx) {
            return;
        }
        let prefs = Preferences {
            theme_name: Some(name),
            ..Preferences::load()
        };
        prefs.save();
        cx.notify();
    }

    fn install_helper(&mut self, cx: &mut Context<Self>) {
        info!("user requested helper service install");
        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async {
                tokio::task::spawn_blocking(|| {
                    crate::core::service_install::install_with_admin_prompt()
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("join error: {e}")))
            })
            .await;

            if let Err(e) = result {
                warn!(error = %e, "helper install failed");
            } else {
                info!("helper install ok");
            }
            // Re-render so the section reflects the new state.
            let _ = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |_this, cx| cx.notify());
                }
            });
        })
        .detach();
    }

    fn uninstall_helper(&mut self, cx: &mut Context<Self>) {
        info!("user requested helper service uninstall");
        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async {
                tokio::task::spawn_blocking(|| {
                    crate::core::service_install::uninstall_with_admin_prompt()
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("join error: {e}")))
            })
            .await;

            if let Err(e) = result {
                warn!(error = %e, "helper uninstall failed");
            } else {
                info!("helper uninstall ok");
            }
            let _ = cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |_this, cx| cx.notify());
                }
            });
        })
        .detach();
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_4()
            .child(div().font_bold().text_lg().child("Settings"))
            .child(self.appearance_section(cx))
            .child(self.system_proxy_section(cx))
            .child(self.helper_service_section(cx))
            .child(self.section(
                "Clash Core",
                "Manually control the mihomo subprocess. Normally not needed — the core starts and restarts automatically when you select a profile.",
                cx,
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("restart-core-btn")
                            .label("Restart Core")
                            .compact()
                            .on_click(cx.listener(|this, _e, w, cx| this.restart_core(w, cx))),
                    )
                    .child(
                        Button::new("stop-core-btn")
                            .label("Stop Core")
                            .compact()
                            .ghost()
                            .on_click(cx.listener(|this, _e, w, cx| this.stop_core(w, cx))),
                    )
                    .into_any_element(),
            ))
    }
}

impl SettingsPage {
    fn appearance_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let names = self.theme_names.clone();
        let current = cx.theme().theme_name().to_string();

        let popover = Popover::new("theme-picker")
            .trigger(
                Button::new("theme-picker-btn")
                    .label(SharedString::from(current.clone()))
                    .icon(IconName::ChevronDown)
                    .compact(),
            )
            .content(move |_state, _w, cx| {
                let names = names.clone();
                let current = cx.theme().theme_name().to_string();
                div()
                    .id("theme-list")
                    .py_1()
                    .min_w(px(220.))
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(
                        v_flex().children(names.into_iter().map(|name| {
                            let is_current = name == current;
                            let label = name.clone();
                            let n = name.clone();
                            h_flex()
                                .id(SharedString::from(format!("theme-opt-{}", name)))
                                .px_3()
                                .py_1p5()
                                .gap_2()
                                .items_center()
                                .cursor_pointer()
                                .when(is_current, |el| el.bg(cx.theme().accent.opacity(0.15)))
                                .hover(|s| s.bg(cx.theme().accent.opacity(0.10)))
                                .on_mouse_down(MouseButton::Left, {
                                    let n = n.clone();
                                    move |_ev, window, cx| {
                                        if theming::apply_theme_by_name(&n, window, cx) {
                                            let prefs = Preferences {
                                                theme_name: Some(n.clone()),
                                                ..Preferences::load()
                                            };
                                            prefs.save();
                                        }
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
                        })),
                    )
            });

        self.section(
            "Appearance",
            "Pick a theme. The choice is saved and restored next launch.",
            cx,
            h_flex().gap_2().child(popover).into_any_element(),
        )
    }

    fn section(
        &self,
        title: &str,
        description: &str,
        cx: &Context<Self>,
        body: AnyElement,
    ) -> impl IntoElement {
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
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(description.to_string()),
            )
            .child(div().pt_2().child(body))
    }

    fn system_proxy_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.system_proxy_on;
        let body = h_flex()
            .gap_3()
            .items_center()
            .child(
                Switch::new("system-proxy-switch")
                    .checked(on)
                    .on_click(cx.listener(|this, checked: &bool, w, cx| {
                        this.toggle_system_proxy(*checked, w, cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if on {
                        format!("Routing through {}:{}", MIHOMO_HOST, MIHOMO_MIXED_PORT)
                    } else {
                        "Off".to_string()
                    }),
            )
            .into_any_element();

        self.section(
            "System Proxy",
            "Send your Mac's HTTP, HTTPS, and SOCKS traffic through mihomo so apps and browsers go through the proxy. Requires the core to be running.",
            cx,
            body,
        )
    }

    fn helper_service_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let installed = crate::core::service_install::is_service_installed();
        let running = crate::core::service_install::is_service_running();

        let (label, color) = match (installed, running) {
            (true, true) => ("Installed and running", cx.theme().accent),
            (true, false) => ("Installed but not running", cx.theme().muted_foreground),
            (false, _) => ("Not installed", cx.theme().muted_foreground),
        };

        let buttons = if installed {
            h_flex().gap_2().child(
                Button::new("svc-uninstall")
                    .label("Uninstall Helper")
                    .compact()
                    .ghost()
                    .on_click(cx.listener(|this, _e, _w, cx| this.uninstall_helper(cx))),
            )
        } else {
            h_flex().gap_2().child(
                Button::new("svc-install")
                    .label("Install Helper")
                    .compact()
                    .on_click(cx.listener(|this, _e, _w, cx| this.install_helper(cx))),
            )
        };

        let body = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    ),
            )
            .child(buttons)
            .into_any_element();

        self.section(
            "Helper Service",
            "A small root-privileged daemon that launches mihomo. Required for TUN mode (which creates a virtual network adapter and needs admin rights). Installs to /Library/LaunchDaemons and survives reboots.",
            cx,
            body,
        )
    }
}
