use gpui::*;
use gpui_component::{
    ActiveTheme, StyledExt as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
};
use tracing::{info, warn};

use crate::core::{CoreManager, CoreStatus};
use crate::runtime::spawn_on_tokio;

pub struct SettingsPage;

impl SettingsPage {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
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
        cx.spawn(async move |_entity, _cx| {
            let _ = spawn_on_tokio(async move { CoreManager::global().stop().await }).await;
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
}
