//! The g-force panel: a lateral/longitudinal scatter with a decaying trail.

use windows::Win32::Graphics::Direct2D::*;
use windows_numerics::Vector2;

use super::Panel;
use crate::overlay::geometry::rectf;
use crate::telemetry::display::DisplayFrame;

impl Panel {
    pub(super) fn draw_gforce(&self, f: &DisplayFrame, w: f32, h: f32) {
        let head_h = h * 0.12;
        let foot_h = h * 0.14;
        let cxg = w * 0.5;
        let cyg = head_h + (h - head_h - foot_h) * 0.5;
        let r = w.min(h - head_h - foot_h) * 0.36;
        let dot_r = (r * 0.09).clamp(5.0, 9.0);

        self.text(
            "G-FORCE",
            &self.f.small,
            rectf(0.0, head_h * 0.25, w, head_h),
            &self.b.gray,
        );
        // G sentida pelo PILOTO = reação inercial, oposta à aceleração do carro.
        // Inverte só o lateral; o longitudinal já fica certo com o Y da tela p/ baixo.
        let g_lat = -f.lat;
        let g_long = f.long;
        unsafe {
            let outer = D2D1_ELLIPSE {
                point: Vector2 { X: cxg, Y: cyg },
                radiusX: r,
                radiusY: r,
            };
            self.rt.DrawEllipse(&outer, &self.b.gray, 1.5, None);
            let inner = D2D1_ELLIPSE {
                point: Vector2 { X: cxg, Y: cyg },
                radiusX: r * 0.5,
                radiusY: r * 0.5,
            };
            self.rt.DrawEllipse(&inner, &self.b.track, 1.0, None);
            self.rt.DrawLine(
                Vector2 { X: cxg - r, Y: cyg },
                Vector2 { X: cxg + r, Y: cyg },
                &self.b.track,
                1.0,
                None,
            );
            self.rt.DrawLine(
                Vector2 { X: cxg, Y: cyg - r },
                Vector2 { X: cxg, Y: cyg + r },
                &self.b.track,
                1.0,
                None,
            );
            // Virar p/ a esquerda joga o piloto p/ a direita; frear → topo; acelerar → baixo.
            let scale = r / 1.5;
            let mut dx = g_lat * scale;
            let mut dy = g_long * scale;
            let mag = (dx * dx + dy * dy).sqrt();
            if mag > r {
                dx *= r / mag;
                dy *= r / mag;
            }
            let dot = D2D1_ELLIPSE {
                point: Vector2 {
                    X: cxg + dx,
                    Y: cyg + dy,
                },
                radiusX: dot_r,
                radiusY: dot_r,
            };
            self.rt.FillEllipse(&dot, &self.b.white);
        }
        let txt = format!("{:.1}g  {:.1}g", g_lat, g_long);
        self.text(
            &txt,
            &self.f.small,
            rectf(w * 0.06, h - foot_h, w * 0.94, h - foot_h * 0.25),
            &self.b.gray,
        );
    }
}
