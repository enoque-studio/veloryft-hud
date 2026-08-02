//! The edit-mode toolbar: accent color, shift target, panel visibility, and the
//! glass opacity and tint sliders.
//!
//! The toolbar is the only interactive surface in the overlay, so it is also the
//! only one that drops `WS_EX_TRANSPARENT`. Its layout is a fixed pixel grid;
//! the `TB_*` constants below are the single source of truth for both drawing
//! and hit testing, which is why the two live in the same module.
//!
//! Hit testing returns an opaque `HIT_*` code rather than an enum so that
//! contiguous ranges (one per swatch, one per panel toggle) stay cheap to test.

use windows::core::Interface;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::Vector2;

use crate::overlay::geometry::{in_rect, rectf, rounded, PANEL_COUNT, PANEL_KINDS};
use crate::overlay::graphics::{make_dcrt, Fonts};
use crate::overlay::runtime::{PANEL_VISIBLE, TOOLBAR_DOWN};
use crate::overlay::settings;
use crate::overlay::theme::{
    color, hsv_color, hue_color, C_WHITE, GLASS_ALPHA_MAX, GLASS_ALPHA_MIN, THEME,
};
use crate::overlay::window::{
    bring_to_top, make_dib, make_window, present, set_interactive, window_rect,
};

pub(crate) const SWATCHES: [f32; 8] = [4.0, 30.0, 50.0, 140.0, 188.0, 218.0, 280.0, 320.0];
pub(crate) const TB_PAD: i32 = 14;
pub(crate) const TB_LABEL_W: i32 = 52;
pub(crate) const TB_SW: i32 = 26;
pub(crate) const TB_GAP: i32 = 8;
pub(crate) const TB_COLOR_X: i32 = TB_PAD + TB_LABEL_W;
pub(crate) const TB_SWATCH_Y: i32 = 38;
pub(crate) const TB_COLOR_W: i32 = 8 * TB_SW + 7 * TB_GAP;
pub(crate) const TB_TARGET_X: i32 = TB_COLOR_X + TB_COLOR_W + 16;
pub(crate) const TB_TARGET_W: i32 = 174;
pub(crate) const TB_TARGET_SEG_W: i32 = TB_TARGET_W / 2;
pub(crate) const TB_TARGET_H: i32 = 30;
pub(crate) const TB_TARGET_Y: i32 = TB_SWATCH_Y - 2;
pub(crate) const TB_PANEL_Y: i32 = 82;
pub(crate) const TB_PANEL_W: i32 = 78;
pub(crate) const TB_PANEL_H: i32 = 30;
pub(crate) const TB_PANEL_GAP: i32 = 6;
pub(crate) const TB_PANEL_X: i32 = TB_COLOR_X;
// Linha 3: sliders do vidro (opacidade + tom).
pub(crate) const TB_ROW3_Y: i32 = 122;
pub(crate) const TB_SLIDER_H: i32 = 26;
pub(crate) const TB_GA_X: i32 = TB_COLOR_X;
pub(crate) const TB_GA_W: i32 = 150;
pub(crate) const TB_GT_LABEL_X: i32 = TB_GA_X + TB_GA_W + 16;
pub(crate) const TB_GT_X: i32 = TB_GT_LABEL_X + 38;
pub(crate) const TB_GT_W: i32 = TB_W - TB_PAD - TB_GT_X;
/// Célula "neutro" no início do slider de tom.
pub(crate) const TONE_NEUTRAL_W: i32 = 22;
pub(crate) const TONE_GAP: i32 = 4;
pub(crate) const TB_H: i32 = 160;
pub(crate) const TB_W: i32 = TB_TARGET_X + TB_TARGET_W + TB_PAD;
pub(crate) const HIT_NONE: i32 = -1;
pub(crate) const HIT_SWATCH_BASE: i32 = 10;
pub(crate) const HIT_TARGET_CARD: i32 = 100;
pub(crate) const HIT_TARGET_GEAR: i32 = 101;
pub(crate) const HIT_PANEL_BASE: i32 = 200;
pub(crate) const HIT_GLASS_ALPHA: i32 = 300;
pub(crate) const HIT_GLASS_TONE: i32 = 301;

pub(crate) struct Toolbar {
    pub hwnd: HWND,
    pub memdc: HDC,
    pub bitmap: HBITMAP,
    pub pos: POINT,
    pub size: SIZE,
    pub dcrt: ID2D1DCRenderTarget,
    pub rt: ID2D1RenderTarget,
    pub bg: ID2D1SolidColorBrush,
    pub edge: ID2D1SolidColorBrush,
    pub text_brush: ID2D1SolidColorBrush,
    pub muted_brush: ID2D1SolidColorBrush,
    pub scratch: ID2D1SolidColorBrush,
    pub font_button: IDWriteTextFormat,
    pub font_label: IDWriteTextFormat,
    pub font_caption: IDWriteTextFormat,
}

impl Toolbar {
    pub fn new(
        class: PCWSTR,
        instance: HINSTANCE,
        factory: &ID2D1Factory,
        fonts: &Fonts,
        sw: i32,
    ) -> Result<Self> {
        let pos = POINT {
            x: (sw - TB_W) / 2,
            y: 24,
        };
        let size = SIZE { cx: TB_W, cy: TB_H };
        let hwnd = make_window(class, instance, pos.x, pos.y, size.cx, size.cy)?;
        set_interactive(hwnd, true);
        let (memdc, bitmap) = make_dib(size.cx, size.cy)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
        let bg = unsafe { rt.CreateSolidColorBrush(&color(0.048, 0.055, 0.071, 0.90), None)? };
        let edge = unsafe { rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 0.16), None)? };
        let text_brush = unsafe { rt.CreateSolidColorBrush(&C_WHITE, None)? };
        let muted_brush =
            unsafe { rt.CreateSolidColorBrush(&color(0.70, 0.74, 0.80, 0.82), None)? };
        let scratch = unsafe { rt.CreateSolidColorBrush(&C_WHITE, None)? };
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        Ok(Self {
            hwnd,
            memdc,
            bitmap,
            pos,
            size,
            dcrt,
            rt,
            bg,
            edge,
            text_brush,
            muted_brush,
            scratch,
            font_button: fonts.toolbar_button.clone(),
            font_label: fonts.toolbar_label.clone(),
            font_caption: fonts.toolbar_caption.clone(),
        })
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

    pub fn draw(&self) -> Result<()> {
        let t = THEME.get();
        let visible = PANEL_VISIBLE.get();
        let hover = toolbar_hover_hit(self.hwnd);
        let down = TOOLBAR_DOWN.with(|v| v.get());
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

            let card = rounded(
                rectf(
                    0.75,
                    0.75,
                    self.size.cx as f32 - 0.75,
                    self.size.cy as f32 - 0.75,
                ),
                8.0,
            );
            self.rt.FillRoundedRectangle(&card, &self.bg);
            self.rt.DrawRoundedRectangle(&card, &self.edge, 1.0, None);

            self.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.05));
            self.rt.FillRoundedRectangle(
                &rounded(rectf(2.0, 2.0, self.size.cx as f32 - 2.0, 28.0), 7.0),
                &self.scratch,
            );
            self.text(
                "EDITAR HUD",
                &self.font_caption,
                rectf(TB_PAD as f32, 4.0, 140.0, 28.0),
                &self.muted_brush,
            );
            self.text(
                "COR",
                &self.font_label,
                rectf(
                    TB_PAD as f32,
                    TB_SWATCH_Y as f32,
                    TB_COLOR_X as f32,
                    (TB_SWATCH_Y + TB_SW) as f32,
                ),
                &self.muted_brush,
            );

            for (i, hue) in SWATCHES.iter().enumerate() {
                let hit = HIT_SWATCH_BASE + i as i32;
                let pressed = down == hit && hover == hit;
                let offset = if pressed { 1.0 } else { 0.0 };
                let (l, tt, r, b) = swatch_rect(i);
                let swatch = rounded(
                    rectf(
                        l as f32 + offset,
                        tt as f32 + offset,
                        r as f32 + offset,
                        b as f32 + offset,
                    ),
                    6.0,
                );
                self.scratch.SetColor(&hue_color(*hue));
                self.rt.FillRoundedRectangle(&swatch, &self.scratch);
                if hover == hit {
                    self.scratch
                        .SetColor(&color(1.0, 1.0, 1.0, if pressed { 0.20 } else { 0.11 }));
                    self.rt.FillRoundedRectangle(&swatch, &self.scratch);
                }
                if (t.hue - *hue).abs() < 0.5 {
                    self.scratch.SetColor(&C_WHITE);
                    self.rt
                        .DrawRoundedRectangle(&swatch, &self.scratch, 2.0, None);
                } else {
                    self.rt.DrawRoundedRectangle(&swatch, &self.edge, 1.0, None);
                }
            }

            let (tl, tt, tr, tb) = target_rect();
            let toggle = rounded(rectf(tl as f32, tt as f32, tr as f32, tb as f32), 7.0);
            let mut accent = hue_color(t.hue);
            accent.a = 0.20;
            self.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.055));
            self.rt.FillRoundedRectangle(&toggle, &self.scratch);
            self.rt.DrawRoundedRectangle(&toggle, &self.edge, 1.0, None);

            if hover == HIT_TARGET_CARD || hover == HIT_TARGET_GEAR {
                let hover_l = if hover == HIT_TARGET_GEAR {
                    TB_TARGET_X + TB_TARGET_SEG_W
                } else {
                    TB_TARGET_X
                };
                let pressed = down == hover;
                self.scratch
                    .SetColor(&color(1.0, 1.0, 1.0, if pressed { 0.10 } else { 0.065 }));
                self.rt.FillRoundedRectangle(
                    &rounded(
                        rectf(
                            hover_l as f32 + 2.0,
                            TB_TARGET_Y as f32 + 2.0,
                            (hover_l + TB_TARGET_SEG_W) as f32 - 2.0,
                            (TB_TARGET_Y + TB_TARGET_H) as f32 - 2.0,
                        ),
                        5.0,
                    ),
                    &self.scratch,
                );
            }

            let active_l = if t.gear_target {
                TB_TARGET_X + TB_TARGET_SEG_W
            } else {
                TB_TARGET_X
            };
            let active_hit = if t.gear_target {
                HIT_TARGET_GEAR
            } else {
                HIT_TARGET_CARD
            };
            let active = rounded(
                rectf(
                    active_l as f32 + 2.0,
                    TB_TARGET_Y as f32 + 2.0,
                    (active_l + TB_TARGET_SEG_W) as f32 - 2.0,
                    (TB_TARGET_Y + TB_TARGET_H) as f32 - 2.0,
                ),
                5.0,
            );
            accent.a = if down == active_hit && hover == active_hit {
                0.34
            } else if hover == active_hit {
                0.27
            } else {
                0.20
            };
            self.scratch.SetColor(&accent);
            self.rt.FillRoundedRectangle(&active, &self.scratch);
            self.scratch.SetColor(&hue_color(t.hue));
            self.rt
                .DrawRoundedRectangle(&active, &self.scratch, 1.0, None);
            self.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.10));
            self.rt.DrawLine(
                Vector2 {
                    X: (TB_TARGET_X + TB_TARGET_SEG_W) as f32,
                    Y: TB_TARGET_Y as f32 + 6.0,
                },
                Vector2 {
                    X: (TB_TARGET_X + TB_TARGET_SEG_W) as f32,
                    Y: (TB_TARGET_Y + TB_TARGET_H) as f32 - 6.0,
                },
                &self.scratch,
                1.0,
                None,
            );
            self.text(
                "CARD",
                &self.font_button,
                rectf(
                    TB_TARGET_X as f32,
                    TB_TARGET_Y as f32,
                    (TB_TARGET_X + TB_TARGET_SEG_W) as f32,
                    (TB_TARGET_Y + TB_TARGET_H) as f32,
                ),
                if t.gear_target {
                    &self.muted_brush
                } else {
                    &self.text_brush
                },
            );
            self.text(
                "MARCHA",
                &self.font_button,
                rectf(
                    (TB_TARGET_X + TB_TARGET_SEG_W) as f32,
                    TB_TARGET_Y as f32,
                    (TB_TARGET_X + TB_TARGET_W) as f32,
                    (TB_TARGET_Y + TB_TARGET_H) as f32,
                ),
                if t.gear_target {
                    &self.text_brush
                } else {
                    &self.muted_brush
                },
            );

            self.text(
                "CARDS",
                &self.font_label,
                rectf(
                    TB_PAD as f32,
                    TB_PANEL_Y as f32,
                    TB_COLOR_X as f32,
                    (TB_PANEL_Y + TB_PANEL_H) as f32,
                ),
                &self.muted_brush,
            );
            for (i, kind) in PANEL_KINDS.iter().enumerate() {
                let on = visible[i];
                let hit = HIT_PANEL_BASE + i as i32;
                let hot = hover == hit;
                let pressed = down == hit && hot;
                let offset = if pressed { 1.0 } else { 0.0 };
                let (l, tt, r, b) = panel_toggle_rect(i);
                let pill = rounded(
                    rectf(
                        l as f32 + 0.5,
                        tt as f32 + offset,
                        r as f32 - 0.5,
                        b as f32 + offset,
                    ),
                    7.0,
                );
                let mut fill = if on {
                    let mut c = hue_color(t.hue);
                    c.a = if pressed {
                        0.30
                    } else if hot {
                        0.24
                    } else {
                        0.17
                    };
                    c
                } else {
                    color(
                        1.0,
                        1.0,
                        1.0,
                        if pressed {
                            0.095
                        } else if hot {
                            0.075
                        } else {
                            0.045
                        },
                    )
                };
                self.scratch.SetColor(&fill);
                self.rt.FillRoundedRectangle(&pill, &self.scratch);
                if on || hot {
                    fill = if on {
                        hue_color(t.hue)
                    } else {
                        color(1.0, 1.0, 1.0, 0.28)
                    };
                    self.scratch.SetColor(&fill);
                    self.rt.DrawRoundedRectangle(
                        &pill,
                        &self.scratch,
                        if hot { 1.35 } else { 1.0 },
                        None,
                    );
                } else {
                    self.rt.DrawRoundedRectangle(&pill, &self.edge, 1.0, None);
                }

                if on && hot {
                    self.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.08));
                    self.rt.FillRoundedRectangle(&pill, &self.scratch);
                    self.scratch.SetColor(&hue_color(t.hue));
                    self.rt
                        .DrawRoundedRectangle(&pill, &self.scratch, 1.0, None);
                }

                self.text(
                    kind.short_label(),
                    &self.font_button,
                    rectf(l as f32, tt as f32 + offset, r as f32, b as f32 + offset),
                    if on || hot {
                        &self.text_brush
                    } else {
                        &self.muted_brush
                    },
                );
            }

            // ---- Linha 3: vidro (opacidade + tom) ----
            self.text(
                "VIDRO",
                &self.font_label,
                rectf(
                    TB_PAD as f32,
                    TB_ROW3_Y as f32,
                    TB_COLOR_X as f32,
                    (TB_ROW3_Y + TB_SLIDER_H) as f32,
                ),
                &self.muted_brush,
            );
            let knob = |x: f32, y: f32| D2D1_ELLIPSE {
                point: Vector2 { X: x, Y: y },
                radiusX: 7.0,
                radiusY: 7.0,
            };
            let cy = TB_ROW3_Y as f32 + TB_SLIDER_H as f32 / 2.0;

            // Opacidade: trilho + preenchimento até o knob.
            let (gal, _, gar, _) = glass_alpha_rect();
            let ga_track = rounded(rectf(gal as f32, cy - 3.0, gar as f32, cy + 3.0), 3.0);
            self.scratch.SetColor(&color(1.0, 1.0, 1.0, 0.10));
            self.rt.FillRoundedRectangle(&ga_track, &self.scratch);
            let kx = alpha_knob_x(t.glass_alpha);
            let mut acc = hue_color(t.hue);
            acc.a = 0.60;
            self.scratch.SetColor(&acc);
            self.rt.FillRoundedRectangle(
                &rounded(rectf(gal as f32, cy - 3.0, kx, cy + 3.0), 3.0),
                &self.scratch,
            );
            self.scratch.SetColor(&C_WHITE);
            self.rt.FillEllipse(&knob(kx, cy), &self.scratch);
            self.scratch.SetColor(&color(0.0, 0.0, 0.0, 0.35));
            self.rt.DrawEllipse(&knob(kx, cy), &self.scratch, 1.0, None);

            self.text(
                "TOM",
                &self.font_label,
                rectf(
                    TB_GT_LABEL_X as f32,
                    TB_ROW3_Y as f32,
                    TB_GT_X as f32,
                    (TB_ROW3_Y + TB_SLIDER_H) as f32,
                ),
                &self.muted_brush,
            );
            // Célula "neutro" + faixa de matiz.
            let neutral = rounded(
                rectf(
                    TB_GT_X as f32,
                    cy - 3.0,
                    (TB_GT_X + TONE_NEUTRAL_W) as f32,
                    cy + 3.0,
                ),
                3.0,
            );
            self.scratch.SetColor(&color(0.62, 0.66, 0.72, 0.38));
            self.rt.FillRoundedRectangle(&neutral, &self.scratch);
            let sx = tone_strip_x() as f32;
            let sw = (TB_GT_X + TB_GT_W) as f32 - sx;
            const TONE_SEGS: i32 = 24;
            for i in 0..TONE_SEGS {
                let x0 = sx + sw * i as f32 / TONE_SEGS as f32;
                let x1 = sx + sw * (i + 1) as f32 / TONE_SEGS as f32;
                let h = (i as f32 + 0.5) / TONE_SEGS as f32 * 360.0;
                self.scratch.SetColor(&hsv_color(h, 0.72, 0.85, 0.90));
                self.rt
                    .FillRectangle(&rectf(x0, cy - 3.0, x1, cy + 3.0), &self.scratch);
            }
            let tx = tone_knob_x(t.glass_tone);
            self.scratch.SetColor(&C_WHITE);
            self.rt.FillEllipse(&knob(tx, cy), &self.scratch);
            if t.glass_tone >= 0.0 {
                self.scratch
                    .SetColor(&hsv_color(t.glass_tone, 0.72, 0.85, 1.0));
                self.rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2 { X: tx, Y: cy },
                        radiusX: 4.5,
                        radiusY: 4.5,
                    },
                    &self.scratch,
                );
            }
            self.scratch.SetColor(&color(0.0, 0.0, 0.0, 0.35));
            self.rt.DrawEllipse(&knob(tx, cy), &self.scratch, 1.0, None);

            self.rt.EndDraw(None, None)?;
        }
        present(self.hwnd, self.memdc, self.pos, self.size, 255)
    }

    pub fn show(&self) {
        let _ = self.draw();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        bring_to_top(self.hwnd);
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

impl Drop for Toolbar {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memdc);
        }
    }
}

pub(crate) fn swatch_rect(i: usize) -> (i32, i32, i32, i32) {
    let x = TB_COLOR_X + i as i32 * (TB_SW + TB_GAP);
    (x, TB_SWATCH_Y, x + TB_SW, TB_SWATCH_Y + TB_SW)
}

pub(crate) fn target_rect() -> (i32, i32, i32, i32) {
    (
        TB_TARGET_X,
        TB_TARGET_Y,
        TB_TARGET_X + TB_TARGET_W,
        TB_TARGET_Y + TB_TARGET_H,
    )
}

pub(crate) fn panel_toggle_rect(i: usize) -> (i32, i32, i32, i32) {
    let x = TB_PANEL_X + i as i32 * (TB_PANEL_W + TB_PANEL_GAP);
    (x, TB_PANEL_Y, x + TB_PANEL_W, TB_PANEL_Y + TB_PANEL_H)
}

pub(crate) fn glass_alpha_rect() -> (i32, i32, i32, i32) {
    (
        TB_GA_X,
        TB_ROW3_Y,
        TB_GA_X + TB_GA_W,
        TB_ROW3_Y + TB_SLIDER_H,
    )
}

pub(crate) fn glass_tone_rect() -> (i32, i32, i32, i32) {
    (
        TB_GT_X,
        TB_ROW3_Y,
        TB_GT_X + TB_GT_W,
        TB_ROW3_Y + TB_SLIDER_H,
    )
}

/// Início da faixa de matiz (depois da célula "neutro") no slider de tom.
pub(crate) fn tone_strip_x() -> i32 {
    TB_GT_X + TONE_NEUTRAL_W + TONE_GAP
}

pub(crate) fn alpha_from_x(x: i32) -> f32 {
    let f = ((x - TB_GA_X) as f32 / TB_GA_W as f32).clamp(0.0, 1.0);
    GLASS_ALPHA_MIN + f * (GLASS_ALPHA_MAX - GLASS_ALPHA_MIN)
}

pub(crate) fn alpha_knob_x(a: f32) -> f32 {
    let f = ((a - GLASS_ALPHA_MIN) / (GLASS_ALPHA_MAX - GLASS_ALPHA_MIN)).clamp(0.0, 1.0);
    TB_GA_X as f32 + f * TB_GA_W as f32
}

pub(crate) fn tone_from_x(x: i32) -> f32 {
    let sx = tone_strip_x();
    if x < sx - TONE_GAP / 2 {
        return -1.0;
    }
    ((x - sx) as f32 / (TB_GT_X + TB_GT_W - sx) as f32).clamp(0.0, 1.0) * 360.0
}

pub(crate) fn tone_knob_x(tone: f32) -> f32 {
    if tone < 0.0 {
        (TB_GT_X + TONE_NEUTRAL_W / 2) as f32
    } else {
        let sx = tone_strip_x();
        sx as f32 + (tone / 360.0).clamp(0.0, 1.0) * (TB_GT_X + TB_GT_W - sx) as f32
    }
}

pub(crate) fn toolbar_hit(x: i32, y: i32) -> i32 {
    for i in 0..SWATCHES.len() {
        if in_rect(x, y, swatch_rect(i)) {
            return HIT_SWATCH_BASE + i as i32;
        }
    }

    if in_rect(x, y, target_rect()) {
        return if x >= TB_TARGET_X + TB_TARGET_SEG_W {
            HIT_TARGET_GEAR
        } else {
            HIT_TARGET_CARD
        };
    }

    for i in 0..PANEL_COUNT {
        if in_rect(x, y, panel_toggle_rect(i)) {
            return HIT_PANEL_BASE + i as i32;
        }
    }

    if in_rect(x, y, glass_alpha_rect()) {
        return HIT_GLASS_ALPHA;
    }
    if in_rect(x, y, glass_tone_rect()) {
        return HIT_GLASS_TONE;
    }

    HIT_NONE
}

/// Aplica o arrasto de um slider do vidro (chamado no down e no move).
pub(crate) fn toolbar_slider_drag(hit: i32, x: i32) {
    let mut t = THEME.get();
    match hit {
        HIT_GLASS_ALPHA => t.glass_alpha = alpha_from_x(x),
        HIT_GLASS_TONE => t.glass_tone = tone_from_x(x),
        _ => return,
    }
    THEME.set(t);
}

pub(crate) fn toolbar_hover_hit(hwnd: HWND) -> i32 {
    unsafe {
        let mut m = POINT::default();
        let _ = GetCursorPos(&mut m);
        let r = window_rect(hwnd);
        if m.x < r.left || m.x >= r.right || m.y < r.top || m.y >= r.bottom {
            return HIT_NONE;
        }
        toolbar_hit(m.x - r.left, m.y - r.top)
    }
}

pub(crate) fn toolbar_activate(hit: i32) {
    let mut t = THEME.get();
    if (HIT_SWATCH_BASE..HIT_SWATCH_BASE + SWATCHES.len() as i32).contains(&hit) {
        if let Some(hue) = SWATCHES.get((hit - HIT_SWATCH_BASE) as usize) {
            t.hue = *hue;
            THEME.set(t);
            settings::save_theme();
        }
        return;
    }

    if hit == HIT_TARGET_CARD || hit == HIT_TARGET_GEAR {
        t.gear_target = hit == HIT_TARGET_GEAR;
        THEME.set(t);
        settings::save_theme();
        return;
    }

    if (HIT_PANEL_BASE..HIT_PANEL_BASE + PANEL_COUNT as i32).contains(&hit) {
        let i = (hit - HIT_PANEL_BASE) as usize;
        let mut visible = PANEL_VISIBLE.get();
        visible[i] = !visible[i];
        PANEL_VISIBLE.set(visible);
    }
}
