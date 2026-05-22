//! Branding overrides applied on top of the gpui-component default theme.
//!
//! gpui-component picks reasonable defaults (a neutral gray accent) but we
//! want everything that's "selected / primary" to be our brand purple
//! `#7F60D3`. Centralising this here means individual pages can use
//! `cx.theme().primary` etc. and the colours follow theme switches
//! automatically.

use gpui::{App, Hsla, hsla};
use gpui_component::Theme;

/// ClashR brand color (#7F60D3).
pub const BRAND_PURPLE: Hsla = Hsla {
    h: 0.73,
    s: 0.55,
    l: 0.6,
    a: 1.0,
};

const BRAND_PURPLE_HOVER: Hsla = Hsla {
    h: 0.73,
    s: 0.55,
    l: 0.52,
    a: 1.0,
};

const BRAND_PURPLE_ACTIVE: Hsla = Hsla {
    h: 0.73,
    s: 0.55,
    l: 0.45,
    a: 1.0,
};

/// Apply ClashR's purple accent to the active gpui-component theme.
///
/// Call this once at startup, *after* `gpui_component::init(cx)`.
pub fn apply_brand(cx: &mut App) {
    let theme = Theme::global_mut(cx);

    // "Where is the primary brand color used?" tokens we override:
    theme.colors.primary = BRAND_PURPLE;
    theme.colors.primary_foreground = hsla(0.0, 0.0, 1.0, 1.0); // white

    theme.colors.button_primary = BRAND_PURPLE;
    theme.colors.button_primary_hover = BRAND_PURPLE_HOVER;
    theme.colors.button_primary_active = BRAND_PURPLE_ACTIVE;
    theme.colors.button_primary_foreground = hsla(0.0, 0.0, 1.0, 1.0);

    theme.colors.accent = BRAND_PURPLE;
    theme.colors.accent_foreground = hsla(0.0, 0.0, 1.0, 1.0);

    // Sidebar tokens.
    //
    // `sidebar_primary` is the *active* (selected) item's background — solid
    // purple with white text. We override both.
    theme.colors.sidebar_primary = BRAND_PURPLE;
    theme.colors.sidebar_primary_foreground = hsla(0.0, 0.0, 1.0, 1.0);
    // `sidebar_accent` is the *hover* background. We let it pick up our
    // brand purple (it's painted at .opacity(0.8) by the component, so the
    // result is still readable with the default dark sidebar_foreground).
    //
    // We deliberately DO NOT override `sidebar_accent_foreground`: keeping
    // it at the theme default (dark text) avoids a stale-paint glitch where
    // a previously-hovered item ends up with white-on-light text after the
    // mouse leaves, until the next event triggers a redraw.
    theme.colors.sidebar_accent = BRAND_PURPLE;

    theme.colors.ring = BRAND_PURPLE;
}
