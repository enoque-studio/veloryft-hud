//! The cluster panel: RPM bar, gear, speed, and the pedal rows.
//!
//! This is the panel a driver actually watches, so it is the one the frame
//! governor never rate-limits. The RPM bar carries the learned shift marks —
//! a white tick at the recommended upshift and a red band up to the observed
//! limiter — both supplied by `telemetry::rev_limit`, not by a fixed fraction
//! of the redline.

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows_numerics::Vector2;

use super::Panel;
use crate::overlay::geometry::{rectf, rounded};
use crate::overlay::geometry::{
    CL_BAR_H, CL_BAR_X, CL_BRAKE_Y, CL_GEAR_W, CL_GEAR_Y_SHIFT, CL_H, CL_META_Y, CL_PAD,
    CL_PEDAL_H, CL_RIGHT_R, CL_RIGHT_X, CL_RPM_Y, CL_SPEED_Y, CL_W,
};
use crate::overlay::theme::{color, C_RED};
use crate::telemetry::display::DisplayFrame;

impl Panel {
    pub(super) fn pedal_row_right(
        &self,
        label: &str,
        frac: f32,
        fill: &ID2D1SolidColorBrush,
        y: f32,
        w: f32,
    ) {
        let sx = w / CL_W as f32;
        let label_w = 64.0 * sx;
        let gap = 8.0 * sx;
        let right_x = CL_RIGHT_X / CL_W as f32 * w;
        let right_r = CL_RIGHT_R / CL_W as f32 * w;
        let bar_x = right_x + label_w + gap;
        let bar_w = right_r - bar_x;
        let th = CL_PEDAL_H * sx;
        self.text(
            label,
            &self.f.small_left,
            rectf(right_x, y - 9.0 * sx, right_x + label_w, y + 9.0 * sx),
            &self.b.gray,
        );
        let track = rounded(
            rectf(bar_x, y - th / 2.0, bar_x + bar_w, y + th / 2.0),
            th / 2.0,
        );
        unsafe {
            self.rt.FillRoundedRectangle(&track, &self.b.track);
            self.rt
                .DrawRoundedRectangle(&track, &self.b.track_edge, 0.75, None);
            let f = frac.clamp(0.0, 1.0);
            if f > 0.001 {
                self.rt.FillRoundedRectangle(
                    &rounded(
                        rectf(bar_x, y - th / 2.0, bar_x + bar_w * f, y + th / 2.0),
                        th / 2.0,
                    ),
                    fill,
                );
            }
        }
    }
    pub(super) fn draw_cluster(
        &self,
        f: &DisplayFrame,
        accent: D2D1_COLOR_F,
        gear_accent: bool,
        w: f32,
        h: f32,
    ) {
        let px = |x: f32| x / CL_W as f32 * w;
        let py = |y: f32| y / CL_H as f32 * h;
        let gear = match f.gear {
            Some(0) => "R".to_string(),
            Some(g) => g.to_string(),
            None => "–".to_string(),
        };
        if f.near && gear_accent {
            unsafe { self.b.scratch.SetColor(&accent) };
        }
        let gb = if f.near && gear_accent {
            &self.b.scratch
        } else {
            &self.b.white
        };
        self.text(
            &gear,
            &self.f.gear,
            rectf(
                px(CL_BAR_X),
                py(CL_PAD + CL_GEAR_Y_SHIFT),
                px(CL_BAR_X + CL_GEAR_W),
                h - py(CL_PAD) + py(CL_GEAR_Y_SHIFT),
            ),
            gb,
        );

        let rpm_y = py(CL_RPM_Y);
        let bar_h = py(CL_BAR_H);
        unsafe {
            self.rt.FillRoundedRectangle(
                &rounded(
                    rectf(px(CL_RIGHT_X), rpm_y, px(CL_RIGHT_R), rpm_y + bar_h),
                    bar_h / 2.0,
                ),
                &self.b.track,
            );
            let rpm_track = rounded(
                rectf(px(CL_RIGHT_X), rpm_y, px(CL_RIGHT_R), rpm_y + bar_h),
                bar_h / 2.0,
            );
            self.rt
                .DrawRoundedRectangle(&rpm_track, &self.b.track_edge, 0.75, None);
            if f.rpm_frac > 0.001 {
                let rw = (px(CL_RIGHT_R) - px(CL_RIGHT_X)) * f.rpm_frac;
                self.rt.FillRoundedRectangle(
                    &rounded(
                        rectf(px(CL_RIGHT_X), rpm_y, px(CL_RIGHT_X) + rw, rpm_y + bar_h),
                        bar_h / 2.0,
                    ),
                    &self.b.grad,
                );
            }

            // Zona de troca aprendida por carro (revlimit): banda vermelha do
            // ponto de troca até o corte, tick branco na troca, tick vermelho
            // no corte quando o limitador já foi observado.
            if f.shift_frac > 0.05 && f.shift_frac < 0.995 {
                let bar_x0 = px(CL_RIGHT_X);
                let bw = px(CL_RIGHT_R) - bar_x0;
                let z0 = bar_x0 + bw * f.shift_frac;
                let z1 = bar_x0 + bw * f.limit_frac.clamp(f.shift_frac, 1.0);
                if z1 - z0 > 1.5 {
                    self.b
                        .scratch
                        .SetColor(&color(C_RED.r, C_RED.g, C_RED.b, 0.30));
                    self.rt.FillRoundedRectangle(
                        &rounded(rectf(z0, rpm_y, z1, rpm_y + bar_h), bar_h / 2.0),
                        &self.b.scratch,
                    );
                }
                let tick = bar_h * 0.7;
                self.b.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.85));
                self.rt.DrawLine(
                    Vector2 {
                        X: z0,
                        Y: rpm_y - tick,
                    },
                    Vector2 {
                        X: z0,
                        Y: rpm_y + bar_h + tick,
                    },
                    &self.b.scratch,
                    1.4,
                    None,
                );
                if f.limit_known {
                    self.b
                        .scratch
                        .SetColor(&color(C_RED.r, C_RED.g, C_RED.b, 0.95));
                    self.rt.DrawLine(
                        Vector2 {
                            X: z1,
                            Y: rpm_y - tick,
                        },
                        Vector2 {
                            X: z1,
                            Y: rpm_y + bar_h + tick,
                        },
                        &self.b.scratch,
                        1.4,
                        None,
                    );
                }
            }
        }

        self.text(
            &f.car_name,
            &self.f.small_left,
            rectf(
                px(CL_RIGHT_X),
                py(CL_META_Y - 10.0),
                px(CL_RIGHT_R - 90.0),
                py(CL_META_Y + 24.0),
            ),
            &self.b.gray,
        );
        if f.show_power {
            let power = format!("{} hp", f.power_cv.round() as i32);
            self.text(
                &power,
                &self.f.small_right,
                rectf(
                    px(CL_RIGHT_R - 130.0),
                    py(CL_META_Y - 10.0),
                    px(CL_RIGHT_R),
                    py(CL_META_Y + 10.0),
                ),
                &self.b.white,
            );
        }

        let sp = (f.speed.round() as i32).to_string();
        self.text(
            &sp,
            &self.f.speed,
            rectf(
                px(CL_RIGHT_X),
                py(CL_SPEED_Y - 16.0),
                px(CL_RIGHT_X + 120.0),
                py(CL_SPEED_Y + 16.0),
            ),
            &self.b.white,
        );
        self.text(
            "km/h",
            &self.f.small_left,
            rectf(
                px(CL_RIGHT_X + 122.0),
                py(CL_SPEED_Y - 8.0),
                px(CL_RIGHT_X + 170.0),
                py(CL_SPEED_Y + 12.0),
            ),
            &self.b.gray,
        );

        self.pedal_row_right("BRAKE", f.brake, &self.b.brake, py(CL_BRAKE_Y), w);
    }
}
