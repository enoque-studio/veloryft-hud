//! Overlay nativo: cada painel é uma janela layered própria (pequena,
//! click-through, alpha por pixel via `UpdateLayeredWindow`). Modo edição
//! (Ctrl+Alt+E) mostra uma malha na tela e torna os painéis arrastáveis;
//! posições são salvas e recarregadas. Ctrl+Alt+Q sai.

#![allow(clippy::too_many_lines)]

use std::cell::Cell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::{Matrix3x2, Vector2};

use crate::forza::payload::{self, Telemetry};
use crate::forza::state::Shared;
use crate::forza::udp;
use crate::forza::visibility;

const TAU_BAR: f32 = 0.032;
const TAU_TEXT: f32 = 0.11;
const TAU_FADE: f32 = 0.07;
const SHIFT_AT: f32 = 0.92;
const LEARN_MIN_THROTTLE: f32 = 90.0;
const LEARN_MIN_OVER_IDLE: f32 = 2200.0;
const LEARN_MIN_REPORTED_FRAC: f32 = 0.45;
const LEARN_SUSPICIOUS_REPORTED_FRAC: f32 = 0.88;
const STALE_S: f32 = 1.5;
const UDP_PORT: u16 = 20066;
const HK_QUIT: i32 = 1;
const HK_EDIT: i32 = 2;
const GRID: i32 = 40; // malha / passo de snap
const RESIZE_MARGIN: i32 = 8; // borda sensível para redimensionar (px)
const MIN_SZ: i32 = 80; // tamanho mínimo do card
const MAGNET: i32 = 12; // alcance do "imã" suave até a linha da malha (px)

static EDIT: AtomicBool = AtomicBool::new(false);

// Tema editável (thread da UI): matiz do destaque do shift + se ele colore a
// marcha (true) ou o card (false). Roda do mouse muda a cor; botão direito alterna.
#[derive(Clone, Copy)]
struct Theme {
    hue: f32,
    gear_target: bool,
}
thread_local! {
    static THEME: Cell<Theme> = const { Cell::new(Theme { hue: 4.0, gear_target: false }) };
}

/// HSV→RGB (s/v fixos, cor viva) para o matiz do destaque.
fn hue_color(h: f32) -> D2D1_COLOR_F {
    let (s, v) = (0.82f32, 0.96f32);
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
    color(r + m, g + m, b + m, 1.0)
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
const TR_W: i32 = 210;
const TR_H: i32 = 210;
// --- G-force ---
const GF_W: i32 = 200;
const GF_H: i32 = 250;

// --- Barra de cores (modo edição) ---
const SWATCHES: [f32; 8] = [4.0, 30.0, 50.0, 140.0, 188.0, 218.0, 280.0, 320.0];
const TB_PAD: i32 = 12;
const TB_SW: i32 = 34; // tamanho do swatch
const TB_GAP: i32 = 6;
const TB_TOGGLE_W: i32 = 104;
const TB_H: i32 = 58;
const TB_W: i32 = TB_PAD + 8 * TB_SW + 7 * TB_GAP + 8 + TB_TOGGLE_W + TB_PAD;
/// x local do botão CARD/MARCHA.
const TB_TOGGLE_X: i32 = TB_PAD + 8 * TB_SW + 7 * TB_GAP + 8;
const TB_SWATCH_Y: i32 = (TB_H - TB_SW) / 2;

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

// Glassmorphism (vidro fosco simulado): fundo translúcido em gradiente +
// borda clara + brilho interno. O jogo aparece tingido através do card.
const GLASS_TOP: D2D1_COLOR_F = color(0.172, 0.192, 0.235, 0.55); // #2c313c a 0.55
const GLASS_BOT: D2D1_COLOR_F = color(0.063, 0.071, 0.094, 0.34); // #101218 a 0.34
const GLASS_BORDER: D2D1_COLOR_F = color(1.0, 1.0, 1.0, 0.14);
const GLASS_HI: D2D1_COLOR_F = color(1.0, 1.0, 1.0, 0.07);
const GLASS_RADIUS: f32 = 18.0;

fn exp_smooth(cur: f32, target: f32, dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 || dt <= 0.0 {
        return target;
    }
    cur + (target - cur) * (1.0 - (-dt / tau).exp())
}

fn rectf(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right, bottom }
}
fn rounded(r: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }
}
fn slip_color(s: f32) -> D2D1_COLOR_F {
    let f = (s / 1.3).clamp(0.0, 1.0);
    color(0.2 + 0.8 * f, 0.85 - 0.65 * f, 0.25 * (1.0 - f), 0.18 + 0.55 * f)
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
    ((v & 0xFFFF) as i16 as i32, ((v >> 16) & 0xFFFF) as i16 as i32)
}

fn swatch_rect(i: usize) -> (i32, i32, i32, i32) {
    let x = TB_PAD + i as i32 * (TB_SW + TB_GAP);
    (x, TB_SWATCH_Y, x + TB_SW, TB_SWATCH_Y + TB_SW)
}

fn toolbar_click(x: i32, y: i32) {
    let mut t = THEME.get();
    for (i, hue) in SWATCHES.iter().enumerate() {
        let (l, tt, r, b) = swatch_rect(i);
        if x >= l && x < r && y >= tt && y < b {
            t.hue = *hue;
            THEME.set(t);
            save_theme();
            return;
        }
    }
    if x >= TB_TOGGLE_X && x < TB_TOGGLE_X + TB_TOGGLE_W && y >= TB_SWATCH_Y && y < TB_SWATCH_Y + TB_SW {
        t.gear_target = !t.gear_target;
        THEME.set(t);
        save_theme();
    }
}

// ===================== telemetria suavizada =====================

#[derive(Clone, Copy)]
enum Kind {
    Cluster,
    Tires,
    GForce,
}

impl Kind {
    fn size(self) -> (i32, i32) {
        match self {
            Kind::Cluster => (CL_W, CL_H),
            Kind::Tires => (TR_W, TR_H),
            Kind::GForce => (GF_W, GF_H),
        }
    }
    fn default_pos(self, sw: i32, sh: i32) -> (i32, i32) {
        match self {
            Kind::Cluster => ((sw - CL_W) / 2, sh - CL_H - 30),
            Kind::Tires => (40, sh - TR_H - 30),
            Kind::GForce => (sw - GF_W - 40, sh - GF_H - 30),
        }
    }
    fn key(self) -> &'static str {
        match self {
            Kind::Cluster => "cluster",
            Kind::Tires => "tires",
            Kind::GForce => "gforce",
        }
    }
}

struct FrameData {
    rpm_frac: f32,
    near: bool,
    gear: Option<u8>,
    speed: f32,
    accel: f32,
    brake: f32,
    car_name: String,
    power_cv: f32,
    show_power: bool,
    slip: [f32; 4],
    tires: Option<[f32; 4]>,
    lat: f32,
    long: f32,
    fresh: bool,
    alpha: f32,
    accent: D2D1_COLOR_F, // cor do destaque (tema)
    gear_accent: bool,    // shift colore a marcha? (senão, o card)
}

struct Smoother {
    d_rpm: f32,
    d_accel: f32,
    d_brake: f32,
    d_speed: f32,
    d_lat: f32,
    d_long: f32,
    d_slip: [f32; 4],
    d_power: f32,
    last_pkt: u64,
    last_change: Instant,
    last_frame: Instant,
    /// Último max_rpm > 0 — evita piscar quando o jogo zera o campo na troca de carro.
    sticky_max_rpm: f32,
    /// Pico observado sob aceleração forte; usado para aprender redline real de carros com max_rpm errado.
    observed_peak_rpm: f32,
    learned_max_rpm: f32,
    last_raw_rpm: f32,
    last_gear: Option<u8>,
    /// Assinatura do carro (max_rpm + idle_rpm + car_group) para detectar troca.
    car_sig: u64,
    d_visible: f32,
}

fn car_signature(snap: &Telemetry) -> u64 {
    let max_b = if snap.max_rpm > 100.0 { snap.max_rpm.to_bits() as u64 } else { 0 };
    let idle_b = if snap.idle_rpm > 0.0 { snap.idle_rpm.to_bits() as u64 } else { 0 };
    let group = snap.car_group.unwrap_or(0) as u64;
    let ord = if snap.car_ordinal != 0 { snap.car_ordinal as u64 } else { 0 };
    max_b ^ (idle_b << 1) ^ (group << 2) ^ (ord << 3)
}

impl Smoother {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            d_rpm: 0.0,
            d_accel: 0.0,
            d_brake: 0.0,
            d_speed: 0.0,
            d_lat: 0.0,
            d_long: 0.0,
            d_slip: [0.0; 4],
            d_power: 0.0,
            last_pkt: 0,
            last_change: now,
            last_frame: now,
            sticky_max_rpm: 0.0,
            observed_peak_rpm: 0.0,
            learned_max_rpm: 0.0,
            last_raw_rpm: 0.0,
            last_gear: None,
            car_sig: 0,
            d_visible: 0.0,
        }
    }

    fn reset_to(&mut self, t_rpm: f32, accel: f32, brake: f32, speed: f32, lat: f32, long: f32, slip: [f32; 4]) {
        self.d_rpm = t_rpm;
        self.d_accel = accel;
        self.d_brake = brake;
        self.d_speed = speed;
        self.d_lat = lat;
        self.d_long = long;
        self.d_slip = slip;
    }

    fn effective_max_rpm(&self) -> f32 {
        if self.learned_max_rpm > 100.0 {
            self.learned_max_rpm
        } else {
            self.sticky_max_rpm
        }
    }

    fn reset_car_learning(&mut self, rpm: f32, gear: Option<u8>) {
        self.observed_peak_rpm = rpm.max(0.0);
        self.learned_max_rpm = 0.0;
        self.last_raw_rpm = rpm.max(0.0);
        self.last_gear = gear;
    }

    fn commit_observed_peak(&mut self, idle_rpm: f32) {
        let peak = self.observed_peak_rpm;
        let reported = self.sticky_max_rpm;
        if peak <= idle_rpm + LEARN_MIN_OVER_IDLE || reported <= 100.0 {
            return;
        }
        let frac = peak / reported;
        let looks_like_low_rev_car = frac >= LEARN_MIN_REPORTED_FRAC && frac < LEARN_SUSPICIOUS_REPORTED_FRAC;
        if looks_like_low_rev_car {
            // Queremos que o vermelho acenda no pico observado de upshift,
            // não 8% antes dele. Como o alerta usa SHIFT_AT, convertemos o
            // pico em max efetivo: peak == learned_max * SHIFT_AT.
            self.learned_max_rpm = self.learned_max_rpm.max(peak / SHIFT_AT);
        }
    }

    fn learn_effective_redline(&mut self, snap: &Telemetry, fresh: bool) {
        if !fresh {
            return;
        }
        let gear = snap.gear;
        let accel = snap.accel.unwrap_or(0.0);
        let rpm = snap.rpm.max(0.0);
        let upshift = matches!((self.last_gear, gear), (Some(prev), Some(next)) if next > prev && prev > 0);

        if upshift && accel >= LEARN_MIN_THROTTLE {
            self.commit_observed_peak(snap.idle_rpm);
            self.observed_peak_rpm = rpm;
        }
        if accel >= LEARN_MIN_THROTTLE && gear != Some(0) {
            self.observed_peak_rpm = self.observed_peak_rpm.max(rpm);
        }
        self.last_raw_rpm = rpm;
        self.last_gear = gear;
    }

    fn tick(&mut self, snap: &Telemetry, edit: bool) -> FrameData {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if snap._pkt != self.last_pkt {
            self.last_pkt = snap._pkt;
            self.last_change = now;
        }
        let age_s = (now - self.last_change).as_secs_f32();
        let fresh = age_s < STALE_S;
        let driving = edit || visibility::is_driving(snap, age_s);
        let target_vis = if driving { 1.0 } else { 0.0 };
        self.d_visible = exp_smooth(self.d_visible, target_vis, dt, TAU_FADE);

        if snap.max_rpm > 100.0 {
            self.sticky_max_rpm = snap.max_rpm;
        }
        let sig = car_signature(snap);
        if sig != 0 && self.car_sig != 0 && sig != self.car_sig {
            self.reset_car_learning(snap.rpm, snap.gear);
            let max = self.effective_max_rpm();
            let t_rpm = if fresh && max > 0.0 {
                (snap.rpm / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let accel = if fresh { snap.accel.unwrap_or(0.0) / 100.0 } else { 0.0 };
            let brake = if fresh { snap.brake.unwrap_or(0.0) / 100.0 } else { 0.0 };
            let speed = if fresh { snap.speed.unwrap_or(0.0) } else { 0.0 };
            let lat = if fresh { snap.lat_g } else { 0.0 };
            let long = if fresh { snap.long_g } else { 0.0 };
            let slip = if fresh { snap.slip } else { [0.0; 4] };
            self.reset_to(t_rpm, accel, brake, speed, lat, long, slip);
            self.d_power = if fresh { snap.power_hp.unwrap_or(0.0) } else { 0.0 };
        }
        if sig != 0 {
            if self.car_sig == 0 {
                self.reset_car_learning(snap.rpm, snap.gear);
            }
            self.car_sig = sig;
        }
        self.learn_effective_redline(snap, fresh);

        let max = self.effective_max_rpm();
        let t_rpm = if fresh && max > 0.0 {
            (snap.rpm / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.d_rpm = exp_smooth(self.d_rpm, t_rpm, dt, TAU_BAR);
        self.d_accel = exp_smooth(
            self.d_accel,
            if fresh { snap.accel.unwrap_or(0.0) / 100.0 } else { 0.0 },
            dt,
            TAU_BAR,
        );
        self.d_brake = exp_smooth(
            self.d_brake,
            if fresh { snap.brake.unwrap_or(0.0) / 100.0 } else { 0.0 },
            dt,
            TAU_BAR,
        );
        self.d_speed = exp_smooth(
            self.d_speed,
            if fresh { snap.speed.unwrap_or(0.0) } else { 0.0 },
            dt,
            TAU_TEXT,
        );
        self.d_lat = exp_smooth(self.d_lat, if fresh { snap.lat_g } else { 0.0 }, dt, TAU_BAR);
        self.d_long = exp_smooth(self.d_long, if fresh { snap.long_g } else { 0.0 }, dt, TAU_BAR);
        for i in 0..4 {
            self.d_slip[i] = exp_smooth(self.d_slip[i], if fresh { snap.slip[i] } else { 0.0 }, dt, TAU_BAR);
        }
        let power_target = snap.power_hp.unwrap_or(0.0);
        self.d_power = exp_smooth(self.d_power, if fresh { power_target } else { 0.0 }, dt, TAU_TEXT);

        FrameData {
            rpm_frac: self.d_rpm,
            near: self.d_rpm >= SHIFT_AT,
            gear: snap.gear,
            speed: self.d_speed,
            accel: self.d_accel,
            brake: self.d_brake,
            car_name: payload::car_label(snap.car_ordinal, snap.car_name.as_deref()),
            power_cv: self.d_power,
            show_power: snap.power_hp.is_some(),
            slip: self.d_slip,
            tires: if fresh { snap.tires } else { None },
            lat: self.d_lat,
            long: self.d_long,
            fresh,
            alpha: self.d_visible,
            accent: hue_color(THEME.get().hue),
            gear_accent: THEME.get().gear_target,
        }
    }
}

// ===================== GDI / D2D helpers =====================

#[derive(Clone)]
struct Fonts {
    gear: IDWriteTextFormat,
    speed: IDWriteTextFormat,
    small: IDWriteTextFormat,
    small_left: IDWriteTextFormat,
    small_right: IDWriteTextFormat,
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
    glass: ID2D1LinearGradientBrush, // fundo do card (gradiente vertical)
    glass_border: ID2D1SolidColorBrush,
    glass_hi: ID2D1SolidColorBrush,
}

impl Brushes {
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
                D2D1_GRADIENT_STOP { position: 0.0, color: C_GREEN },
                D2D1_GRADIENT_STOP { position: 0.75, color: C_AMBER },
                D2D1_GRADIENT_STOP { position: 1.0, color: C_RED },
            ];
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let grad = rt.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 { X: CL_RIGHT_X, Y: 0.0 },
                    endPoint: Vector2 { X: CL_RIGHT_R, Y: 0.0 },
                },
                None,
                &coll,
            )?;
            // Gradiente vertical do vidro (claro no topo → escuro embaixo).
            let glass_stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: GLASS_TOP },
                D2D1_GRADIENT_STOP { position: 1.0, color: GLASS_BOT },
            ];
            let glass_coll = rt.CreateGradientStopCollection(&glass_stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let glass = rt.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 { X: 0.0, Y: 0.0 },
                    endPoint: Vector2 { X: 0.0, Y: design_h },
                },
                None,
                &glass_coll,
            )?;
            let glass_border = rt.CreateSolidColorBrush(&GLASS_BORDER, None)?;
            let glass_hi = rt.CreateSolidColorBrush(&GLASS_HI, None)?;
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
                glass_border,
                glass_hi,
            })
        }
    }
}

fn make_fonts() -> Result<Fonts> {
    unsafe {
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let make = |size: f32,
                    weight: DWRITE_FONT_WEIGHT,
                    align: DWRITE_TEXT_ALIGNMENT,
                    para: DWRITE_PARAGRAPH_ALIGNMENT|
         -> Result<IDWriteTextFormat> {
            let fmt = dwrite.CreateTextFormat(
                w!("Segoe UI Variable"),
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
        Ok(Fonts {
            gear: make(
                176.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            speed: make(
                32.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            small: make(
                13.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?,
            small_left: make(
                13.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?,
            small_right: make(
                13.0,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
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
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

// ===================== Painel =====================

struct Panel {
    kind: Kind,
    hwnd: HWND,
    memdc: HDC,
    bitmap: HBITMAP,
    pos: POINT,
    size: SIZE,   // tamanho atual (redimensionável)
    design: SIZE, // tamanho-base do layout (conteúdo é escalado p/ `size`)
    dcrt: ID2D1DCRenderTarget,
    rt: ID2D1RenderTarget,
    b: Brushes,
    f: Fonts,
    shown: Cell<bool>,
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
    ) -> Result<Self> {
        let (dw, dh) = kind.size();
        // Mantém a proporção do design (a escala uniforme preenche sem margem);
        // corrige tamanhos salvos fora de proporção.
        let min_scale = MIN_SZ as f32 / (dw.min(dh) as f32);
        let scale = (size.0 as f32 / dw as f32).max(size.1 as f32 / dh as f32).max(min_scale);
        let (w, h) = ((dw as f32 * scale).round() as i32, (dh as f32 * scale).round() as i32);
        let hwnd = make_window(class, instance, pos.0, pos.1, w, h)?;
        unsafe {
            // Guarda a proporção do design para a trava de aspecto no resize.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ((dw as isize) << 16) | (dh as isize));
        }
        let (memdc, bitmap) = make_dib(w, h)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        let b = Brushes::new(&rt, dh as f32)?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(Self {
            kind,
            hwnd,
            memdc,
            bitmap,
            pos: POINT { x: pos.0, y: pos.1 },
            size: SIZE { cx: w, cy: h },
            design: SIZE { cx: dw, cy: dh },
            dcrt,
            rt,
            b,
            f: fonts.clone(),
            shown: Cell::new(false),
        })
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
            self.rt.DrawText(&wide, fmt, &r, brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    /// Superfície de vidro fosco do card (gradiente + borda + brilho interno).
    /// `shift` = borda vermelha (troca de marcha); `edit` = borda ciano (seleção).
    fn draw_card(&self, dw: f32, dh: f32, edit: bool, shift: bool, accent: D2D1_COLOR_F) {
        let outer = rounded(rectf(0.75, 0.75, dw - 0.75, dh - 0.75), GLASS_RADIUS);
        let inner = rounded(rectf(1.75, 1.75, dw - 1.75, dh - 1.75), GLASS_RADIUS - 1.0);
        unsafe {
            if shift {
                // Card inteiro na cor do tema no momento do shift.
                let mut fill = accent;
                fill.a = 0.62;
                self.b.scratch.SetColor(&fill);
                self.rt.FillRoundedRectangle(&outer, &self.b.scratch);
            } else {
                self.rt.FillRoundedRectangle(&outer, &self.b.glass);
            }
            self.rt.DrawRoundedRectangle(&inner, &self.b.glass_hi, 1.0, None); // brilho interno
            if edit {
                self.b.scratch.SetColor(&C_EDIT);
                self.rt.DrawRoundedRectangle(&outer, &self.b.scratch, 2.0, None);
            } else if shift {
                self.b.scratch.SetColor(&accent);
                self.rt.DrawRoundedRectangle(&outer, &self.b.scratch, 2.5, None);
            } else {
                self.rt.DrawRoundedRectangle(&outer, &self.b.glass_border, 1.0, None);
            }
        }
    }

    fn pedal_row_right(&self, label: &str, frac: f32, fill: &ID2D1SolidColorBrush, y: f32) {
        let label_w = 64.0f32;
        let gap = 8.0f32;
        let bar_x = CL_RIGHT_X + label_w + gap;
        let bar_w = CL_RIGHT_R - bar_x;
        let th = CL_PEDAL_H;
        self.text(
            label,
            &self.f.small_left,
            rectf(CL_RIGHT_X, y - 9.0, CL_RIGHT_X + label_w, y + 9.0),
            &self.b.gray,
        );
        let track = rounded(rectf(bar_x, y - th / 2.0, bar_x + bar_w, y + th / 2.0), th / 2.0);
        unsafe {
            self.rt.FillRoundedRectangle(&track, &self.b.track);
            self.rt.DrawRoundedRectangle(&track, &self.b.track_edge, 0.75, None);
            let f = frac.clamp(0.0, 1.0);
            if f > 0.001 {
                self.rt.FillRoundedRectangle(
                    &rounded(rectf(bar_x, y - th / 2.0, bar_x + bar_w * f, y + th / 2.0), th / 2.0),
                    fill,
                );
            }
        }
    }

    fn draw_cluster(&self, f: &FrameData) {
        let gear = match f.gear {
            Some(0) => "R".to_string(),
            Some(g) => g.to_string(),
            None => "–".to_string(),
        };
        // Marcha branca por padrão; cor do tema só se o destaque do shift for a marcha.
        if f.near && f.gear_accent {
            unsafe { self.b.scratch.SetColor(&f.accent) };
        }
        let gb = if f.near && f.gear_accent { &self.b.scratch } else { &self.b.white };
        self.text(
            &gear,
            &self.f.gear,
            rectf(
                CL_BAR_X,
                CL_PAD + CL_GEAR_Y_SHIFT,
                CL_BAR_X + CL_GEAR_W,
                CL_H as f32 - CL_PAD + CL_GEAR_Y_SHIFT,
            ),
            gb,
        );

        unsafe {
            self.rt.FillRoundedRectangle(
                &rounded(rectf(CL_RIGHT_X, CL_RPM_Y, CL_RIGHT_R, CL_RPM_Y + CL_BAR_H), CL_BAR_H / 2.0),
                &self.b.track,
            );
            let rpm_track = rounded(rectf(CL_RIGHT_X, CL_RPM_Y, CL_RIGHT_R, CL_RPM_Y + CL_BAR_H), CL_BAR_H / 2.0);
            self.rt.DrawRoundedRectangle(&rpm_track, &self.b.track_edge, 0.75, None);
            if f.rpm_frac > 0.001 {
                let rw = (CL_RIGHT_R - CL_RIGHT_X) * f.rpm_frac;
                self.rt.FillRoundedRectangle(
                    &rounded(rectf(CL_RIGHT_X, CL_RPM_Y, CL_RIGHT_X + rw, CL_RPM_Y + CL_BAR_H), CL_BAR_H / 2.0),
                    &self.b.grad,
                );
            }
        }

        self.text(
            &f.car_name,
            &self.f.small_left,
            rectf(CL_RIGHT_X, CL_META_Y - 10.0, CL_RIGHT_R - 90.0, CL_META_Y + 24.0),
            &self.b.gray,
        );
        if f.show_power {
            let power = format!("{} hp", f.power_cv.round() as i32);
            self.text(
                &power,
                &self.f.small_right,
                rectf(CL_RIGHT_R - 130.0, CL_META_Y - 10.0, CL_RIGHT_R, CL_META_Y + 10.0),
                &self.b.white,
            );
        }

        let sp = (f.speed.round() as i32).to_string();
        self.text(
            &sp,
            &self.f.speed,
            rectf(CL_RIGHT_X, CL_SPEED_Y - 16.0, CL_RIGHT_X + 120.0, CL_SPEED_Y + 16.0),
            &self.b.white,
        );
        self.text(
            "km/h",
            &self.f.small_left,
            rectf(CL_RIGHT_X + 122.0, CL_SPEED_Y - 8.0, CL_RIGHT_X + 170.0, CL_SPEED_Y + 12.0),
            &self.b.gray,
        );

        self.pedal_row_right("BRAKE", f.brake, &self.b.brake, CL_BRAKE_Y);
        self.pedal_row_right("THROTTLE", f.accel, &self.b.accel, CL_ACCEL_Y);
    }

    fn draw_tires(&self, f: &FrameData) {
        self.text("TIRES", &self.f.small, rectf(0.0, 8.0, TR_W as f32, 30.0), &self.b.gray);
        let cells = [(16.0, 40.0), (108.0, 40.0), (16.0, 118.0), (108.0, 118.0)];
        let (cw, ch) = (86.0f32, 66.0f32);
        for i in 0..4 {
            let (x, y) = cells[i];
            let cell = rounded(rectf(x, y, x + cw, y + ch), 8.0);
            unsafe {
                self.rt.FillRoundedRectangle(&cell, &self.b.track);
                self.b.scratch.SetColor(&slip_color(f.slip[i]));
                self.rt.FillRoundedRectangle(&cell, &self.b.scratch);
            }
            let label = match (f.tires, f.fresh) {
                (Some(t), true) => format!("{:.0}", t[i]),
                _ => "--".to_string(),
            };
            self.text(&label, &self.f.small, rectf(x, y, x + cw, y + ch), &self.b.white);
        }
    }

    fn draw_gforce(&self, f: &FrameData) {
        let (cxg, cyg, r) = (GF_W as f32 / 2.0, 120.0f32, 80.0f32);
        self.text("G", &self.f.small, rectf(0.0, 8.0, GF_W as f32, 30.0), &self.b.gray);
        unsafe {
            let outer = D2D1_ELLIPSE { point: Vector2 { X: cxg, Y: cyg }, radiusX: r, radiusY: r };
            self.rt.DrawEllipse(&outer, &self.b.gray, 1.5, None);
            let inner = D2D1_ELLIPSE { point: Vector2 { X: cxg, Y: cyg }, radiusX: r * 0.5, radiusY: r * 0.5 };
            self.rt.DrawEllipse(&inner, &self.b.track, 1.0, None);
            self.rt.DrawLine(Vector2 { X: cxg - r, Y: cyg }, Vector2 { X: cxg + r, Y: cyg }, &self.b.track, 1.0, None);
            self.rt.DrawLine(Vector2 { X: cxg, Y: cyg - r }, Vector2 { X: cxg, Y: cyg + r }, &self.b.track, 1.0, None);
            let scale = r / 1.5;
            let mut dx = f.lat * scale;
            let mut dy = -f.long * scale;
            let mag = (dx * dx + dy * dy).sqrt();
            if mag > r {
                dx *= r / mag;
                dy *= r / mag;
            }
            let dot = D2D1_ELLIPSE { point: Vector2 { X: cxg + dx, Y: cyg + dy }, radiusX: 7.0, radiusY: 7.0 };
            self.rt.FillEllipse(&dot, &self.b.white);
        }
        let txt = format!("{:.1}g  {:.1}g", f.lat, f.long);
        self.text(&txt, &self.f.small, rectf(0.0, cyg + r + 6.0, GF_W as f32, cyg + r + 32.0), &self.b.gray);
    }

    fn draw(&self, f: &FrameData, edit: bool) -> Result<()> {
        let visible = edit || f.alpha > 0.01;
        self.set_shown(visible);
        if !visible {
            return Ok(());
        }

        let (sw, sh) = (self.size.cx as f32, self.size.cy as f32);
        let (dw, dh) = (self.design.cx as f32, self.design.cy as f32);
        let s = (sw / dw).min(sh / dh);
        let content = Matrix3x2 {
            M11: s,
            M12: 0.0,
            M21: 0.0,
            M22: s,
            M31: (sw - dw * s) / 2.0,
            M32: (sh - dh * s) / 2.0,
        };
        let layer_alpha = if edit { 1.0 } else { f.alpha.clamp(0.0, 1.0) };
        let shift = f.near && matches!(self.kind, Kind::Cluster);
        let card_shift = shift && !f.gear_accent;
        unsafe {
            let full = RECT { left: 0, top: 0, right: self.size.cx, bottom: self.size.cy };
            self.dcrt.BindDC(self.memdc, &full)?;
            self.rt.BeginDraw();
            self.rt.Clear(Some(&color(0.0, 0.0, 0.0, 0.0)));

            // Modo edição: fundo de captura quase invisível na janela INTEIRA, para
            // todo pixel (bordas/cantos/margens) receber mouse → cursores e resize
            // funcionam mesmo em cards pequenos ou de proporção diferente.
            if edit {
                self.rt.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 });
                self.b.scratch.SetColor(&color(0.55, 0.6, 0.7, 0.06));
                self.rt.FillRectangle(&rectf(0.0, 0.0, sw, sh), &self.b.scratch);
            }

            // Vidro fosco do card (sob a mesma transformação do conteúdo).
            self.rt.SetTransform(&content);
            self.draw_card(dw, dh, edit, card_shift, f.accent);
            match self.kind {
                Kind::Cluster => self.draw_cluster(f),
                Kind::Tires => self.draw_tires(f),
                Kind::GForce => self.draw_gforce(f),
            }

            self.rt.EndDraw(None, None)?;
        }
        let alpha = if edit { 255 } else { (layer_alpha * 255.0).round() as u8 };
        present(self.hwnd, self.memdc, self.pos, self.size, alpha)
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
    fn new(class: PCWSTR, instance: HINSTANCE, factory: &ID2D1Factory, sw: i32, sh: i32) -> Result<Self> {
        let hwnd = make_window(class, instance, 0, 0, sw, sh)?;
        let (memdc, bitmap) = make_dib(sw, sh)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        let dim = unsafe { rt.CreateSolidColorBrush(&color(0.0, 0.0, 0.0, 0.35), None)? };
        let line = unsafe { rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 0.12), None)? };
        Ok(Self { hwnd, memdc, bitmap, size: SIZE { cx: sw, cy: sh }, dcrt, rt, dim, line })
    }

    fn draw(&self) -> Result<()> {
        let (w, h) = (self.size.cx as f32, self.size.cy as f32);
        unsafe {
            let full = RECT { left: 0, top: 0, right: self.size.cx, bottom: self.size.cy };
            self.dcrt.BindDC(self.memdc, &full)?;
            self.rt.BeginDraw();
            self.rt.Clear(Some(&color(0.0, 0.0, 0.0, 0.0)));
            self.rt.FillRectangle(&rectf(0.0, 0.0, w, h), &self.dim);
            let mut x = 0;
            while x <= self.size.cx {
                self.rt.DrawLine(Vector2 { X: x as f32, Y: 0.0 }, Vector2 { X: x as f32, Y: h }, &self.line, 1.0, None);
                x += GRID;
            }
            let mut y = 0;
            while y <= self.size.cy {
                self.rt.DrawLine(Vector2 { X: 0.0, Y: y as f32 }, Vector2 { X: w, Y: y as f32 }, &self.line, 1.0, None);
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
    scratch: ID2D1SolidColorBrush,
    font: IDWriteTextFormat,
}

impl Toolbar {
    fn new(class: PCWSTR, instance: HINSTANCE, factory: &ID2D1Factory, fonts: &Fonts, sw: i32) -> Result<Self> {
        let pos = POINT { x: (sw - TB_W) / 2, y: 24 };
        let size = SIZE { cx: TB_W, cy: TB_H };
        let hwnd = make_window(class, instance, pos.x, pos.y, size.cx, size.cy)?;
        set_interactive(hwnd, true);
        let (memdc, bitmap) = make_dib(size.cx, size.cy)?;
        let dcrt = make_dcrt(factory)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        let bg = unsafe { rt.CreateSolidColorBrush(&color(0.055, 0.065, 0.085, 0.82), None)? };
        let edge = unsafe { rt.CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 0.18), None)? };
        let text_brush = unsafe { rt.CreateSolidColorBrush(&C_WHITE, None)? };
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
            scratch,
            font: fonts.small.clone(),
        })
    }

    fn text(&self, s: &str, r: D2D_RECT_F) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.rt.DrawText(&wide, &self.font, &r, &self.text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    fn draw(&self) -> Result<()> {
        let t = THEME.get();
        unsafe {
            let full = RECT { left: 0, top: 0, right: self.size.cx, bottom: self.size.cy };
            self.dcrt.BindDC(self.memdc, &full)?;
            self.rt.BeginDraw();
            self.rt.Clear(Some(&color(0.0, 0.0, 0.0, 0.0)));

            let card = rounded(rectf(0.75, 0.75, self.size.cx as f32 - 0.75, self.size.cy as f32 - 0.75), 14.0);
            self.rt.FillRoundedRectangle(&card, &self.bg);
            self.rt.DrawRoundedRectangle(&card, &self.edge, 1.0, None);

            for (i, hue) in SWATCHES.iter().enumerate() {
                let (l, tt, r, b) = swatch_rect(i);
                let swatch = rounded(rectf(l as f32, tt as f32, r as f32, b as f32), 7.0);
                self.scratch.SetColor(&hue_color(*hue));
                self.rt.FillRoundedRectangle(&swatch, &self.scratch);
                if (t.hue - *hue).abs() < 0.5 {
                    self.scratch.SetColor(&C_WHITE);
                    self.rt.DrawRoundedRectangle(&swatch, &self.scratch, 2.0, None);
                } else {
                    self.rt.DrawRoundedRectangle(&swatch, &self.edge, 1.0, None);
                }
            }

            let toggle = rounded(
                rectf(
                    TB_TOGGLE_X as f32,
                    TB_SWATCH_Y as f32,
                    (TB_TOGGLE_X + TB_TOGGLE_W) as f32,
                    (TB_SWATCH_Y + TB_SW) as f32,
                ),
                8.0,
            );
            let mut accent = hue_color(t.hue);
            accent.a = 0.24;
            self.scratch.SetColor(&accent);
            self.rt.FillRoundedRectangle(&toggle, &self.scratch);
            self.scratch.SetColor(&hue_color(t.hue));
            self.rt.DrawRoundedRectangle(&toggle, &self.scratch, 1.8, None);
            self.text(
                if t.gear_target { "GEAR" } else { "CARD" },
                rectf(TB_TOGGLE_X as f32, TB_SWATCH_Y as f32 + 5.0, (TB_TOGGLE_X + TB_TOGGLE_W) as f32, (TB_SWATCH_Y + TB_SW) as f32),
            );

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

fn layout_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("forza-hud-native").join("layout.txt")
}

/// "key=x,y,w,h" por linha.
fn load_layout() -> HashMap<String, (i32, i32, i32, i32)> {
    let mut map = HashMap::new();
    if let Ok(text) = fs::read_to_string(layout_path()) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let nums: Vec<i32> = v.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                if nums.len() == 4 {
                    map.insert(k.trim().to_string(), (nums[0], nums[1], nums[2], nums[3]));
                }
            }
        }
    }
    map
}

fn save_layout(panels: &[Panel]) {
    let mut s = String::new();
    for p in panels {
        s.push_str(&format!("{}={},{},{},{}\n", p.kind.key(), p.pos.x, p.pos.y, p.size.cx, p.size.cy));
    }
    let path = layout_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, s);
}

fn theme_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("forza-hud-native").join("theme.txt")
}

fn load_theme() -> Theme {
    let mut t = Theme { hue: 4.0, gear_target: false };
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
                    _ => {}
                }
            }
        }
    }
    t
}

fn save_theme() {
    let t = THEME.get();
    let s = format!("hue={}\ntarget={}\n", t.hue, if t.gear_target { "gear" } else { "card" });
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
            anchor: POINT { x: r.left, y: r.top },
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
        let _ = SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
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
    let left = if g.from_right { g.anchor.x - nw } else { g.anchor.x };
    let top = if g.from_bottom { g.anchor.y - nh } else { g.anchor.y };
    let _ = SetWindowPos(hwnd, None, left, top, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE);
}

unsafe fn grab_end() {
    let _ = ReleaseCapture();
    GRAB.set(None);
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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

unsafe extern "system" fn backdrop_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize), // a malha nunca pega clique
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn toolbar_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lparam);
            toolbar_click(x, y);
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
        for p in panels.iter_mut() {
            sync_panel_geometry(p);
        }
        toolbar.hide();
        backdrop.hide();
        save_layout(panels);
        save_theme();
    }
}

/// Lê pos/tamanho atuais da janela (já encaixados na malha durante o arraste)
/// e recria o DIB se o tamanho mudou.
fn sync_panel_geometry(p: &mut Panel) {
    let r = window_rect(p.hwnd);
    p.pos = POINT { x: r.left, y: r.top };
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

        let mut panels: Vec<Panel> = Vec::new();
        for kind in [Kind::Cluster, Kind::Tires, Kind::GForce] {
            let (dw, dh) = kind.size();
            let (x, y, w, h) = saved.get(kind.key()).copied().unwrap_or_else(|| {
                let (dx, dy) = kind.default_pos(sw, sh);
                (dx, dy, dw, dh)
            });
            panels.push(Panel::new(
                kind,
                panel_class,
                instance,
                &factory,
                &fonts,
                (x, y),
                (w.max(MIN_SZ), h.max(MIN_SZ)),
            )?);
        }
        let backdrop = Backdrop::new(backdrop_class, instance, &factory, sw, sh)?;
        let toolbar = Toolbar::new(toolbar_class, instance, &factory, &fonts, sw)?;

        let mut smoother = Smoother::new();

        let shared = Arc::new(Shared::new());
        let port = std::env::var("FORZA_UDP_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(UDP_PORT);
        match udp::spawn(shared.clone(), port, |_| {}) {
            Ok(_) => println!("[overlay] ouvindo UDP em 0.0.0.0:{port} | Ctrl+Alt+E editar | Ctrl+Alt+Q sair"),
            Err(e) => eprintln!("[overlay] falha ao abrir UDP {port}: {e}"),
        }

        let _ = RegisterHotKey(None, HK_QUIT, MOD_CONTROL | MOD_ALT, 0x51 /* Q */);
        let _ = RegisterHotKey(None, HK_EDIT, MOD_CONTROL | MOD_ALT, 0x45 /* E */);

        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    let _ = UnregisterHotKey(None, HK_QUIT);
                    let _ = UnregisterHotKey(None, HK_EDIT);
                    return Ok(());
                }
                if msg.message == WM_HOTKEY {
                    match msg.wParam.0 as i32 {
                        HK_QUIT => {
                            save_theme();
                            let _ = UnregisterHotKey(None, HK_QUIT);
                            let _ = UnregisterHotKey(None, HK_EDIT);
                            return Ok(());
                        }
                        HK_EDIT => toggle_edit(&mut panels, &backdrop, &toolbar),
                        _ => {}
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let snap = shared.latest();
            let edit = EDIT.load(Ordering::Relaxed);
            let fd = smoother.tick(&snap, edit);
            for p in panels.iter_mut() {
                if edit {
                    sync_panel_geometry(p); // segue arraste/resize do OS
                }
                if let Err(e) = p.draw(&fd, edit) {
                    eprintln!("[overlay] draw: {e:?}");
                }
            }
            if edit {
                let _ = toolbar.draw();
                bring_to_top(toolbar.hwnd);
            }
            let _ = DwmFlush();
        }
    }
}
