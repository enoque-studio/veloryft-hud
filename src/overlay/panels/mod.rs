//! The HUD panels.
//!
//! [`Panel`] owns one layered window, its Direct2D render target, and the DIB it
//! presents. Its `draw` method dispatches to the per-panel drawing code, which
//! lives in the sibling modules so that no single file owns the layout of every
//! panel:
//!
//! * [`cluster`] — RPM, gear, speed, pedals
//! * [`tires`] — per-corner temperature and session range
//! * [`g_force`] — lateral and longitudinal g scatter
//! * [`elevation`] — the lap elevation profile
//!
//! [`throttle`] is deliberately separate. The critical throttle bar has its own
//! surface and its own high-priority thread, so it keeps responding at 125 Hz
//! even when the main loop is busy; nothing here rate-limits it.
//!
//! [`toolbar`] and [`backdrop`] are edit-mode chrome, not telemetry panels.

pub(crate) mod backdrop;
pub(crate) mod cluster;
pub(crate) mod elevation;
pub(crate) mod g_force;
pub(crate) mod throttle;
pub(crate) mod tires;
pub(crate) mod toolbar;

use std::cell::Cell;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::Matrix3x2;

use super::geometry::{glass_radius, panel_canvas, rectf, rounded, Kind, MIN_SZ};
use super::graphics::{make_dcrt, Brushes, Fonts};
use super::theme::{color, C_EDIT, THEME};
use super::window::{bring_to_top, create_dib_bitmap, make_dib, make_window, present};
use crate::telemetry::display::{self, DisplayFrame};
use crate::telemetry::elevation::ElevationProfile;

pub(crate) struct Panel {
    pub kind: Kind,
    pub hwnd: HWND,
    pub memdc: HDC,
    pub bitmap: HBITMAP,
    pub pos: POINT,
    pub size: SIZE,
    pub design: SIZE,
    pub factory: ID2D1Factory,
    pub dcrt: ID2D1DCRenderTarget,
    pub rt: ID2D1RenderTarget,
    pub b: Brushes,
    pub f: Fonts,
    pub enabled: bool,
    pub shown: Cell<bool>,
    pub paint_key: Cell<u64>,
}

impl Panel {
    // Window creation genuinely needs every one of these; bundling them into a
    // builder would only move the same list one call further away.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: Kind,
        class: PCWSTR,
        instance: HINSTANCE,
        factory: &ID2D1Factory,
        fonts: &Fonts,
        pos: (i32, i32),
        size: (i32, i32),
        enabled: bool,
    ) -> Result<Self> {
        let (dw, dh) = kind.size();
        // Preserve the design aspect ratio. Uniform scaling fills without a
        // margin and repairs saved sizes with the wrong proportions.
        let min_scale = MIN_SZ as f32 / (dw.min(dh) as f32);
        let scale = (size.0 as f32 / dw as f32)
            .max(size.1 as f32 / dh as f32)
            .max(min_scale);
        let (w, h) = (
            (dw as f32 * scale).round() as i32,
            (dh as f32 * scale).round() as i32,
        );
        let hwnd = make_window(class, instance, pos.0, pos.1, w, h)?;
        // SAFETY: hwnd was just created by make_window and is owned by this
        // Panel. GWLP_USERDATA on our own window class is free for our use, and
        // the packed value is read back only by window::design_of.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ((dw as isize) << 16) | (dh as isize));
        }
        let (memdc, bitmap) = make_dib(w, h)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        // ClearType does not work over per-pixel alpha in a layered window;
        // grayscale antialiasing stays crisp over every background.
        unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
        let b = Brushes::new(&rt, dh as f32)?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        Ok(Self {
            kind,
            hwnd,
            memdc,
            bitmap,
            pos: POINT { x: pos.0, y: pos.1 },
            size: SIZE { cx: w, cy: h },
            design: SIZE { cx: dw, cy: dh },
            factory: factory.clone(),
            dcrt,
            rt,
            b,
            f: fonts.clone(),
            enabled,
            shown: Cell::new(false),
            paint_key: Cell::new(0),
        })
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.paint_key.set(0);
        if !enabled {
            self.set_shown(false);
        }
    }

    pub fn set_shown(&self, show: bool) {
        if self.shown.get() == show {
            return;
        }
        self.shown.set(show);
        unsafe {
            if show {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                bring_to_top(self.hwnd);
            } else {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    /// Recreate the DIB after the window is resized.
    pub fn resize_dib(&mut self, w: i32, h: i32) -> Result<()> {
        let new_bmp = create_dib_bitmap(self.memdc, w, h)?;
        unsafe {
            let old = SelectObject(self.memdc, HGDIOBJ(new_bmp.0));
            let _ = DeleteObject(old);
        }
        self.bitmap = new_bmp;
        self.size = SIZE { cx: w, cy: h };
        Ok(())
    }

    pub fn text(
        &self,
        s: &str,
        fmt: &IDWriteTextFormat,
        r: D2D_RECT_F,
        brush: &ID2D1SolidColorBrush,
    ) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.rt.DrawText(
                &wide,
                fmt,
                &r,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Card surface: dark glass in one fill, without a border or glow; contrast
    /// supplies the outline. `shift` colors the whole card with the theme, while
    /// `edit` adds a functional cyan selection outline only in edit mode.
    pub fn draw_card(
        &self,
        w: f32,
        h: f32,
        radius: f32,
        edit: bool,
        shift: bool,
        accent: D2D1_COLOR_F,
    ) {
        let card = rounded(rectf(0.0, 0.0, w, h), radius);
        unsafe {
            if shift {
                // Card inteiro na cor do tema no momento do shift.
                let mut fill = accent;
                fill.a = 0.62;
                self.b.scratch.SetColor(&fill);
                self.rt.FillRoundedRectangle(&card, &self.b.scratch);
            } else if let Ok(glass) = self.b.glass(&self.rt, THEME.get()) {
                self.rt.FillRoundedRectangle(&card, &glass);
            }
            if edit {
                self.b.scratch.SetColor(&C_EDIT);
                self.rt.DrawRoundedRectangle(
                    &rounded(rectf(1.0, 1.0, w - 1.0, h - 1.0), radius),
                    &self.b.scratch,
                    2.0,
                    None,
                );
            }
        }
    }

    /// Return compositor time when a new bitmap was presented.
    pub fn draw(
        &self,
        f: &DisplayFrame,
        elev: &ElevationProfile,
        edit: bool,
        accent: D2D1_COLOR_F,
        gear_accent: bool,
        allow_present: bool,
    ) -> Result<Option<Duration>> {
        if !self.enabled {
            self.set_shown(false);
            self.paint_key.set(0);
            return Ok(None);
        }
        let visible = edit || f.alpha > 0.01;
        self.set_shown(visible);
        if !visible {
            self.paint_key.set(0);
            return Ok(None);
        }

        let theme = THEME.get();
        let accent_bits = theme.hue.to_bits()
            ^ theme.glass_alpha.to_bits().rotate_left(11)
            ^ theme.glass_tone.to_bits().rotate_left(22)
            ^ if gear_accent { 1 } else { 0 };
        let elev_opt = if matches!(self.kind, Kind::Elevation) {
            Some(elev)
        } else {
            None
        };
        let key = display::panel_paint_key(self.kind.panel_kind(), f, edit, accent_bits, elev_opt);
        // Animation is global but the key is panel-specific. A pedal animating
        // in the cluster must not force tires, G-force, or elevation to repaint.
        if !edit && key == self.paint_key.get() {
            return Ok(None);
        }
        if !allow_present {
            return Ok(None);
        }
        self.paint_key.set(key);

        let (sw, sh) = (self.size.cx as f32, self.size.cy as f32);
        let (dw, dh) = (self.design.cx as f32, self.design.cy as f32);
        let (cw, ch, s) = panel_canvas(sw, sh, dw, dh);
        let radius = glass_radius(cw, ch);
        let content = Matrix3x2 {
            M11: s,
            M12: 0.0,
            M21: 0.0,
            M22: s,
            M31: 0.0,
            M32: 0.0,
        };
        let layer_alpha = if edit { 1.0 } else { f.alpha.clamp(0.0, 1.0) };
        let shift = f.near && matches!(self.kind, Kind::Cluster);
        let card_shift = shift && !gear_accent;
        unsafe {
            let full = RECT {
                left: 0,
                top: 0,
                right: self.size.cx,
                bottom: self.size.cy,
            };
            self.dcrt.BindDC(self.memdc, &full)?;
            self.rt.BeginDraw();
            self.rt.Clear(Some(&color(0.0, 0.0, 0.0, 0.0)));

            if edit {
                self.rt.SetTransform(&Matrix3x2 {
                    M11: 1.0,
                    M12: 0.0,
                    M21: 0.0,
                    M22: 1.0,
                    M31: 0.0,
                    M32: 0.0,
                });
                self.b.scratch.SetColor(&color(0.55, 0.6, 0.7, 0.06));
                self.rt
                    .FillRectangle(&rectf(0.0, 0.0, sw, sh), &self.b.scratch);
            }

            self.rt.SetTransform(&content);
            self.draw_card(cw, ch, radius, edit, card_shift, accent);
            match self.kind {
                Kind::Cluster => self.draw_cluster(f, accent, gear_accent, cw, ch),
                Kind::Tires => self.draw_tires(f, cw, ch),
                Kind::GForce => self.draw_gforce(f, cw, ch),
                Kind::Elevation => self.draw_elevation(f, elev, cw, ch),
            }

            self.rt.EndDraw(None, None)?;
        }
        let alpha = if edit {
            255
        } else {
            (layer_alpha * 255.0).round() as u8
        };
        let present_started = Instant::now();
        present(self.hwnd, self.memdc, self.pos, self.size, alpha)?;
        Ok(Some(present_started.elapsed()))
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memdc);
        }
    }
}
