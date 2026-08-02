//! Colors, the editable theme, and color-space conversion.
//!
//! Everything the overlay draws takes its accent and glass appearance from a
//! single [`Theme`] value. The theme lives in thread-local storage because only
//! the UI thread mutates it (mouse wheel and right-click on the edit toolbar),
//! and the render loop reads it once per frame.
//!
//! Persisted by [`super::settings`]; the constants here are the fallbacks used
//! before any settings file exists.

use std::cell::Cell;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

/// User-editable appearance, owned by the UI thread.
///
/// The mouse wheel over the edit toolbar changes the accent hue; right-click
/// switches whether the shift highlight tints the gear or the whole card.
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    /// Accent hue in degrees, 0..360.
    pub hue: f32,
    /// `true` tints the gear readout, `false` tints the card.
    pub gear_target: bool,
    /// Glass opacity (alpha at the top; the bottom scales with it).
    pub glass_alpha: f32,
    /// Glass tint: negative means neutral dark, otherwise a hue in 0..360.
    pub glass_tone: f32,
}

/// Appearance before any settings file has been written.
pub(crate) const THEME_DEFAULT: Theme = Theme {
    hue: 4.0,
    gear_target: false,
    glass_alpha: 0.62,
    glass_tone: -1.0,
};

thread_local! {
    /// The live theme. Thread-local by design: only the UI thread writes it.
    pub(crate) static THEME: Cell<Theme> = const { Cell::new(THEME_DEFAULT) };
}

/// Generic HSV to RGB conversion.
pub(crate) fn hsv_color(h: f32, s: f32, v: f32, a: f32) -> D2D1_COLOR_F {
    let c = v * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    color(r + m, g + m, b + m, a)
}

/// The accent color for a hue, at the fixed saturation and value the HUD uses.
pub(crate) fn hue_color(h: f32) -> D2D1_COLOR_F {
    hsv_color(h, 0.82, 0.96, 1.0)
}

/// Compact constructor for the Direct2D color literals below.
pub(crate) const fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

pub(crate) const C_GREEN: D2D1_COLOR_F = color(0.204, 0.780, 0.349, 1.0);
pub(crate) const C_AMBER: D2D1_COLOR_F = color(0.902, 0.718, 0.282, 1.0);
pub(crate) const C_RED: D2D1_COLOR_F = color(0.961, 0.263, 0.212, 1.0);
pub(crate) const C_TRACK: D2D1_COLOR_F = color(1.0, 1.0, 1.0, 0.08);
pub(crate) const C_TRACK_EDGE: D2D1_COLOR_F = color(1.0, 1.0, 1.0, 0.05);
pub(crate) const C_WHITE: D2D1_COLOR_F = color(0.98, 0.98, 0.99, 1.0);
pub(crate) const C_GRAY: D2D1_COLOR_F = color(0.620, 0.667, 0.714, 0.92);
pub(crate) const C_EDIT: D2D1_COLOR_F = color(0.40, 0.80, 1.0, 0.9);
pub(crate) const C_ELEV_LINE: D2D1_COLOR_F = color(0.224, 0.780, 1.0, 1.0);
pub(crate) const C_ELEV_MARKER: D2D1_COLOR_F = color(1.0, 0.310, 0.541, 1.0);

// Polished dark glass: it tints the game underneath rather than fogging it —
// no milky light grey — with a subtle vertical gradient and no border, because
// the card's own contrast provides the outline. These are the neutral base RGB
// values; opacity and tint come from the theme (the GLASS and TONE sliders).
pub(crate) const GLASS_TOP: D2D1_COLOR_F = color(0.071, 0.078, 0.098, 0.62); // #121419
pub(crate) const GLASS_BOT: D2D1_COLOR_F = color(0.027, 0.031, 0.043, 0.48); // #07080b
pub(crate) const GLASS_ALPHA_MIN: f32 = 0.12;
pub(crate) const GLASS_ALPHA_MAX: f32 = 0.95;

/// Top and bottom of the glass gradient for a theme: adjustable opacity plus an
/// optional tint (negative means neutral dark, otherwise a muted hue).
pub(crate) fn glass_colors(t: Theme) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
    let a_top = t.glass_alpha.clamp(GLASS_ALPHA_MIN, GLASS_ALPHA_MAX);
    let a_bot = a_top * 0.77;
    if t.glass_tone >= 0.0 {
        (
            hsv_color(t.glass_tone, 0.50, 0.17, a_top),
            hsv_color(t.glass_tone, 0.55, 0.075, a_bot),
        )
    } else {
        let mut top = GLASS_TOP;
        top.a = a_top;
        let mut bot = GLASS_BOT;
        bot.a = a_bot;
        (top, bot)
    }
}
