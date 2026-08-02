//! Edit mode: dragging, resizing, and snapping panels.
//!
//! Resizing scales the panel from the corner opposite the grabbed edge and
//! preserves the design aspect ratio, so a card never distorts — only its scale
//! changes. The anchor is captured once when the drag begins; deriving it every
//! mouse move would let rounding drift the panel across a long drag.
//!
//! Positions snap to the [`super::geometry::GRID`] grid, but only within
//! [`super::geometry::MAGNET`] pixels, so placement stays free elsewhere.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::geometry::{soft_snap, MIN_SZ, RESIZE_MARGIN};
use super::window::{design_of, window_rect};

// A manual move or resize in progress, owned by the UI thread. Resizing scales
// away from the fixed corner (`anchor`) toward the cursor, preserving the design
// aspect ratio.
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

/// What the cursor is over: the centre moves the panel, edges and corners
/// resize it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Zone {
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

/// Zona sob o cursor (coords de tela): centro = mover; bordas/cantos = resize.
pub(crate) unsafe fn zone_at(hwnd: HWND, sx: i32, sy: i32) -> Zone {
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
pub(crate) unsafe fn apply_cursor(hwnd: HWND) {
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

pub(crate) unsafe fn grab_begin(hwnd: HWND) {
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

pub(crate) unsafe fn grab_move(hwnd: HWND) {
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

pub(crate) unsafe fn grab_end() {
    let _ = ReleaseCapture();
    GRAB.set(None);
}
