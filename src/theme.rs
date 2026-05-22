use gpui::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

pub struct AppTheme {
    pub mode: ThemeMode,
}

impl Global for AppTheme {}

pub fn init(cx: &mut AppContext) {
    cx.set_global(AppTheme {
        mode: ThemeMode::Dark,
    });
}

pub fn toggle_theme(cx: &mut AppContext) {
    let current = cx.global::<AppTheme>().mode;
    cx.set_global(AppTheme {
        mode: match current {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        },
    });
}

pub fn is_dark(cx: &AppContext) -> bool {
    cx.global::<AppTheme>().mode == ThemeMode::Dark
}

pub fn bg_color(cx: &AppContext) -> Hsla {
    if is_dark(cx) {
        hsla(0.0, 0.0, 0.1, 1.0)
    } else {
        hsla(0.0, 0.0, 0.98, 1.0)
    }
}

pub fn sidebar_bg(cx: &AppContext) -> Hsla {
    if is_dark(cx) {
        hsla(0.0, 0.0, 0.12, 1.0)
    } else {
        hsla(0.0, 0.0, 0.95, 1.0)
    }
}

pub fn text_color(cx: &AppContext) -> Hsla {
    if is_dark(cx) {
        hsla(0.0, 0.0, 0.9, 1.0)
    } else {
        hsla(0.0, 0.0, 0.1, 1.0)
    }
}

pub fn text_muted(cx: &AppContext) -> Hsla {
    if is_dark(cx) {
        hsla(0.0, 0.0, 0.5, 1.0)
    } else {
        hsla(0.0, 0.0, 0.4, 1.0)
    }
}

pub fn border_color(cx: &AppContext) -> Hsla {
    if is_dark(cx) {
        hsla(0.0, 0.0, 0.2, 1.0)
    } else {
        hsla(0.0, 0.0, 0.88, 1.0)
    }
}

pub fn accent_color() -> Hsla {
    hsla(0.58, 0.7, 0.5, 1.0)
}
