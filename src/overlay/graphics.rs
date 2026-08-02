//! Direct2D, DirectWrite, and GDI resources.
//!
//! Owns everything expensive enough that it must be created once and reused:
//! text formats, solid-color brushes, the cached glass gradient, and the DC
//! render targets the panels draw into.
//!
//! The glass gradient is keyed on the theme values that affect it
//! ([`glass_key`]) and rebuilt only when that key changes, because building a
//! gradient stop collection every frame would show up in the present time the
//! [`super::frame_pacing`] governor watches.

use std::cell::RefCell;

use windows::core::w;
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use windows::core::PCWSTR;
use windows_numerics::Vector2;

use crate::localization::text as localized_text;

use super::geometry::{CL_RIGHT_R, CL_RIGHT_X};
use super::theme::{
    glass_colors, Theme, C_AMBER, C_GRAY, C_GREEN, C_RED, C_TRACK, C_TRACK_EDGE, C_WHITE, THEME,
};

#[derive(Clone)]
pub(crate) struct Fonts {
    pub gear: IDWriteTextFormat,
    pub speed: IDWriteTextFormat,
    pub tire: IDWriteTextFormat,
    pub micro: IDWriteTextFormat,
    pub small: IDWriteTextFormat,
    pub small_left: IDWriteTextFormat,
    pub small_right: IDWriteTextFormat,
    pub toolbar_button: IDWriteTextFormat,
    pub toolbar_label: IDWriteTextFormat,
    pub toolbar_caption: IDWriteTextFormat,
}

pub(crate) struct Brushes {
    pub track: ID2D1SolidColorBrush,
    pub track_edge: ID2D1SolidColorBrush,
    pub accel: ID2D1SolidColorBrush,
    pub brake: ID2D1SolidColorBrush,
    pub white: ID2D1SolidColorBrush,
    pub gray: ID2D1SolidColorBrush,
    pub scratch: ID2D1SolidColorBrush,
    pub grad: ID2D1LinearGradientBrush,
    /// Card background (vertical gradient), rebuilt when opacity or tint changes.
    pub glass: RefCell<GlassCache>,
    pub design_h: f32,
}

pub(crate) struct GlassCache {
    pub key: u64,
    pub brush: ID2D1LinearGradientBrush,
}

pub(crate) fn glass_key(t: Theme) -> u64 {
    ((t.glass_alpha.to_bits() as u64) << 32) | t.glass_tone.to_bits() as u64
}

pub(crate) fn make_glass_brush(
    rt: &ID2D1RenderTarget,
    t: Theme,
    design_h: f32,
) -> Result<ID2D1LinearGradientBrush> {
    let (top, bot) = glass_colors(t);
    unsafe {
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: top,
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: bot,
            },
        ];
        let coll =
            rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
        rt.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: 0.0, Y: 0.0 },
                endPoint: Vector2 {
                    X: 0.0,
                    Y: design_h,
                },
            },
            None,
            &coll,
        )
    }
}

impl Brushes {
    /// Glass brush for the current theme, reusing the cache while opacity and tint match.
    pub fn glass(&self, rt: &ID2D1RenderTarget, t: Theme) -> Result<ID2D1LinearGradientBrush> {
        let key = glass_key(t);
        {
            let g = self.glass.borrow();
            if g.key == key {
                return Ok(g.brush.clone());
            }
        }
        let brush = make_glass_brush(rt, t, self.design_h)?;
        *self.glass.borrow_mut() = GlassCache {
            key,
            brush: brush.clone(),
        };
        Ok(brush)
    }

    pub fn new(rt: &ID2D1RenderTarget, design_h: f32) -> Result<Self> {
        unsafe {
            let track = rt.CreateSolidColorBrush(&C_TRACK, None)?;
            let track_edge = rt.CreateSolidColorBrush(&C_TRACK_EDGE, None)?;
            let accel = rt.CreateSolidColorBrush(&C_GREEN, None)?;
            let brake = rt.CreateSolidColorBrush(&C_RED, None)?;
            let white = rt.CreateSolidColorBrush(&C_WHITE, None)?;
            let gray = rt.CreateSolidColorBrush(&C_GRAY, None)?;
            let scratch = rt.CreateSolidColorBrush(&C_WHITE, None)?;
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: C_GREEN,
                },
                D2D1_GRADIENT_STOP {
                    position: 0.75,
                    color: C_AMBER,
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: C_RED,
                },
            ];
            let coll =
                rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let grad = rt.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 {
                        X: CL_RIGHT_X,
                        Y: 0.0,
                    },
                    endPoint: Vector2 {
                        X: CL_RIGHT_R,
                        Y: 0.0,
                    },
                },
                None,
                &coll,
            )?;
            let theme = THEME.get();
            let glass = RefCell::new(GlassCache {
                key: glass_key(theme),
                brush: make_glass_brush(rt, theme, design_h)?,
            });
            Ok(Self {
                track,
                track_edge,
                accel,
                brake,
                white,
                gray,
                scratch,
                grad,
                glass,
                design_h,
            })
        }
    }
}

pub(crate) fn make_fonts() -> Result<Fonts> {
    unsafe {
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let locale: Vec<u16> = localized_text()
            .language_tag
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let locale = PCWSTR(locale.as_ptr());
        let make = |family: PCWSTR,
                    size: f32,
                    weight: DWRITE_FONT_WEIGHT,
                    align: DWRITE_TEXT_ALIGNMENT,
                    para: DWRITE_PARAGRAPH_ALIGNMENT|
         -> Result<IDWriteTextFormat> {
            let fmt = dwrite.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                locale,
            )?;
            fmt.SetTextAlignment(align)?;
            fmt.SetParagraphAlignment(para)?;
            Ok(fmt)
        };
        // Segoe UI Variable optical cuts on Windows 11: Display for large
        // numbers and Text, designed for 9–14 px, for crisp small labels.
        let display = w!("Segoe UI Variable Display");
        let text = w!("Segoe UI Variable Text");
        Ok(Fonts {
            gear: make(
                display,
                176.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            speed: make(
                display,
                32.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            tire: make(
                display,
                26.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            micro: make(
                text,
                11.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            small: make(
                text,
                13.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            small_left: make(
                text,
                13.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?,
            small_right: make(
                text,
                13.0,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?,
            toolbar_button: make(
                text,
                12.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            toolbar_label: make(
                text,
                11.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            toolbar_caption: make(
                text,
                10.5,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
        })
    }
}

pub(crate) fn make_dcrt_with_type(
    factory: &ID2D1Factory,
    render_type: D2D1_RENDER_TARGET_TYPE,
) -> Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: render_type,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe { factory.CreateDCRenderTarget(&props) }
}

pub(crate) fn make_dcrt(factory: &ID2D1Factory) -> Result<ID2D1DCRenderTarget> {
    make_dcrt_with_type(factory, D2D1_RENDER_TARGET_TYPE_DEFAULT)
}
