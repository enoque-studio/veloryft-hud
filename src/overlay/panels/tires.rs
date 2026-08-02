//! The tire panel: per-corner temperature, plus the session range each corner
//! has covered.
//!
//! Colors come from `telemetry::tire_temperature`, which owns the temperature
//! scale; this module only lays the readouts out.

use windows::Win32::Graphics::Direct2D::Common::*;
use windows_numerics::Vector2;

use super::Panel;
use crate::localization::text;
use crate::overlay::geometry::{rectf, rounded};
use crate::overlay::theme::color;
use crate::telemetry::display::DisplayFrame;
use crate::telemetry::tire_temperature::temp_color;

impl Panel {
    // Drawing primitive: geometry plus the values it renders. Called per frame,
    // so the arguments stay by value rather than behind a struct reference.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_tire_range(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        cur: f32,
        min_v: Option<f32>,
        max_v: Option<f32>,
        accent: D2D1_COLOR_F,
    ) {
        let lo = min_v.unwrap_or(cur);
        let hi = max_v.unwrap_or(cur);
        let pad = (hi - lo).max(8.0) * 0.15;
        let t0 = (lo - pad).min(cur);
        let t1 = (hi + pad).max(cur);
        let span = (t1 - t0).max(1.0);
        let norm = |t: f32| ((t - t0) / span).clamp(0.0, 1.0);
        let cur_n = norm(cur);
        let lo_n = norm(lo);
        let hi_n = norm(hi);
        let track = rounded(rectf(x, y, x + w, y + h), h / 2.0);
        unsafe {
            self.rt.FillRoundedRectangle(&track, &self.b.track);
            if min_v.is_some() && max_v.is_some() && hi_n > lo_n + 0.02 {
                let mut band = accent;
                band.a = 0.28;
                self.b.scratch.SetColor(&band);
                self.rt.FillRoundedRectangle(
                    &rounded(rectf(x + w * lo_n, y, x + w * hi_n, y + h), h / 2.0),
                    &self.b.scratch,
                );
                self.b.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.35));
                let tick_h = h * 1.8;
                for n in [lo_n, hi_n] {
                    let tx = x + w * n;
                    self.rt.DrawLine(
                        Vector2 {
                            X: tx,
                            Y: y + h / 2.0 - tick_h / 2.0,
                        },
                        Vector2 {
                            X: tx,
                            Y: y + h / 2.0 + tick_h / 2.0,
                        },
                        &self.b.scratch,
                        1.0,
                        None,
                    );
                }
            }
            let mx = x + w * cur_n;
            let mut dot = accent;
            dot.a = 1.0;
            self.b.scratch.SetColor(&dot);
            self.rt.FillRoundedRectangle(
                &rounded(
                    rectf(mx - h * 0.55, y - h * 0.15, mx + h * 0.55, y + h + h * 0.15),
                    h,
                ),
                &self.b.scratch,
            );
        }
    }
    pub(super) fn draw_tires(&self, f: &DisplayFrame, w: f32, h: f32) {
        let messages = text();
        let pad = (w * 0.05).max(8.0);
        let head_h = (h * 0.09).max(20.0);
        let gap = pad * 0.85;
        let grid_top = head_h + pad * 0.55;
        let grid_w = w - pad * 2.0;
        let grid_h = h - grid_top - pad;
        let cell_w = ((grid_w - gap) * 0.5).max(44.0);
        let cell_h = ((grid_h - gap) * 0.5).max(44.0);
        let grid_x = pad + (grid_w - (cell_w * 2.0 + gap)).max(0.0) * 0.5;
        let cell_r = (cell_w.min(cell_h) * 0.11).clamp(8.0, 14.0);
        let inner = (cell_w.min(cell_h) * 0.09).clamp(6.0, 10.0);

        self.text(
            messages.tires,
            &self.f.small_left,
            rectf(pad, pad * 0.35, pad + grid_w * 0.55, head_h),
            &self.b.gray,
        );
        self.text(
            messages.degrees_celsius,
            &self.f.small_right,
            rectf(w - pad - 36.0, pad * 0.35, w - pad, head_h),
            &self.b.gray,
        );
        unsafe {
            self.rt.DrawLine(
                Vector2 {
                    X: pad,
                    Y: head_h + 2.0,
                },
                Vector2 {
                    X: w - pad,
                    Y: head_h + 2.0,
                },
                &self.b.track_edge,
                1.0,
                None,
            );
        }

        let corners = messages.tire_corners;
        let positions = [
            (grid_x, grid_top),
            (grid_x + cell_w + gap, grid_top),
            (grid_x, grid_top + cell_h + gap),
            (grid_x + cell_w + gap, grid_top + cell_h + gap),
        ];

        for i in 0..4 {
            let (x, y) = positions[i];
            let (cur, min_v, max_v) = match (f.tires, f.tire_min, f.tire_max, f.fresh) {
                (Some(t), min, max, true) if t[i].is_finite() && t[i] > 0.0 => {
                    let lo = min.and_then(|m| m[i].is_finite().then_some(m[i]));
                    let hi = max.and_then(|m| m[i].is_finite().then_some(m[i]));
                    (Some(t[i]), lo, hi)
                }
                _ => (None, None, None),
            };

            let outer = rounded(rectf(x, y, x + cell_w, y + cell_h), cell_r);
            unsafe {
                self.b.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.035));
                self.rt.FillRoundedRectangle(&outer, &self.b.scratch);
                self.rt
                    .DrawRoundedRectangle(&outer, &self.b.track_edge, 0.75, None);
            }

            // Corner badge.
            let badge_h = (cell_h * 0.16).clamp(14.0, 20.0);
            let badge_w = (cell_w * 0.30).clamp(28.0, 44.0);
            let badge = rounded(
                rectf(
                    x + inner,
                    y + inner,
                    x + inner + badge_w,
                    y + inner + badge_h,
                ),
                badge_h / 2.0,
            );
            unsafe {
                self.rt.FillRoundedRectangle(&badge, &self.b.track);
            }
            self.text(
                corners[i],
                &self.f.micro,
                rectf(
                    x + inner,
                    y + inner,
                    x + inner + badge_w,
                    y + inner + badge_h,
                ),
                &self.b.gray,
            );

            // Current temperature.
            let temp_top = y + cell_h * 0.30;
            let temp_bot = y + cell_h * 0.62;
            let bar_h = (cell_h * 0.07).clamp(4.0, 6.0);
            let bar_y = y + cell_h * 0.68;
            let bar_x = x + inner;
            let bar_w = cell_w - inner * 2.0;
            let foot_top = bar_y + bar_h + inner * 0.35;
            let foot_bot = y + cell_h - inner * 0.45;

            if let Some(temp) = cur {
                let (r, g, b, a) = temp_color(temp);
                let accent = color(r, g, b, a);
                let temp_s = format!("{:.0}", temp);
                unsafe { self.b.scratch.SetColor(&accent) };
                self.text(
                    &temp_s,
                    &self.f.tire,
                    rectf(x + inner, temp_top, x + cell_w - inner, temp_bot),
                    &self.b.scratch,
                );
                self.draw_tire_range(bar_x, bar_y, bar_w, bar_h, temp, min_v, max_v, accent);

                let min_s = min_v
                    .map(|v| format!("{:.0}", v))
                    .unwrap_or_else(|| messages.missing_value.to_string());
                let max_s = max_v
                    .map(|v| format!("{:.0}", v))
                    .unwrap_or_else(|| messages.missing_value.to_string());
                self.text(
                    &min_s,
                    &self.f.small_left,
                    rectf(bar_x, foot_top, bar_x + bar_w * 0.48, foot_bot),
                    &self.b.white,
                );
                self.text(
                    &max_s,
                    &self.f.small_right,
                    rectf(bar_x + bar_w * 0.52, foot_top, bar_x + bar_w, foot_bot),
                    &self.b.white,
                );
            } else {
                self.text(
                    messages.missing_value,
                    &self.f.tire,
                    rectf(x + inner, temp_top, x + cell_w - inner, temp_bot),
                    &self.b.gray,
                );
            }
        }
    }
}
