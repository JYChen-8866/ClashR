use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_component::{
    ActiveTheme, IconName, StyledExt as _, h_flex, v_flex, Root, WindowExt as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::runtime::spawn_on_tokio;
use crate::services::subscription;
use crate::core::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfilesState {
    profiles: Vec<ProfileItem>,
    current_uid: Option<String>,
}

impl ProfilesState {
    fn path() -> std::path::PathBuf {
        paths::data_dir().join("profiles.json")
    }

    fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(state) = serde_json::from_str(&data) {
                    return state;
                }
            }
        }
        Self { profiles: Vec::new(), current_uid: None }
    }

    fn save(profiles: &[ProfileItem], current_uid: &Option<String>) {
        let state = ProfilesState {
            profiles: profiles.to_vec(),
            current_uid: current_uid.clone(),
        };
        let path = Self::path();
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(&path, json);
        }
    }
}

const GRID_GAP: Pixels = px(16.);
const CARD_MIN_WIDTH: Pixels = px(320.);
const CARD_MAX_WIDTH: Pixels = px(400.);

fn brand_color(cx: &App) -> Hsla {
    cx.theme().primary
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileItem {
    pub uid: String,
    #[serde(rename = "type")]
    pub profile_type: String,
    pub name: String,
    pub desc: Option<String>,
    pub url: Option<String>,
    pub updated: Option<u64>,
    pub extra: Option<ProfileExtra>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileExtra {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: u64,
}

pub struct ProfilesPage {
    profiles: Vec<ProfileItem>,
    current_uid: Option<String>,
    url_input: Entity<InputState>,
    desc_input: Entity<InputState>,
    edit_url_input: Entity<InputState>,
    edit_desc_input: Entity<InputState>,
}

impl ProfilesPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://example.com/subscription")
        });
        let desc_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("My Proxy (optional)")
        });
        let edit_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://example.com/subscription")
        });
        let edit_desc_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Description")
        });

        let state = ProfilesState::load();
        let initial_profiles = state.profiles.clone();
        let initial_current_uid = state.current_uid.clone();

        // If we have a previously-active profile on disk, activate and start
        // the core in the background so the user is up & running immediately.
        if let Some(uid) = initial_current_uid.clone() {
            cx.spawn(async move |_entity, _cx| {
                let _ = spawn_on_tokio(async move {
                    let mgr = crate::core::CoreManager::global();
                    if mgr.activate_profile(&uid).is_ok() {
                        let _ = mgr.start().await;
                    }
                    // Re-apply the system proxy if the user had it on last
                    // session. We do this *after* the core start attempt so
                    // we don't point traffic at a dead port.
                    if crate::theming::Preferences::load().system_proxy_enabled {
                        if matches!(
                            mgr.status().await,
                            crate::core::CoreStatus::Running { .. }
                        ) {
                            let _ = crate::core::sysproxy::enable("127.0.0.1", 7890);
                        }
                    }
                    anyhow::Ok(())
                }).await;
            }).detach();
        }

        Self {
            profiles: initial_profiles,
            current_uid: initial_current_uid,
            url_input,
            desc_input,
            edit_url_input,
            edit_desc_input,
        }
    }

    fn persist(&self) {
        ProfilesState::save(&self.profiles, &self.current_uid);
    }

    fn show_import_dialog(&mut self, _event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let url_input = self.url_input.clone();
        let desc_input = self.desc_input.clone();
        let entity = cx.entity().downgrade();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let url_input_clone = url_input.clone();
            let desc_input_clone = desc_input.clone();
            let entity_clone = entity.clone();

            dialog
                .title("Import Profile")
                .child(
                    v_flex()
                        .gap_3()
                        .p_4()
                        .child(div().text_sm().child("Description"))
                        .child(Input::new(&desc_input))
                        .child(div().text_sm().child("Subscription URL"))
                        .child(Input::new(&url_input))
                )
                .on_ok(move |_event, _window, cx| {
                    if let Some(entity) = entity_clone.upgrade() {
                        let url = url_input_clone.read(cx).text().to_string();
                        let desc = desc_input_clone.read(cx).text().to_string();
                        let desc = if desc.trim().is_empty() { None } else { Some(desc) };
                        entity.update(cx, |this, cx| {
                            this.do_import_profile(url, desc, cx);
                        });
                    }
                    true
                })
        });
    }

    fn do_import_profile(&mut self, url: String, desc: Option<String>, cx: &mut Context<Self>) {
        if url.trim().is_empty() {
            return;
        }

        let uid = format!("R{:08x}", rand_u32());
        let name = desc.clone().unwrap_or_else(|| extract_name_from_url(&url));

        info!(uid = %uid, name = %name, "importing profile");

        let profile = ProfileItem {
            uid: uid.clone(),
            profile_type: "remote".to_string(),
            name: name.clone(),
            desc,
            url: Some(url.clone()),
            updated: Some(now_timestamp()),
            extra: None,
        };

        self.profiles.push(profile);
        if self.current_uid.is_none() {
            self.current_uid = Some(uid.clone());
        }
        self.persist();
        cx.notify();

        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async move {
                subscription::fetch_subscription_direct(&url).await
            }).await;

            cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        let is_current = this.current_uid.as_deref() == Some(&uid);
                        if let Some(profile) = this.profiles.iter_mut().find(|p| p.uid == uid) {
                            match result {
                                Ok(fetch_result) => {
                                    info!(uid = %profile.uid, "profile fetched successfully");
                                    if let Some(remote_name) = fetch_result.name {
                                        if profile.desc.is_none() {
                                            profile.name = remote_name;
                                        }
                                    }
                                    if let Some(extra) = fetch_result.extra {
                                        profile.extra = Some(ProfileExtra {
                                            upload: extra.upload,
                                            download: extra.download,
                                            total: extra.total,
                                            expire: extra.expire,
                                        });
                                    }
                                    profile.updated = Some(now_timestamp());

                                    // Persist body to disk for the core to consume.
                                    let mgr = crate::core::CoreManager::global();
                                    if let Err(e) = mgr.save_profile(&profile.uid, &fetch_result.body) {
                                        warn!(uid = %profile.uid, error = %e, "failed to save profile body");
                                    } else if is_current {
                                        // If this is the active profile, activate
                                        // and (re)start the core automatically.
                                        let uid_for_core = profile.uid.clone();
                                        cx.spawn(async move |_entity, _cx| {
                                            let _ = spawn_on_tokio(async move {
                                                mgr.activate_profile(&uid_for_core)?;
                                                match mgr.status().await {
                                                    crate::core::CoreStatus::Stopped | crate::core::CoreStatus::Failed { .. } => {
                                                        mgr.start().await?;
                                                    }
                                                    _ => {
                                                        mgr.restart().await?;
                                                    }
                                                }
                                                anyhow::Ok(())
                                            }).await;
                                        }).detach();
                                    }
                                }
                                Err(e) => {
                                    warn!(uid = %profile.uid, error = %e, "profile fetch failed");
                                    profile.name = format!("{} (fetch failed)", profile.name);
                                }
                            }
                        }
                        this.persist();
                        cx.notify();
                    });
                }
            });
        }).detach();
    }

    fn select_profile(&mut self, uid: String, _window: &mut Window, cx: &mut Context<Self>) {
        if self.current_uid.as_deref() == Some(&uid) {
            return;
        }
        self.current_uid = Some(uid.clone());
        self.persist();
        cx.notify();

        // Activate the runtime config and hot-reload via mihomo API.
        // If core isn't running yet, it will be started.
        cx.spawn(async move |_entity, _cx| {
            let result = spawn_on_tokio(async move {
                let mgr = crate::core::CoreManager::global();
                mgr.activate_profile(&uid)?;
                mgr.reload_config().await
            }).await;

            if let Err(e) = result {
                warn!(error = %e, "failed to switch profile");
            }
        }).detach();
    }

    fn edit_profile(&mut self, uid: String, window: &mut Window, cx: &mut Context<Self>) {
        // Also select it
        self.current_uid = Some(uid.clone());
        self.persist();
        cx.notify();

        let profile = match self.profiles.iter().find(|p| p.uid == uid) {
            Some(p) => p.clone(),
            None => return,
        };

        // Pre-fill edit inputs
        let edit_url = self.edit_url_input.clone();
        let edit_desc = self.edit_desc_input.clone();

        edit_url.update(cx, |state, cx| {
            state.set_value(profile.url.as_deref().unwrap_or(""), window, cx);
        });
        edit_desc.update(cx, |state, cx| {
            state.set_value(&profile.name, window, cx);
        });

        let entity = cx.entity().downgrade();
        let edit_uid = uid.clone();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let edit_url_clone = edit_url.clone();
            let edit_desc_clone = edit_desc.clone();
            let entity_clone = entity.clone();
            let uid_clone = edit_uid.clone();

            dialog
                .title("Edit Profile")
                .child(
                    v_flex()
                        .gap_3()
                        .p_4()
                        .child(div().text_sm().child("Name"))
                        .child(Input::new(&edit_desc))
                        .child(div().text_sm().child("Subscription URL"))
                        .child(Input::new(&edit_url))
                )
                .on_ok(move |_event, _window, cx| {
                    if let Some(entity) = entity_clone.upgrade() {
                        let new_url = edit_url_clone.read(cx).text().to_string();
                        let new_name = edit_desc_clone.read(cx).text().to_string();
                        entity.update(cx, |this, cx| {
                            if let Some(profile) = this.profiles.iter_mut().find(|p| p.uid == uid_clone) {
                                if !new_name.trim().is_empty() {
                                    profile.name = new_name;
                                }
                                if !new_url.trim().is_empty() {
                                    profile.url = Some(new_url);
                                }
                                profile.updated = Some(now_timestamp());
                            }
                            this.persist();
                            cx.notify();
                        });
                    }
                    true
                })
        });
    }

    fn delete_profile(&mut self, uid: String, _window: &mut Window, cx: &mut Context<Self>) {
        info!(uid = %uid, "deleting profile");
        let was_active = self.current_uid.as_deref() == Some(&uid);
        self.profiles.retain(|p| p.uid != uid);
        if was_active {
            self.current_uid = self.profiles.first().map(|p| p.uid.clone());
        }
        // Also remove the on-disk YAML.
        let yaml_path = paths::profile_yaml_path(&uid);
        let _ = std::fs::remove_file(&yaml_path);
        self.persist();
        cx.notify();

        // If we deleted the active profile, activate the new current one
        // so mihomo reloads with the correct config.
        if was_active {
            if let Some(new_uid) = self.current_uid.clone() {
                cx.spawn(async move |_e, _cx| {
                    let _ = crate::runtime::spawn_on_tokio(async move {
                        let mgr = crate::core::CoreManager::global();
                        let _ = mgr.activate_profile(&new_uid);
                        let _ = mgr.restart().await;
                    }).await;
                }).detach();
            }
        }
    }

    fn update_profile(&mut self, uid: String, _window: &mut Window, cx: &mut Context<Self>) {
        let url = match self.profiles.iter().find(|p| p.uid == uid) {
            Some(p) => match &p.url {
                Some(u) => u.clone(),
                None => return,
            },
            None => return,
        };

        info!(uid = %uid, "updating profile");

        let uid_clone = uid.clone();
        cx.spawn(async move |entity, cx| {
            let result = spawn_on_tokio(async move {
                subscription::fetch_subscription_direct(&url).await
            }).await;

            cx.update(|cx| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        if let Some(profile) = this.profiles.iter_mut().find(|p| p.uid == uid_clone) {
                            match result {
                                Ok(fetch_result) => {
                                    info!(uid = %profile.uid, "profile updated");
                                    if let Some(extra) = fetch_result.extra {
                                        profile.extra = Some(ProfileExtra {
                                            upload: extra.upload,
                                            download: extra.download,
                                            total: extra.total,
                                            expire: extra.expire,
                                        });
                                    }
                                    profile.updated = Some(now_timestamp());
                                }
                                Err(e) => {
                                    warn!(uid = %profile.uid, error = %e, "profile update failed");
                                    profile.updated = Some(now_timestamp());
                                }
                            }
                        }
                        this.persist();
                        cx.notify();
                    });
                }
            });
        }).detach();

        cx.notify();
    }

    fn render_profile_card(&self, profile: &ProfileItem, cx: &mut Context<Self>) -> impl IntoElement {
        let is_active = self.current_uid.as_deref() == Some(&profile.uid);
        let uid = profile.uid.clone();
        let uid_select = uid.clone();
        let uid_update = uid.clone();
        let uid_delete = uid.clone();

        let border_color = if is_active {
            brand_color(cx)
        } else {
            cx.theme().border
        };

        let title_color = if is_active {
            brand_color(cx)
        } else {
            cx.theme().foreground
        };

        let domain = profile
            .url
            .as_deref()
            .map(extract_domain)
            .unwrap_or_default();

        let extra = profile.extra.clone();

        v_flex()
            .id(format!("profile-{}", uid))
            .w_full()
            .h_full()
            .px_4()
            .py_3()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(border_color)
            .bg(cx.theme().background)
            .cursor_pointer()
            .when(!is_active, |el| {
                el.hover(|s| s.bg(cx.theme().muted.opacity(0.3)))
            })
            .on_click(cx.listener({
                let uid = uid_select.clone();
                move |this, event: &ClickEvent, window, cx| {
                    if event.click_count() >= 2 {
                        this.edit_profile(uid.clone(), window, cx);
                    } else {
                        this.select_profile(uid.clone(), window, cx);
                    }
                }
            }))
            // Header: name + active tag + actions
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
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_base()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(title_color)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(profile.name.clone()),
                            )
                            .when(is_active, |el| {
                                el.child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(brand_color(cx))
                                        .text_color(gpui::white())
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .flex_shrink_0()
                                        .child("Active"),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                Button::new(format!("update-{}", uid_update))
                                    .icon(IconName::Redo)
                                    .compact()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _event, window, cx| {
                                        this.update_profile(uid_update.clone(), window, cx);
                                    })),
                            )
                            .child(
                                Button::new(format!("delete-{}", uid_delete))
                                    .icon(IconName::Delete)
                                    .compact()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _event, window, cx| {
                                        this.delete_profile(uid_delete.clone(), window, cx);
                                    })),
                            ),
                    ),
            )
            // Sub-row: domain + relative time
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(domain),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .flex_shrink_0()
                            .child(format_timestamp(profile.updated)),
                    ),
            )
            // Optional traffic info
            .when_some(extra, |el, e| {
                let used = e.upload + e.download;
                let total = e.total;
                let percent = if total > 0 {
                    (used as f32 / total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let used_total = format!("{} / {}", format_bytes(used), format_bytes(total));
                let expire_str = format_expire(e.expire);

                el.child(
                    v_flex()
                        .gap_1p5()
                        .pt_1()
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(used_total),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(expire_str),
                                ),
                        )
                        .child(
                            div()
                                .h(px(4.))
                                .w_full()
                                .rounded_full()
                                .bg(brand_color(cx).opacity(0.18))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .bg(brand_color(cx))
                                        .w(relative(percent))
                                        .rounded_full(),
                                ),
                        ),
                )
            })
    }
}

impl Render for ProfilesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let profiles: Vec<_> = self.profiles.clone();

        v_flex()
            .size_full()
            .gap_4()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .flex_shrink_0()
                    .child(div().font_bold().text_lg().child("Profiles"))
                    .child(
                        h_flex().gap_3().items_center()
                            .child(
                                Button::new("import-btn")
                                    .label("Import")
                                    .icon(IconName::Plus)
                                    .on_click(cx.listener(Self::show_import_dialog)),
                            ),
                    ),
            )
            .when(profiles.is_empty(), |el| {
                el.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .text_base()
                                .child("No profiles yet"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Click \"Import\" to add a subscription URL"),
                        ),
                )
            })
            .when(!profiles.is_empty(), |el| {
                el.child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(GRID_GAP)
                                .children(
                                    profiles.iter().map(|p| {
                                        div()
                                            .flex_basis(CARD_MIN_WIDTH)
                                            .flex_grow()
                                            .max_w(CARD_MAX_WIDTH)
                                            .child(self.render_profile_card(p, cx))
                                    })
                                ),
                        )
                )
            })
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn rand_u32() -> u32 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}

fn now_timestamp() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn extract_name_from_url(url: &str) -> String {
    // Use the host's primary label as the default name (e.g.
    // "https://hhh.02000.xin/api/v1/client/subscribe?token=..." -> "02000").
    // Falls back to the full host or "Profile".
    let host = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("");

    if host.is_empty() {
        return "Profile".to_string();
    }

    // Strip a leading "www." for cleanliness.
    let host = host.strip_prefix("www.").unwrap_or(host);

    // Take the second-to-last label when possible (e.g. "02000.xin" -> "02000").
    let labels: Vec<&str> = host.split('.').collect();
    let pick = match labels.len() {
        0 => return "Profile".to_string(),
        1 => labels[0],
        _ => labels[labels.len() - 2],
    };

    pick.to_string()
}

fn extract_domain(url: &str) -> String {
    let s = url.trim_start_matches("http://").trim_start_matches("https://");
    s.split('/').next().unwrap_or(s).to_string()
}

fn format_expire(expire_secs: u64) -> String {
    if expire_secs == 0 {
        return String::new();
    }
    let now = now_timestamp();
    if expire_secs <= now {
        return "expired".to_string();
    }
    let days = (expire_secs - now) / 86400;
    let secs_per_day = 86400u64;
    let date_days = expire_secs / secs_per_day;
    let (year, month, day) = days_to_ymd(date_days);
    if days <= 30 {
        format!("{:04}-{:02}-{:02} ({}d left)", year, month, day, days)
    } else {
        format!("{:04}-{:02}-{:02}", year, month, day)
    }
}

/// Convert days since 1970-01-01 to (year, month, day). Civil-from-days algo.
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as u32;
    (year, m, d)
}

fn format_timestamp(ts: Option<u64>) -> String {
    match ts {
        Some(t) => {
            let now = now_timestamp();
            let diff = now.saturating_sub(t);
            if diff < 60 {
                "just now".to_string()
            } else if diff < 3600 {
                format!("{}m ago", diff / 60)
            } else if diff < 86400 {
                format!("{}h ago", diff / 3600)
            } else {
                format!("{}d ago", diff / 86400)
            }
        }
        None => "never".to_string(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
