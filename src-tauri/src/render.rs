//! Overlay nativo: cada painel é uma janela layered própria (pequena,
//! click-through, alpha por pixel via `UpdateLayeredWindow`). Modo edição
//! (Ctrl+Alt+E) mostra uma malha na tela e torna os painéis arrastáveis;
//! posições são salvas e recarregadas. Ctrl+Alt+Q sai.

#![allow(clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::{Matrix3x2, Vector2};

use crate::forza::display::{self, DisplayFrame, DisplaySmoother, PanelKind};
use crate::forza::elevation::ElevationProfile;
use crate::forza::state::Shared;
use crate::forza::tires::temp_color;
use crate::forza::udp;
use crate::forza::visibility;
use crate::forza::worldmap::{self, WorldMapRecorder};
use crate::forza::TRACK_MAPPING_ENABLED;

const UDP_PORT: u16 = 20066;
const HK_QUIT: i32 = 1;
const HK_EDIT: i32 = 2;
const HK_HIDE: i32 = 3;
const GRID: i32 = 40;
const RESIZE_MARGIN: i32 = 8;
const MIN_SZ: i32 = 80;
const MAGNET: i32 = 12;
const FRAME_MIN_MS: u64 = 16;
const IDLE_SLEEP_MS: u64 = 50;

static EDIT: AtomicBool = AtomicBool::new(false);
static HUD_HIDDEN: AtomicBool = AtomicBool::new(false);

// Tema editável (thread da UI): matiz do destaque do shift, se ele colore a
// marcha (true) ou o card (false), opacidade e tom do vidro dos cards.
// Roda do mouse muda a cor; botão direito alterna o alvo do shift.
#[derive(Clone, Copy)]
struct Theme {
    hue: f32,
    gear_target: bool,
    /// Opacidade do vidro (alpha do topo; a base escala junto).
    glass_alpha: f32,
    /// Tom do vidro: negativo = neutro escuro; senão matiz 0..360.
    glass_tone: f32,
}

const THEME_DEFAULT: Theme = Theme {
    hue: 4.0,
    gear_target: false,
    glass_alpha: 0.62,
    glass_tone: -1.0,
};

thread_local! {
    static THEME: Cell<Theme> = const { Cell::new(THEME_DEFAULT) };
    static PANEL_VISIBLE: Cell<[bool; PANEL_COUNT]> = const { Cell::new([true; PANEL_COUNT]) };
    static TOOLBAR_DOWN: Cell<i32> = const { Cell::new(HIT_NONE) };
}

/// HSV→RGB genérico.
fn hsv_color(h: f32, s: f32, v: f32, a: f32) -> D2D1_COLOR_F {
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

/// Matiz do destaque (s/v fixos, cor viva).
fn hue_color(h: f32) -> D2D1_COLOR_F {
    hsv_color(h, 0.82, 0.96, 1.0)
}

// Move/resize manual (thread da UI). Resize escala a partir do canto fixo
// (`anchor`) em direção ao cursor, mantendo a proporção do design.
#[derive(Clone, Copy)]
struct Grab {
    win0: RECT,
    mouse0: POINT,
    move_mode: bool,
    anchor: POINT,     // canto fixo no resize
    h_active: bool,    // largura varia (bordas/cantos horizontais)
    v_active: bool,    // altura varia
    from_right: bool,  // anchor na direita → card cresce p/ esquerda
    from_bottom: bool, // anchor embaixo → card cresce p/ cima
}
thread_local! {
    static GRAB: Cell<Option<Grab>> = const { Cell::new(None) };
}

/// Zona sob o cursor: centro = mover; bordas/cantos = redimensionar.
#[derive(Clone, Copy, PartialEq)]
enum Zone {
    Move,
    L,
    R,
    T,
    B,
    Tl,
    Tr,
    Bl,
    Br,
}

// --- Cluster (layout local) ---
const CL_W: i32 = 680;
const CL_H: i32 = 240;
const CL_BAR_X: f32 = 20.0;
const CL_BAR_W: f32 = 640.0;
const CL_BAR_H: f32 = 6.0;
const CL_PEDAL_H: f32 = 5.0;
const CL_PAD: f32 = 8.0;
const CL_GEAR_W: f32 = 160.0;
const CL_GEAR_GAP: f32 = 14.0;
const CL_GEAR_Y_SHIFT: f32 = 14.0;
const CL_RIGHT_X: f32 = CL_BAR_X + CL_GEAR_W + CL_GEAR_GAP;
const CL_RIGHT_R: f32 = CL_BAR_X + CL_BAR_W;
const CL_RPM_Y: f32 = 50.0;
const CL_META_Y: f32 = 86.0;
const CL_SPEED_Y: f32 = 124.0;
const CL_BRAKE_Y: f32 = 156.0;
const CL_ACCEL_Y: f32 = 188.0;
// --- Pneus ---
const TR_W: i32 = 256;
const TR_H: i32 = 272;
// --- G-force ---
const GF_W: i32 = 200;
const GF_H: i32 = 250;
// --- Elevação ---
const EL_W: i32 = 300;
const EL_H: i32 = 200;

// --- Toolbar de edição ---
const PANEL_COUNT: usize = 4;
const PANEL_KINDS: [Kind; PANEL_COUNT] =
    [Kind::Cluster, Kind::Tires, Kind::GForce, Kind::Elevation];
const SWATCHES: [f32; 8] = [4.0, 30.0, 50.0, 140.0, 188.0, 218.0, 280.0, 320.0];
const TB_PAD: i32 = 14;
const TB_LABEL_W: i32 = 52;
const TB_SW: i32 = 26;
const TB_GAP: i32 = 8;
const TB_COLOR_X: i32 = TB_PAD + TB_LABEL_W;
const TB_SWATCH_Y: i32 = 38;
const TB_COLOR_W: i32 = 8 * TB_SW + 7 * TB_GAP;
const TB_TARGET_X: i32 = TB_COLOR_X + TB_COLOR_W + 16;
const TB_TARGET_W: i32 = 174;
const TB_TARGET_SEG_W: i32 = TB_TARGET_W / 2;
const TB_TARGET_H: i32 = 30;
const TB_TARGET_Y: i32 = TB_SWATCH_Y - 2;
const TB_PANEL_Y: i32 = 82;
const TB_PANEL_W: i32 = 78;
const TB_PANEL_H: i32 = 30;
const TB_PANEL_GAP: i32 = 6;
const TB_PANEL_X: i32 = TB_COLOR_X;
// Linha 3: sliders do vidro (opacidade + tom).
const TB_ROW3_Y: i32 = 122;
const TB_SLIDER_H: i32 = 26;
const TB_GA_X: i32 = TB_COLOR_X;
const TB_GA_W: i32 = 150;
const TB_GT_LABEL_X: i32 = TB_GA_X + TB_GA_W + 16;
const TB_GT_X: i32 = TB_GT_LABEL_X + 38;
const TB_GT_W: i32 = TB_W - TB_PAD - TB_GT_X;
/// Célula "neutro" no início do slider de tom.
const TONE_NEUTRAL_W: i32 = 22;
const TONE_GAP: i32 = 4;
const TB_H: i32 = 160;
const TB_W: i32 = TB_TARGET_X + TB_TARGET_W + TB_PAD;
const HIT_NONE: i32 = -1;
const HIT_SWATCH_BASE: i32 = 10;
const HIT_TARGET_CARD: i32 = 100;
const HIT_TARGET_GEAR: i32 = 101;
const HIT_PANEL_BASE: i32 = 200;
const HIT_GLASS_ALPHA: i32 = 300;
const HIT_GLASS_TONE: i32 = 301;

const fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

const C_GREEN: D2D1_COLOR_F = color(0.204, 0.780, 0.349, 1.0);
const C_AMBER: D2D1_COLOR_F = color(0.902, 0.718, 0.282, 1.0);
const C_RED: D2D1_COLOR_F = color(0.961, 0.263, 0.212, 1.0);
const C_TRACK: D2D1_COLOR_F = color(1.0, 1.0, 1.0, 0.08);
const C_TRACK_EDGE: D2D1_COLOR_F = color(1.0, 1.0, 1.0, 0.05);
const C_WHITE: D2D1_COLOR_F = color(0.98, 0.98, 0.99, 1.0);
const C_GRAY: D2D1_COLOR_F = color(0.620, 0.667, 0.714, 0.92);
const C_EDIT: D2D1_COLOR_F = color(0.40, 0.80, 1.0, 0.9);
const C_ELEV_LINE: D2D1_COLOR_F = color(0.224, 0.780, 1.0, 1.0);
const C_ELEV_MARKER: D2D1_COLOR_F = color(1.0, 0.310, 0.541, 1.0);

// Vidro escuro polido: tinge o jogo por baixo em vez de "esfumaçar" (nada de
// cinza claro leitoso), gradiente vertical sutil e sem bordas — o contorno vem
// do contraste do próprio card. RGB base do tom neutro; a opacidade e o tom
// vêm do tema (sliders VIDRO/TOM da toolbar de edição).
const GLASS_TOP: D2D1_COLOR_F = color(0.071, 0.078, 0.098, 0.62); // #121419
const GLASS_BOT: D2D1_COLOR_F = color(0.027, 0.031, 0.043, 0.48); // #07080b
const GLASS_ALPHA_MIN: f32 = 0.12;
const GLASS_ALPHA_MAX: f32 = 0.95;

/// Topo/base do gradiente do vidro a partir do tema: opacidade ajustável e
/// tom opcional (negativo = neutro escuro; senão matiz tingido, sem neon).
fn glass_colors(t: Theme) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
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

fn rectf(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}
fn rounded(r: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: r,
        radiusX: radius,
        radiusY: radius,
    }
}

/// Espaço de desenho que preenche a janela (sem letterbox) com escala uniforme.
fn panel_canvas(sw: f32, sh: f32, base_w: f32, base_h: f32) -> (f32, f32, f32) {
    let aspect = sw / sh;
    let base_aspect = base_w / base_h;
    let (w, h) = if aspect >= base_aspect {
        (base_h * aspect, base_h)
    } else {
        (base_w, base_w / aspect)
    };
    (w, h, sw / w)
}

fn glass_radius(w: f32, h: f32) -> f32 {
    (w.min(h) * 0.078).clamp(8.0, 28.0)
}
/// Imã suave: encaixa na linha da malha só quando está dentro de `MAGNET`;
/// fora disso, posição livre (movimento natural, não quantizado).
fn soft_snap(v: i32) -> i32 {
    let nearest = ((v + GRID / 2).div_euclid(GRID)) * GRID;
    if (v - nearest).abs() <= MAGNET {
        nearest
    } else {
        v
    }
}

fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    let v = lparam.0 as u32;
    (
        (v & 0xFFFF) as i16 as i32,
        ((v >> 16) & 0xFFFF) as i16 as i32,
    )
}

fn swatch_rect(i: usize) -> (i32, i32, i32, i32) {
    let x = TB_COLOR_X + i as i32 * (TB_SW + TB_GAP);
    (x, TB_SWATCH_Y, x + TB_SW, TB_SWATCH_Y + TB_SW)
}

fn target_rect() -> (i32, i32, i32, i32) {
    (
        TB_TARGET_X,
        TB_TARGET_Y,
        TB_TARGET_X + TB_TARGET_W,
        TB_TARGET_Y + TB_TARGET_H,
    )
}

fn panel_toggle_rect(i: usize) -> (i32, i32, i32, i32) {
    let x = TB_PANEL_X + i as i32 * (TB_PANEL_W + TB_PANEL_GAP);
    (x, TB_PANEL_Y, x + TB_PANEL_W, TB_PANEL_Y + TB_PANEL_H)
}

fn glass_alpha_rect() -> (i32, i32, i32, i32) {
    (TB_GA_X, TB_ROW3_Y, TB_GA_X + TB_GA_W, TB_ROW3_Y + TB_SLIDER_H)
}

fn glass_tone_rect() -> (i32, i32, i32, i32) {
    (TB_GT_X, TB_ROW3_Y, TB_GT_X + TB_GT_W, TB_ROW3_Y + TB_SLIDER_H)
}

/// Início da faixa de matiz (depois da célula "neutro") no slider de tom.
fn tone_strip_x() -> i32 {
    TB_GT_X + TONE_NEUTRAL_W + TONE_GAP
}

fn alpha_from_x(x: i32) -> f32 {
    let f = ((x - TB_GA_X) as f32 / TB_GA_W as f32).clamp(0.0, 1.0);
    GLASS_ALPHA_MIN + f * (GLASS_ALPHA_MAX - GLASS_ALPHA_MIN)
}

fn alpha_knob_x(a: f32) -> f32 {
    let f = ((a - GLASS_ALPHA_MIN) / (GLASS_ALPHA_MAX - GLASS_ALPHA_MIN)).clamp(0.0, 1.0);
    TB_GA_X as f32 + f * TB_GA_W as f32
}

fn tone_from_x(x: i32) -> f32 {
    let sx = tone_strip_x();
    if x < sx - TONE_GAP / 2 {
        return -1.0;
    }
    ((x - sx) as f32 / (TB_GT_X + TB_GT_W - sx) as f32).clamp(0.0, 1.0) * 360.0
}

fn tone_knob_x(tone: f32) -> f32 {
    if tone < 0.0 {
        (TB_GT_X + TONE_NEUTRAL_W / 2) as f32
    } else {
        let sx = tone_strip_x();
        sx as f32 + (tone / 360.0).clamp(0.0, 1.0) * (TB_GT_X + TB_GT_W - sx) as f32
    }
}

fn in_rect(x: i32, y: i32, r: (i32, i32, i32, i32)) -> bool {
    x >= r.0 && x < r.2 && y >= r.1 && y < r.3
}

fn toolbar_hit(x: i32, y: i32) -> i32 {
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
fn toolbar_slider_drag(hit: i32, x: i32) {
    let mut t = THEME.get();
    match hit {
        HIT_GLASS_ALPHA => t.glass_alpha = alpha_from_x(x),
        HIT_GLASS_TONE => t.glass_tone = tone_from_x(x),
        _ => return,
    }
    THEME.set(t);
}

fn toolbar_hover_hit(hwnd: HWND) -> i32 {
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

fn toolbar_activate(hit: i32) {
    let mut t = THEME.get();
    if (HIT_SWATCH_BASE..HIT_SWATCH_BASE + SWATCHES.len() as i32).contains(&hit) {
        if let Some(hue) = SWATCHES.get((hit - HIT_SWATCH_BASE) as usize) {
            t.hue = *hue;
            THEME.set(t);
            save_theme();
        }
        return;
    }

    if hit == HIT_TARGET_CARD || hit == HIT_TARGET_GEAR {
        t.gear_target = hit == HIT_TARGET_GEAR;
        THEME.set(t);
        save_theme();
        return;
    }

    if (HIT_PANEL_BASE..HIT_PANEL_BASE + PANEL_COUNT as i32).contains(&hit) {
        let i = (hit - HIT_PANEL_BASE) as usize;
        let mut visible = PANEL_VISIBLE.get();
        visible[i] = !visible[i];
        PANEL_VISIBLE.set(visible);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Cluster,
    Tires,
    GForce,
    Elevation,
}

impl Kind {
    fn size(self) -> (i32, i32) {
        match self {
            Kind::Cluster => (CL_W, CL_H),
            Kind::Tires => (TR_W, TR_H),
            Kind::GForce => (GF_W, GF_H),
            Kind::Elevation => (EL_W, EL_H),
        }
    }
    fn default_pos(self, sw: i32, sh: i32) -> (i32, i32) {
        match self {
            Kind::Cluster => ((sw - CL_W) / 2, sh - CL_H - 30),
            Kind::Tires => (40, sh - TR_H - 30),
            Kind::GForce => (sw - GF_W - 40, sh - GF_H - 30),
            Kind::Elevation => (sw - EL_W - 40, 40),
        }
    }
    fn key(self) -> &'static str {
        match self {
            Kind::Cluster => "cluster",
            Kind::Tires => "tires",
            Kind::GForce => "gforce",
            Kind::Elevation => "elevation",
        }
    }
    fn idx(self) -> usize {
        match self {
            Kind::Cluster => 0,
            Kind::Tires => 1,
            Kind::GForce => 2,
            Kind::Elevation => 3,
        }
    }
    fn short_label(self) -> &'static str {
        match self {
            Kind::Cluster => "RPM",
            Kind::Tires => "PNEUS",
            Kind::GForce => "G",
            Kind::Elevation => "ALT",
        }
    }
    fn panel_kind(self) -> PanelKind {
        match self {
            Kind::Cluster => PanelKind::Cluster,
            Kind::Tires => PanelKind::Tires,
            Kind::GForce => PanelKind::GForce,
            Kind::Elevation => PanelKind::Elevation,
        }
    }
}

// ===================== GDI / D2D helpers =====================

#[derive(Clone)]
struct Fonts {
    gear: IDWriteTextFormat,
    speed: IDWriteTextFormat,
    tire: IDWriteTextFormat,
    micro: IDWriteTextFormat,
    small: IDWriteTextFormat,
    small_left: IDWriteTextFormat,
    small_right: IDWriteTextFormat,
    toolbar_button: IDWriteTextFormat,
    toolbar_label: IDWriteTextFormat,
    toolbar_caption: IDWriteTextFormat,
}

struct Brushes {
    track: ID2D1SolidColorBrush,
    track_edge: ID2D1SolidColorBrush,
    accel: ID2D1SolidColorBrush,
    brake: ID2D1SolidColorBrush,
    white: ID2D1SolidColorBrush,
    gray: ID2D1SolidColorBrush,
    scratch: ID2D1SolidColorBrush,
    grad: ID2D1LinearGradientBrush,
    /// Fundo do card (gradiente vertical) — recriado quando opacidade/tom mudam.
    glass: RefCell<GlassCache>,
    design_h: f32,
}

struct GlassCache {
    key: u64,
    brush: ID2D1LinearGradientBrush,
}

fn glass_key(t: Theme) -> u64 {
    ((t.glass_alpha.to_bits() as u64) << 32) | t.glass_tone.to_bits() as u64
}

fn make_glass_brush(
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
        let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
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
    /// Brush do vidro para o tema atual — reusa o cache se opacidade/tom não mudaram.
    fn glass(&self, rt: &ID2D1RenderTarget, t: Theme) -> Result<ID2D1LinearGradientBrush> {
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

    fn new(rt: &ID2D1RenderTarget, design_h: f32) -> Result<Self> {
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

fn make_fonts() -> Result<Fonts> {
    unsafe {
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
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
                w!("pt-BR"),
            )?;
            fmt.SetTextAlignment(align)?;
            fmt.SetParagraphAlignment(para)?;
            Ok(fmt)
        };
        // Cortes ópticos do Segoe UI Variable (Win11): "Display" para números
        // grandes, "Text" desenhado para 9–14px — nítido em texto pequeno.
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

/// Cria uma janela layered (transparente, topo, sem taskbar, click-through).
fn make_window(class: PCWSTR, instance: HINSTANCE, x: i32, y: i32, w: i32, h: i32) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Forza HUD"),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            None,
        )
    }
}

/// Cria um DIB top-down 32 bpp (premultiplicado) ligado a um DC.
fn create_dib_bitmap(memdc: HDC, w: i32, h: i32) -> Result<HBITMAP> {
    unsafe {
        let mut bmi: BITMAPINFO = core::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
        CreateDIBSection(Some(memdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
    }
}

/// DC de memória + DIB selecionado (alvo do D2D / fonte do ULW).
fn make_dib(w: i32, h: i32) -> Result<(HDC, HBITMAP)> {
    unsafe {
        let memdc = CreateCompatibleDC(None);
        let bitmap = create_dib_bitmap(memdc, w, h)?;
        SelectObject(memdc, HGDIOBJ(bitmap.0));
        Ok((memdc, bitmap))
    }
}

fn window_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        r
    }
}

/// Tamanho-base (design) do painel, guardado no GWLP_USERDATA da janela
/// (alto 16 bits = largura, baixo 16 = altura). Usado para travar a proporção.
unsafe fn design_of(hwnd: HWND) -> (i32, i32) {
    let ud = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    (((ud >> 16) & 0xFFFF) as i32, (ud & 0xFFFF) as i32)
}

fn make_dcrt(factory: &ID2D1Factory) -> Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
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

/// Atualiza a janela layered com o conteúdo já desenhado no `memdc`.
fn present(hwnd: HWND, memdc: HDC, pos: POINT, size: SIZE, alpha: u8) -> Result<()> {
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let src = POINT { x: 0, y: 0 };
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&pos as *const POINT),
            Some(&size as *const SIZE),
            Some(memdc),
            Some(&src as *const POINT),
            COLORREF(0),
            Some(&blend as *const BLENDFUNCTION),
            ULW_ALPHA,
        )
    }
}

fn set_interactive(hwnd: HWND, interactive: bool) {
    unsafe {
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mask = WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
        let new = if interactive { cur & !mask } else { cur | mask };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new as isize);
        // Sem SWP_FRAMECHANGED a mudança de ex-style não é reavaliada no hit-test.
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

fn bring_to_top(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

// ===================== Painel =====================

struct Panel {
    kind: Kind,
    hwnd: HWND,
    memdc: HDC,
    bitmap: HBITMAP,
    pos: POINT,
    size: SIZE,
    design: SIZE,
    factory: ID2D1Factory,
    dcrt: ID2D1DCRenderTarget,
    rt: ID2D1RenderTarget,
    b: Brushes,
    f: Fonts,
    enabled: bool,
    shown: Cell<bool>,
    paint_key: Cell<u64>,
}

impl Panel {
    fn new(
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
        // Mantém a proporção do design (a escala uniforme preenche sem margem);
        // corrige tamanhos salvos fora de proporção.
        let min_scale = MIN_SZ as f32 / (dw.min(dh) as f32);
        let scale = (size.0 as f32 / dw as f32)
            .max(size.1 as f32 / dh as f32)
            .max(min_scale);
        let (w, h) = (
            (dw as f32 * scale).round() as i32,
            (dh as f32 * scale).round() as i32,
        );
        let hwnd = make_window(class, instance, pos.0, pos.1, w, h)?;
        unsafe {
            // Guarda a proporção do design para a trava de aspecto no resize.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ((dw as isize) << 16) | (dh as isize));
        }
        let (memdc, bitmap) = make_dib(w, h)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        // ClearType não funciona sobre alpha por pixel (franja/borrão em janela
        // layered) — grayscale dá contorno limpo em qualquer fundo.
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

    fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.paint_key.set(0);
        if !enabled {
            self.set_shown(false);
        }
    }

    fn set_shown(&self, show: bool) {
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

    /// Recria o DIB no novo tamanho (após redimensionar a janela).
    fn resize_dib(&mut self, w: i32, h: i32) -> Result<()> {
        let new_bmp = create_dib_bitmap(self.memdc, w, h)?;
        unsafe {
            let old = SelectObject(self.memdc, HGDIOBJ(new_bmp.0));
            let _ = DeleteObject(old);
        }
        self.bitmap = new_bmp;
        self.size = SIZE { cx: w, cy: h };
        Ok(())
    }

    fn text(&self, s: &str, fmt: &IDWriteTextFormat, r: D2D_RECT_F, brush: &ID2D1SolidColorBrush) {
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

    /// Superfície do card: vidro escuro num fill único, sem borda nem brilho —
    /// o contorno vem do contraste. `shift` = card inteiro na cor do tema;
    /// `edit` = contorno ciano de seleção (funcional, só no modo edição).
    fn draw_card(
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

    fn pedal_row_right(&self, label: &str, frac: f32, fill: &ID2D1SolidColorBrush, y: f32, w: f32) {
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

    fn draw_cluster(
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
        self.pedal_row_right("THROTTLE", f.accel, &self.b.accel, py(CL_ACCEL_Y), w);
    }

    fn draw_tire_range(
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

    fn draw_tires(&self, f: &DisplayFrame, w: f32, h: f32) {
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
            "TIRES",
            &self.f.small_left,
            rectf(pad, pad * 0.35, pad + grid_w * 0.55, head_h),
            &self.b.gray,
        );
        self.text(
            "°C",
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

        let corners = ["FL", "FR", "RL", "RR"];
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

            // Badge do canto
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

            // Temperatura central
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
                    .unwrap_or_else(|| "–".to_string());
                let max_s = max_v
                    .map(|v| format!("{:.0}", v))
                    .unwrap_or_else(|| "–".to_string());
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
                    "–",
                    &self.f.tire,
                    rectf(x + inner, temp_top, x + cell_w - inner, temp_bot),
                    &self.b.gray,
                );
            }
        }
    }

    fn draw_gforce(&self, f: &DisplayFrame, w: f32, h: f32) {
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

    fn draw_elevation(&self, f: &DisplayFrame, elev: &ElevationProfile, w: f32, h: f32) {
        let pad = (w * 0.033).max(6.0);
        let head_h = (h * 0.13).max(22.0);
        let foot_h = (h * 0.12).max(18.0);
        let chart_top = head_h + h * 0.02;
        let chart_bot = h - foot_h - h * 0.02;
        let chart_w = w - pad * 2.0;
        let chart_h = (chart_bot - chart_top).max(20.0);

        self.text(
            "ELEVATION",
            &self.f.small_left,
            rectf(pad, pad * 0.4, pad + w * 0.45, head_h),
            &self.b.gray,
        );
        let lap = if elev.loop_closed {
            format!("{:.2} km", elev.loop_km)
        } else if elev.lap_km > 0.0 {
            format!("{:.2} km", elev.lap_km)
        } else {
            "–".to_string()
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
            format!("{} m", elev.min.round() as i32)
        } else {
            "–".to_string()
        };
        let max_l = if elev.pts.len() >= 2 {
            format!("{} m", elev.max.round() as i32)
        } else {
            "–".to_string()
        };
        let foot_y = h - foot_h + h * 0.01;
        let foot_bot = h - pad * 0.4;
        self.text(
            "MIN",
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
            "MAX",
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
            format!("{} m", f.elev_m.round() as i32)
        } else {
            "–".to_string()
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

    /// Retorna `true` se desenhou (chamou `UpdateLayeredWindow`).
    fn draw(
        &self,
        f: &DisplayFrame,
        elev: &ElevationProfile,
        edit: bool,
        accent: D2D1_COLOR_F,
        gear_accent: bool,
    ) -> Result<bool> {
        if !self.enabled {
            self.set_shown(false);
            self.paint_key.set(0);
            return Ok(false);
        }
        let visible = edit || f.alpha > 0.01;
        self.set_shown(visible);
        if !visible {
            self.paint_key.set(0);
            return Ok(false);
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
        if !edit && !f.animating && key == self.paint_key.get() {
            return Ok(false);
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
        present(self.hwnd, self.memdc, self.pos, self.size, alpha)?;
        Ok(true)
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

// ===================== Malha (fundo do modo edição) =====================

struct Backdrop {
    hwnd: HWND,
    memdc: HDC,
    bitmap: HBITMAP,
    size: SIZE,
    dcrt: ID2D1DCRenderTarget,
    rt: ID2D1RenderTarget,
    dim: ID2D1SolidColorBrush,
    line: ID2D1SolidColorBrush,
}

impl Backdrop {
    fn new(
        class: PCWSTR,
        instance: HINSTANCE,
        factory: &ID2D1Factory,
        sw: i32,
        sh: i32,
    ) -> Result<Self> {
        let hwnd = make_window(class, instance, 0, 0, sw, sh)?;
        let (memdc, bitmap) = make_dib(sw, sh)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        let dim = unsafe { rt.CreateSolidColorBrush(&color(0.0, 0.0, 0.0, 0.35), None)? };
        let line = unsafe { rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 0.12), None)? };
        Ok(Self {
            hwnd,
            memdc,
            bitmap,
            size: SIZE { cx: sw, cy: sh },
            dcrt,
            rt,
            dim,
            line,
        })
    }

    fn draw(&self) -> Result<()> {
        let (w, h) = (self.size.cx as f32, self.size.cy as f32);
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
            self.rt.FillRectangle(&rectf(0.0, 0.0, w, h), &self.dim);
            let mut x = 0;
            while x <= self.size.cx {
                self.rt.DrawLine(
                    Vector2 {
                        X: x as f32,
                        Y: 0.0,
                    },
                    Vector2 { X: x as f32, Y: h },
                    &self.line,
                    1.0,
                    None,
                );
                x += GRID;
            }
            let mut y = 0;
            while y <= self.size.cy {
                self.rt.DrawLine(
                    Vector2 {
                        X: 0.0,
                        Y: y as f32,
                    },
                    Vector2 { X: w, Y: y as f32 },
                    &self.line,
                    1.0,
                    None,
                );
                y += GRID;
            }
            self.rt.EndDraw(None, None)?;
        }
        present(self.hwnd, self.memdc, POINT { x: 0, y: 0 }, self.size, 255)
    }

    fn show(&self) {
        let _ = self.draw();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        bring_to_top(self.hwnd);
    }
    fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memdc);
        }
    }
}

// ===================== toolbar de edição =====================

struct Toolbar {
    hwnd: HWND,
    memdc: HDC,
    bitmap: HBITMAP,
    pos: POINT,
    size: SIZE,
    dcrt: ID2D1DCRenderTarget,
    rt: ID2D1RenderTarget,
    bg: ID2D1SolidColorBrush,
    edge: ID2D1SolidColorBrush,
    text_brush: ID2D1SolidColorBrush,
    muted_brush: ID2D1SolidColorBrush,
    scratch: ID2D1SolidColorBrush,
    font_button: IDWriteTextFormat,
    font_label: IDWriteTextFormat,
    font_caption: IDWriteTextFormat,
}

impl Toolbar {
    fn new(
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

    fn text(&self, s: &str, fmt: &IDWriteTextFormat, r: D2D_RECT_F, brush: &ID2D1SolidColorBrush) {
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

    fn draw(&self) -> Result<()> {
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
                self.scratch.SetColor(&hsv_color(t.glass_tone, 0.72, 0.85, 1.0));
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

    fn show(&self) {
        let _ = self.draw();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        bring_to_top(self.hwnd);
    }

    fn hide(&self) {
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

// ===================== persistência de layout =====================

#[derive(Clone, Copy)]
struct LayoutEntry {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    visible: bool,
}

fn layout_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base)
        .join("forza-hud-native")
        .join("layout.txt")
}

/// "key=x,y,w,h,visible" por linha. O formato antigo sem `visible` ainda carrega.
fn load_layout() -> HashMap<String, LayoutEntry> {
    let mut map = HashMap::new();
    if let Ok(text) = fs::read_to_string(layout_path()) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let nums: Vec<i32> = v.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                if nums.len() >= 4 {
                    map.insert(
                        k.trim().to_string(),
                        LayoutEntry {
                            x: nums[0],
                            y: nums[1],
                            w: nums[2],
                            h: nums[3],
                            visible: nums.get(4).copied().unwrap_or(1) != 0,
                        },
                    );
                }
            }
        }
    }
    map
}

fn save_layout(panels: &[Panel]) {
    let mut s = String::new();
    for p in panels {
        s.push_str(&format!(
            "{}={},{},{},{},{}\n",
            p.kind.key(),
            p.pos.x,
            p.pos.y,
            p.size.cx,
            p.size.cy,
            if p.enabled { 1 } else { 0 }
        ));
    }
    let path = layout_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, s);
}

fn theme_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base)
        .join("forza-hud-native")
        .join("theme.txt")
}

fn load_theme() -> Theme {
    let mut t = THEME_DEFAULT;
    if let Ok(text) = fs::read_to_string(theme_path()) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                match k.trim() {
                    "hue" => {
                        if let Ok(h) = v.trim().parse() {
                            t.hue = h;
                        }
                    }
                    "target" => t.gear_target = v.trim() == "gear",
                    "glass_alpha" => {
                        if let Ok(a) = v.trim().parse::<f32>() {
                            t.glass_alpha = a.clamp(GLASS_ALPHA_MIN, GLASS_ALPHA_MAX);
                        }
                    }
                    "glass_tone" => {
                        if let Ok(tone) = v.trim().parse::<f32>() {
                            t.glass_tone = if tone < 0.0 {
                                -1.0
                            } else {
                                tone.rem_euclid(360.0)
                            };
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    t
}

fn save_theme() {
    let t = THEME.get();
    let s = format!(
        "hue={}\ntarget={}\nglass_alpha={}\nglass_tone={}\n",
        t.hue,
        if t.gear_target { "gear" } else { "card" },
        t.glass_alpha,
        t.glass_tone,
    );
    let path = theme_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, s);
}

// ===================== window procs =====================

/// Zona sob o cursor (coords de tela): centro = mover; bordas/cantos = resize.
unsafe fn zone_at(hwnd: HWND, sx: i32, sy: i32) -> Zone {
    let r = window_rect(hwnd);
    let m = RESIZE_MARGIN;
    let l = sx < r.left + m;
    let rr = sx >= r.right - m;
    let t = sy < r.top + m;
    let b = sy >= r.bottom - m;
    if l && t {
        Zone::Tl
    } else if rr && t {
        Zone::Tr
    } else if l && b {
        Zone::Bl
    } else if rr && b {
        Zone::Br
    } else if l {
        Zone::L
    } else if rr {
        Zone::R
    } else if t {
        Zone::T
    } else if b {
        Zone::B
    } else {
        Zone::Move
    }
}

/// Cursor estilo janela do Windows conforme a zona sob o ponteiro.
unsafe fn apply_cursor(hwnd: HWND) {
    let mut m = POINT::default();
    let _ = GetCursorPos(&mut m);
    let idc = match zone_at(hwnd, m.x, m.y) {
        Zone::Move => IDC_SIZEALL,
        Zone::L | Zone::R => IDC_SIZEWE,
        Zone::T | Zone::B => IDC_SIZENS,
        Zone::Tl | Zone::Br => IDC_SIZENWSE,
        Zone::Tr | Zone::Bl => IDC_SIZENESW,
    };
    if let Ok(cur) = LoadCursorW(None, idc) {
        SetCursor(Some(cur));
    }
}

unsafe fn grab_begin(hwnd: HWND) {
    let mut m = POINT::default();
    let _ = GetCursorPos(&mut m);
    let r = window_rect(hwnd);
    let z = zone_at(hwnd, m.x, m.y);
    let g = if z == Zone::Move {
        Grab {
            win0: r,
            mouse0: m,
            move_mode: true,
            anchor: POINT {
                x: r.left,
                y: r.top,
            },
            h_active: false,
            v_active: false,
            from_right: false,
            from_bottom: false,
        }
    } else {
        let drag_left = matches!(z, Zone::L | Zone::Tl | Zone::Bl);
        let drag_right = matches!(z, Zone::R | Zone::Tr | Zone::Br);
        let drag_top = matches!(z, Zone::T | Zone::Tl | Zone::Tr);
        let drag_bottom = matches!(z, Zone::B | Zone::Bl | Zone::Br);
        Grab {
            win0: r,
            mouse0: m,
            move_mode: false,
            anchor: POINT {
                x: if drag_left { r.right } else { r.left },
                y: if drag_top { r.bottom } else { r.top },
            },
            h_active: drag_left || drag_right,
            v_active: drag_top || drag_bottom,
            from_right: drag_left,
            from_bottom: drag_top,
        }
    };
    GRAB.set(Some(g));
    let _ = SetCapture(hwnd);
}

unsafe fn grab_move(hwnd: HWND) {
    let Some(g) = GRAB.get() else {
        return;
    };
    let mut m = POINT::default();
    let _ = GetCursorPos(&mut m);

    if g.move_mode {
        let nx = soft_snap(g.win0.left + (m.x - g.mouse0.x));
        let ny = soft_snap(g.win0.top + (m.y - g.mouse0.y));
        let _ = SetWindowPos(
            hwnd,
            None,
            nx,
            ny,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        return;
    }

    // Resize: escala a partir do canto fixo em direção ao cursor, mantendo a
    // proporção do design — o card cresce inteiro (preenche, sem deformar, sem
    // tremer). Imã suave encaixa a largura na malha.
    let (dw, dh) = design_of(hwnd);
    let (dwf, dhf) = (dw.max(1) as f32, dh.max(1) as f32);
    let min_scale = MIN_SZ as f32 / dwf.min(dhf);
    let mut scale = min_scale;
    if g.h_active {
        scale = scale.max((m.x - g.anchor.x).abs() as f32 / dwf);
    }
    if g.v_active {
        scale = scale.max((m.y - g.anchor.y).abs() as f32 / dhf);
    }
    let snapped_w = soft_snap((dwf * scale).round() as i32) as f32;
    scale = (snapped_w / dwf).max(min_scale);

    let nw = (dwf * scale).round() as i32;
    let nh = (dhf * scale).round() as i32;
    let left = if g.from_right {
        g.anchor.x - nw
    } else {
        g.anchor.x
    };
    let top = if g.from_bottom {
        g.anchor.y - nh
    } else {
        g.anchor.y
    };
    let _ = SetWindowPos(hwnd, None, left, top, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE);
}

unsafe fn grab_end() {
    let _ = ReleaseCapture();
    GRAB.set(None);
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if EDIT.load(Ordering::Relaxed) {
        match msg {
            // Tudo área-cliente: cursor + arraste/resize manuais (sem o loop modal
            // do OS), para o conteúdo escalar ao vivo durante o ajuste.
            WM_NCHITTEST => return LRESULT(HTCLIENT as isize),
            WM_SETCURSOR => {
                apply_cursor(hwnd);
                return LRESULT(1);
            }
            WM_LBUTTONDOWN => {
                grab_begin(hwnd);
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                grab_move(hwnd);
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                grab_end();
                return LRESULT(0);
            }
            WM_MOUSEWHEEL => {
                // Roda do mouse gira o matiz do destaque.
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let mut t = THEME.get();
                t.hue = (t.hue + (delta as f32 / 120.0) * 12.0).rem_euclid(360.0);
                THEME.set(t);
                return LRESULT(0);
            }
            WM_RBUTTONDOWN => {
                // Botão direito alterna: shift colore a marcha ou o card.
                let mut t = THEME.get();
                t.gear_target = !t.gear_target;
                THEME.set(t);
                save_theme();
                return LRESULT(0);
            }
            _ => {}
        }
    } else if msg == WM_NCHITTEST {
        // Fora de edição: transparente ao hit-test (clique vai pro jogo).
        return LRESULT(HTTRANSPARENT as isize);
    }
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn backdrop_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize), // a malha nunca pega clique
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn toolbar_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lparam);
            let hit = toolbar_hit(x, y);
            TOOLBAR_DOWN.with(|v| v.set(hit));
            if hit != HIT_NONE {
                let _ = SetCapture(hwnd);
            }
            if hit == HIT_GLASS_ALPHA || hit == HIT_GLASS_TONE {
                toolbar_slider_drag(hit, x);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            // Arrasto contínuo dos sliders do vidro (capture ativo).
            let down = TOOLBAR_DOWN.with(|v| v.get());
            if down == HIT_GLASS_ALPHA || down == HIT_GLASS_TONE {
                let (x, _) = lparam_xy(lparam);
                toolbar_slider_drag(down, x);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_xy(lparam);
            let hit = toolbar_hit(x, y);
            let down = TOOLBAR_DOWN.with(|v| {
                let down = v.get();
                v.set(HIT_NONE);
                down
            });
            let _ = ReleaseCapture();
            if down == HIT_GLASS_ALPHA || down == HIT_GLASS_TONE {
                save_theme();
            } else if hit == down && hit != HIT_NONE {
                toolbar_activate(hit);
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            TOOLBAR_DOWN.with(|v| v.set(HIT_NONE));
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn toggle_edit(panels: &mut [Panel], backdrop: &Backdrop, toolbar: &Toolbar) {
    let now = !EDIT.load(Ordering::Relaxed);
    EDIT.store(now, Ordering::Relaxed);
    for p in panels.iter() {
        set_interactive(p.hwnd, now);
    }
    if now {
        backdrop.show();
        for p in panels.iter() {
            bring_to_top(p.hwnd); // painéis acima da malha
        }
        toolbar.show();
    } else {
        save_current_layout(panels);
        toolbar.hide();
        backdrop.hide();
        save_theme();
    }
}

fn sync_panel_visibility(panels: &mut [Panel]) {
    let visible = PANEL_VISIBLE.get();
    for p in panels.iter_mut() {
        p.set_enabled(visible[p.kind.idx()]);
    }
}

fn save_current_layout(panels: &mut [Panel]) {
    sync_panel_visibility(panels);
    for p in panels.iter_mut() {
        sync_panel_geometry(p);
    }
    save_layout(panels);
}

fn hide_overlay_windows(panels: &[Panel], backdrop: &Backdrop, toolbar: &Toolbar) {
    for p in panels {
        p.set_shown(false);
    }
    toolbar.hide();
    backdrop.hide();
}

fn toggle_hud_hidden(panels: &mut [Panel], backdrop: &Backdrop, toolbar: &Toolbar) {
    let hide = !HUD_HIDDEN.load(Ordering::Relaxed);
    HUD_HIDDEN.store(hide, Ordering::Relaxed);
    if hide {
        if EDIT.swap(false, Ordering::Relaxed) {
            save_current_layout(panels);
        }
        for p in panels.iter() {
            set_interactive(p.hwnd, false);
        }
        hide_overlay_windows(panels, backdrop, toolbar);
    }
}

/// Lê pos/tamanho atuais da janela (já encaixados na malha durante o arraste)
/// e recria o DIB se o tamanho mudou.
fn sync_panel_geometry(p: &mut Panel) {
    let r = window_rect(p.hwnd);
    p.pos = POINT {
        x: r.left,
        y: r.top,
    };
    let (nw, nh) = (r.right - r.left, r.bottom - r.top);
    if nw != p.size.cx || nh != p.size.cy {
        let _ = p.resize_dib(nw, nh);
    }
}

pub fn run() -> Result<()> {
    crate::forza::catalog::init();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);

        let panel_class = w!("ForzaPanelClass");
        let backdrop_class = w!("ForzaBackdropClass");
        let toolbar_class = w!("ForzaToolbarClass");
        RegisterClassW(&WNDCLASSW {
            hInstance: instance,
            lpszClassName: panel_class,
            lpfnWndProc: Some(panel_proc),
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            hInstance: instance,
            lpszClassName: backdrop_class,
            lpfnWndProc: Some(backdrop_proc),
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            hInstance: instance,
            lpszClassName: toolbar_class,
            lpfnWndProc: Some(toolbar_proc),
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        });

        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);

        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let fonts = make_fonts()?;
        let saved = load_layout();
        THEME.set(load_theme());
        let mut panel_visible = [true; PANEL_COUNT];
        for kind in PANEL_KINDS {
            if let Some(entry) = saved.get(kind.key()) {
                panel_visible[kind.idx()] = entry.visible;
            }
        }
        PANEL_VISIBLE.set(panel_visible);

        let mut panels: Vec<Panel> = Vec::new();
        for kind in PANEL_KINDS {
            let (dw, dh) = kind.size();
            let entry = saved.get(kind.key()).copied().unwrap_or_else(|| {
                let (dx, dy) = kind.default_pos(sw, sh);
                LayoutEntry {
                    x: dx,
                    y: dy,
                    w: dw,
                    h: dh,
                    visible: true,
                }
            });
            panels.push(Panel::new(
                kind,
                panel_class,
                instance,
                &factory,
                &fonts,
                (entry.x, entry.y),
                (entry.w.max(MIN_SZ), entry.h.max(MIN_SZ)),
                panel_visible[kind.idx()],
            )?);
        }
        let backdrop = Backdrop::new(backdrop_class, instance, &factory, sw, sh)?;
        let toolbar = Toolbar::new(toolbar_class, instance, &factory, &fonts, sw)?;

        let mut smoother = DisplaySmoother::new();
        let mut last_frame = Instant::now();

        let shared = Arc::new(Shared::new());

        // Mapeamento de pista (worldmap + live API :9877) — desligado no lançamento.
        // Ver `forza::TRACK_MAPPING_ENABLED`.
        let track = if TRACK_MAPPING_ENABLED {
            let wm_path = worldmap::default_path();
            let recorder = {
                let mut r = WorldMapRecorder::new();
                r.load_seed(&wm_path);
                Arc::new(Mutex::new(r))
            };
            worldmap::spawn_autosave(recorder.clone(), wm_path.clone());
            let _live = crate::forza::live_api::spawn_live_api(recorder.clone(), wm_path.clone());
            Some((recorder, wm_path))
        } else {
            println!("[overlay] mapeamento de pista desligado (lançamento HUD)");
            None
        };

        let port = std::env::var("FORZA_UDP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(UDP_PORT);
        let recorder_udp = track.as_ref().map(|(r, _)| r.clone());
        match udp::spawn(shared.clone(), port, move |t| {
            if let Some(ref rec_cb) = recorder_udp {
                let active = visibility::is_driving(t, 0.0);
                rec_cb.lock().unwrap().ingest(t, active);
            }
        }) {
            Ok(_) => println!(
                "[overlay] ouvindo UDP em 0.0.0.0:{port} | Ctrl+Alt+E editar | Ctrl+Alt+H ocultar | Ctrl+Alt+Q sair"
            ),
            Err(e) => eprintln!("[overlay] falha ao abrir UDP {port}: {e}"),
        }

        let _ = RegisterHotKey(None, HK_QUIT, MOD_CONTROL | MOD_ALT, 0x51 /* Q */);
        let _ = RegisterHotKey(None, HK_EDIT, MOD_CONTROL | MOD_ALT, 0x45 /* E */);
        let _ = RegisterHotKey(None, HK_HIDE, MOD_CONTROL | MOD_ALT, 0x48 /* H */);

        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    save_current_layout(&mut panels);
                    save_theme();
                    smoother.persist();
                    if let Some((ref recorder, ref wm_path)) = track {
                        // Compacta ao sair: snapshot JSON completo + trunca os logs
                        // incrementais. Garante um boot rápido na próxima sessão.
                        let mut g = recorder.lock().unwrap();
                        let _ = g.compact(wm_path);
                    }
                    let _ = UnregisterHotKey(None, HK_QUIT);
                    let _ = UnregisterHotKey(None, HK_EDIT);
                    let _ = UnregisterHotKey(None, HK_HIDE);
                    return Ok(());
                }
                if msg.message == WM_HOTKEY {
                    match msg.wParam.0 as i32 {
                        HK_QUIT => {
                            save_current_layout(&mut panels);
                            save_theme();
                            smoother.persist();
                            if let Some((ref recorder, ref wm_path)) = track {
                                let g = recorder.lock().unwrap();
                                let _ = g.save(wm_path);
                                let _ = g.save_path(wm_path);
                            }
                            let _ = UnregisterHotKey(None, HK_QUIT);
                            let _ = UnregisterHotKey(None, HK_EDIT);
                            let _ = UnregisterHotKey(None, HK_HIDE);
                            return Ok(());
                        }
                        HK_EDIT => {
                            HUD_HIDDEN.store(false, Ordering::Relaxed);
                            toggle_edit(&mut panels, &backdrop, &toolbar);
                        }
                        HK_HIDE => toggle_hud_hidden(&mut panels, &backdrop, &toolbar),
                        _ => {}
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if HUD_HIDDEN.load(Ordering::Relaxed) {
                hide_overlay_windows(&panels, &backdrop, &toolbar);
                thread::sleep(Duration::from_millis(IDLE_SLEEP_MS));
                continue;
            }

            let snap = shared.latest();
            let edit = EDIT.load(Ordering::Relaxed);
            let theme = THEME.get();
            let accent = hue_color(theme.hue);
            let fd = smoother.tick(&snap, edit).clone();
            let elev = smoother.elevation();
            let mut presented = false;
            if edit {
                sync_panel_visibility(&mut panels);
            }
            for p in panels.iter_mut() {
                if edit {
                    sync_panel_geometry(p);
                }
                match p.draw(&fd, &elev, edit, accent, theme.gear_target) {
                    Ok(true) => presented = true,
                    Ok(false) => {}
                    Err(e) => eprintln!("[overlay] draw: {e:?}"),
                }
            }
            if edit {
                let _ = toolbar.draw();
                bring_to_top(toolbar.hwnd);
                presented = true;
            }

            let elapsed = last_frame.elapsed();
            if presented || fd.animating || edit {
                last_frame = Instant::now();
                if elapsed < Duration::from_millis(FRAME_MIN_MS) {
                    thread::sleep(Duration::from_millis(FRAME_MIN_MS) - elapsed);
                }
            } else if fd.alpha < 0.01 {
                thread::sleep(Duration::from_millis(IDLE_SLEEP_MS));
            } else {
                thread::sleep(Duration::from_millis(FRAME_MIN_MS));
            }
        }
    }
}
