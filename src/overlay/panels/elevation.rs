//! The elevation panel: the lap's elevation profile with a marker at the
//! driver's current position along it.
//!
//! The profile is closed-loop, built by `telemetry::elevation`; this module
//! renders the curve and places the marker.

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows_numerics::Vector2;

use super::Panel;
use crate::localization::{distance_km, elevation_m, text};
use crate::overlay::geometry::rectf;
use crate::overlay::theme::{color, C_ELEV_LINE, C_ELEV_MARKER};
use crate::telemetry::display::DisplayFrame;
use crate::telemetry::elevation::ElevationProfile;

impl Panel {
    pub(super) fn draw_elevation(&self, f: &DisplayFrame, elev: &ElevationProfile, w: f32, h: f32) {
        let messages = text();
        let pad = (w * 0.033).max(6.0);
        let head_h = (h * 0.13).max(22.0);
        let foot_h = (h * 0.12).max(18.0);
        let chart_top = head_h + h * 0.02;
        let chart_bot = h - foot_h - h * 0.02;
        let chart_w = w - pad * 2.0;
        let chart_h = (chart_bot - chart_top).max(20.0);

        self.text(
            messages.elevation,
            &self.f.small_left,
            rectf(pad, pad * 0.4, pad + w * 0.45, head_h),
            &self.b.gray,
        );
        let lap = if elev.loop_closed {
            distance_km(elev.loop_km)
        } else if elev.lap_km > 0.0 {
            distance_km(elev.lap_km)
        } else {
            messages.missing_value.to_string()
        };
        self.text(
            &lap,
            &self.f.small_right,
            rectf(w - pad - w * 0.38, pad * 0.4, w - pad, head_h),
            &self.b.white,
        );

        let d_max = if elev.loop_closed {
            elev.loop_km * 1000.0
        } else {
            elev.pts.last().map(|p| p.d).unwrap_or(1.0)
        }
        .max(1.0);
        let span = (elev.max - elev.min).max(1.0);

        let x_at = |d: f32| pad + (d / d_max) * chart_w;
        let y_at = |y: f32| chart_bot - ((y - elev.min) / span) * chart_h;

        unsafe {
            if elev.pts.len() >= 2 {
                let geom = self.factory.CreatePathGeometry().expect("path geom");
                let sink = geom.Open().expect("path sink");
                let first = &elev.pts[0];
                let x0 = x_at(first.d);
                sink.BeginFigure(
                    Vector2 {
                        X: x0,
                        Y: chart_bot,
                    },
                    D2D1_FIGURE_BEGIN_FILLED,
                );
                for p in &elev.pts {
                    sink.AddLine(Vector2 {
                        X: x_at(p.d),
                        Y: y_at(p.y),
                    });
                }
                let last = elev.pts.last().unwrap();
                sink.AddLine(Vector2 {
                    X: x_at(last.d),
                    Y: chart_bot,
                });
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                let _ = sink.Close();

                let grad_stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: color(1.0, 0.310, 0.541, 0.55),
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: color(0.176, 0.706, 1.0, 0.06),
                    },
                ];
                let coll = self
                    .rt
                    .CreateGradientStopCollection(
                        &grad_stops,
                        D2D1_GAMMA_2_2,
                        D2D1_EXTEND_MODE_CLAMP,
                    )
                    .expect("grad coll");
                let fill = self
                    .rt
                    .CreateLinearGradientBrush(
                        &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                            startPoint: Vector2 {
                                X: pad,
                                Y: chart_top,
                            },
                            endPoint: Vector2 {
                                X: pad,
                                Y: chart_bot,
                            },
                        },
                        None,
                        &coll,
                    )
                    .expect("grad brush");
                self.rt.FillGeometry(&geom, &fill, None);

                self.b.scratch.SetColor(&C_ELEV_LINE);
                let line_w = (w * 0.006).clamp(1.5, 3.0);
                for i in 1..elev.pts.len() {
                    let a = &elev.pts[i - 1];
                    let b = &elev.pts[i];
                    self.rt.DrawLine(
                        Vector2 {
                            X: x_at(a.d),
                            Y: y_at(a.y),
                        },
                        Vector2 {
                            X: x_at(b.d),
                            Y: y_at(b.y),
                        },
                        &self.b.scratch,
                        line_w,
                        None,
                    );
                }
            }

            if elev.pts.len() >= 2 {
                let cy = y_at(elev.marker_y);
                let cx = x_at(elev.marker_d.min(d_max));
                let dot_r = (w * 0.014).clamp(3.5, 6.0);
                let dot = D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: dot_r,
                    radiusY: dot_r,
                };
                self.rt.FillEllipse(&dot, &self.b.white);
                self.b.scratch.SetColor(&C_ELEV_MARKER);
                self.rt.DrawEllipse(&dot, &self.b.scratch, 1.5, None);
            }
        }

        let min_l = if elev.pts.len() >= 2 {
            elevation_m(elev.min)
        } else {
            messages.missing_value.to_string()
        };
        let max_l = if elev.pts.len() >= 2 {
            elevation_m(elev.max)
        } else {
            messages.missing_value.to_string()
        };
        let foot_y = h - foot_h + h * 0.01;
        let foot_bot = h - pad * 0.4;
        self.text(
            messages.minimum,
            &self.f.small_left,
            rectf(pad, foot_y, pad + w * 0.12, foot_bot),
            &self.b.gray,
        );
        self.text(
            &min_l,
            &self.f.small_left,
            rectf(pad + w * 0.1, foot_y, pad + w * 0.32, foot_bot),
            &self.b.white,
        );
        self.text(
            messages.maximum,
            &self.f.small_right,
            rectf(w - pad - w * 0.32, foot_y, w - pad - w * 0.2, foot_bot),
            &self.b.gray,
        );
        self.text(
            &max_l,
            &self.f.small_right,
            rectf(w - pad - w * 0.2, foot_y, w - pad, foot_bot),
            &self.b.white,
        );

        let cur = if f.fresh {
            elevation_m(f.elev_m)
        } else {
            messages.missing_value.to_string()
        };
        self.text(
            &cur,
            &self.f.speed,
            rectf(
                pad,
                chart_top + chart_h * 0.04,
                pad + w * 0.38,
                chart_top + chart_h * 0.22,
            ),
            &self.b.white,
        );
    }
}
