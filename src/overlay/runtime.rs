//! Application lifecycle and the render loop.
//!
//! [`run`] registers the window classes, restores the saved layout and theme,
//! creates the panels, starts the telemetry receiver and the critical throttle
//! thread, installs the tray icon and the global hotkeys, and then owns the main
//! message and render loop until the user quits.
//!
//! The window procedures live here too, because they are the bridge between
//! Win32 messages and the modules that handle them: [`super::editing`] for drag
//! and resize, [`super::panels::toolbar`] for the edit toolbar, and
//! [`super::tray`] for the tray menu.
//!
//! # Loop invariants
//!
//! * No allocation and no logging on the per-frame path. Failures that can recur
//!   every frame are latched — see [`crate::diagnostics::Once`].
//! * The cluster is never rate-limited; only auxiliary panels degrade, and only
//!   under measured compositor pressure ([`super::frame_pacing`]).
//! * When nothing is animating and the HUD has faded out, the loop sleeps rather
//!   than spinning, so an idle overlay costs almost nothing.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::diagnostics::{log_error, log_info, log_warn};
use crate::localization::text;
use crate::overlay::track_recording::TrackRecording;
use crate::telemetry::display::DisplaySmoother;
use crate::telemetry::receiver;
use crate::telemetry::snapshot::Shared;
use crate::telemetry::visibility;

use super::editing::{apply_cursor, grab_begin, grab_end, grab_move};
use super::frame_pacing::{RenderGovernor, FRAME_MIN_MS, IDLE_SLEEP_MS};
use super::geometry::{lparam_xy, Kind, MIN_SZ, PANEL_COUNT, PANEL_KINDS};
use super::graphics::make_fonts;
use super::panels::backdrop::Backdrop;
use super::panels::throttle::{spawn_critical_throttle, throttle_placement};
use super::panels::toolbar::{
    toolbar_activate, toolbar_hit, toolbar_slider_drag, Toolbar, HIT_GLASS_ALPHA, HIT_GLASS_TONE,
    HIT_NONE,
};
use super::panels::Panel;
use super::settings::{self, LayoutEntry};
use super::theme::{hue_color, THEME};
use super::tray::{show_tray_menu, wide_buf, TRAY_TOGGLE_HIDE, WM_TRAYICON};
use super::window::{bring_to_top, set_interactive, window_rect};

/// Forza's default "Data Out" port, and the value the documentation tells users
/// to type into the game. Overridable with [`UDP_PORT_VAR`].
const UDP_PORT: u16 = 20066;
/// Environment variable overriding [`UDP_PORT`]. Part of the public contract —
/// see `docs/reference/environment_variables.md`.
const UDP_PORT_VAR: &str = "FORZA_UDP_PORT";
const HK_QUIT: i32 = 1;
const HK_EDIT: i32 = 2;
const HK_HIDE: i32 = 3;

/// Latch for a draw failure that would otherwise recur every frame — see
/// [`crate::diagnostics::Once`].
static PANEL_DRAW_FAILED: crate::diagnostics::Once = crate::diagnostics::Once::new();

pub(crate) static EDIT: AtomicBool = AtomicBool::new(false);
pub(crate) static HUD_HIDDEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    pub(crate) static PANEL_VISIBLE: Cell<[bool; PANEL_COUNT]> = const { Cell::new([true; PANEL_COUNT]) };
    pub(crate) static TOOLBAR_DOWN: Cell<i32> = const { Cell::new(HIT_NONE) };
}

// --- Edit toolbar ---

// ===================== GDI / D2D helpers =====================

// ===================== Panel =====================

// ===================== Grid (edit-mode background) =====================

// ===================== Edit toolbar =====================

// ===================== Layout persistence =====================

// ===================== window procs =====================

pub(crate) unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if EDIT.load(Ordering::Relaxed) {
        match msg {
            // The whole surface is client area. Manual cursor, drag, and resize
            // avoid the OS modal loop so content scales live during adjustment.
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
                // The mouse wheel rotates the accent hue.
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let mut t = THEME.get();
                t.hue = (t.hue + (delta as f32 / 120.0) * 12.0).rem_euclid(360.0);
                THEME.set(t);
                return LRESULT(0);
            }
            WM_RBUTTONDOWN => {
                // Right-click switches whether shift color targets the gear or card.
                let mut t = THEME.get();
                t.gear_target = !t.gear_target;
                THEME.set(t);
                settings::save_theme();
                return LRESULT(0);
            }
            _ => {}
        }
    } else if msg == WM_NCHITTEST {
        // Outside edit mode, hit testing is transparent so clicks reach the game.
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

unsafe extern "system" fn tray_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TRAYICON {
        match lparam.0 as u32 {
            WM_RBUTTONUP | WM_CONTEXTMENU => show_tray_menu(hwnd),
            WM_LBUTTONDBLCLK => TRAY_TOGGLE_HIDE.store(true, Ordering::Relaxed),
            _ => {}
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
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
            // Continuously drag the glass sliders while capture is active.
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
                settings::save_theme();
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
            bring_to_top(p.hwnd); // panels above the grid
        }
        toolbar.show();
    } else {
        save_current_layout(panels);
        toolbar.hide();
        backdrop.hide();
        settings::save_theme();
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
    let entries: Vec<(Kind, LayoutEntry)> = panels
        .iter()
        .map(|panel| {
            (
                panel.kind,
                LayoutEntry {
                    x: panel.pos.x,
                    y: panel.pos.y,
                    w: panel.size.cx,
                    h: panel.size.cy,
                    visible: panel.enabled,
                },
            )
        })
        .collect();
    settings::save_layout(&entries);
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

/// Read the window's current position and size after drag-time grid snapping,
/// and recreate the DIB if its size changed.
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
    crate::telemetry::vehicle_catalog::init();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);

        let panel_class = w!("ForzaPanelClass");
        let backdrop_class = w!("ForzaBackdropClass");
        let toolbar_class = w!("ForzaToolbarClass");
        let tray_class = w!("VeloryftTrayClass");
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
        RegisterClassW(&WNDCLASSW {
            hInstance: instance,
            lpszClassName: tray_class,
            lpfnWndProc: Some(tray_proc),
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        });

        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);

        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let fonts = make_fonts()?;
        let saved = settings::load_layout();
        THEME.set(settings::load_theme());
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
        let mut render_governor = RenderGovernor::new();
        let mut last_frame = Instant::now();

        let shared = Arc::new(Shared::new());
        let initial_throttle = panels
            .iter()
            .find(|panel| panel.kind == Kind::Cluster)
            .map(throttle_placement)
            .expect("cluster panel is missing");
        let throttle_placement_state = Arc::new(Mutex::new(initial_throttle));
        let _throttle_thread =
            spawn_critical_throttle(shared.clone(), throttle_placement_state.clone());

        // Track mapping (world map recorder + loopback viewer API). Compiled out
        // unless the `track_mapping` feature is enabled, in which case this is
        // `None` and nothing below records anything.
        let track = TrackRecording::start();

        let port = std::env::var(UDP_PORT_VAR)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(UDP_PORT);
        let recording = track.clone();
        match receiver::spawn(shared.clone(), port, move |frame| {
            if let Some(ref recording) = recording {
                recording.ingest(frame, visibility::is_driving(frame, 0.0));
            }
        }) {
            Ok(_) => log_info!("listening for Forza telemetry on 0.0.0.0:{port}"),
            Err(cause) => crate::diagnostics::report_startup_failure(
                &crate::localization::udp_bind_failure(port, UDP_PORT_VAR),
                &format_args!("binding 0.0.0.0:{port} failed: {cause}"),
            ),
        }

        // The tray uses an invisible message-only window to receive icon clicks.
        // Windows initially files the icon under hidden icons until the user pins it.
        let app_name = wide_buf::<64>(text().app_name);
        let tray_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            tray_class,
            PCWSTR(app_name.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )?;
        // MAKEINTRESOURCE(1): the icon embedded by build.rs. The Win32 API reads
        // this as an ordinal, never dereferences it, so it is not a real pointer.
        let icon_ordinal = PCWSTR(std::ptr::without_provenance(1));
        let tray_icon = match LoadIconW(Some(instance), icon_ordinal) {
            Ok(icon) => icon,
            Err(e) => {
                log_warn!("tray: LoadIconW failed, falling back to no icon: {e:?}");
                HICON::default()
            }
        };
        let tray_nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: tray_hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: tray_icon,
            szTip: wide_buf(text().app_name),
            ..Default::default()
        };
        if !Shell_NotifyIconW(NIM_ADD, &tray_nid).as_bool() {
            log_warn!("tray: Shell_NotifyIconW(NIM_ADD) failed; no tray icon this session");
        }

        let _ = RegisterHotKey(None, HK_QUIT, MOD_CONTROL | MOD_ALT, 0x51 /* Q */);
        let _ = RegisterHotKey(None, HK_EDIT, MOD_CONTROL | MOD_ALT, 0x45 /* E */);
        let _ = RegisterHotKey(None, HK_HIDE, MOD_CONTROL | MOD_ALT, 0x48 /* H */);

        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    save_current_layout(&mut panels);
                    settings::save_theme();
                    smoother.persist();
                    if let Some(ref track) = track {
                        // Compact on exit: full JSON snapshot plus truncated
                        // incremental logs, so the next start is fast.
                        track.compact();
                    }
                    let _ = Shell_NotifyIconW(NIM_DELETE, &tray_nid);
                    let _ = UnregisterHotKey(None, HK_QUIT);
                    let _ = UnregisterHotKey(None, HK_EDIT);
                    let _ = UnregisterHotKey(None, HK_HIDE);
                    return Ok(());
                }
                if msg.message == WM_HOTKEY {
                    match msg.wParam.0 as i32 {
                        HK_QUIT => {
                            save_current_layout(&mut panels);
                            settings::save_theme();
                            smoother.persist();
                            if let Some(ref track) = track {
                                track.save();
                            }
                            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_nid);
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

            if TRAY_TOGGLE_HIDE.swap(false, Ordering::Relaxed) {
                toggle_hud_hidden(&mut panels, &backdrop, &toolbar);
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
            let render_now = Instant::now();
            for p in panels.iter_mut() {
                if edit {
                    sync_panel_geometry(p);
                }
                let allow_present = render_governor.should_present(p.kind, render_now, edit);
                match p.draw(&fd, &elev, edit, accent, theme.gear_target, allow_present) {
                    Ok(Some(present_time)) => {
                        presented = true;
                        render_governor.record(p.kind, present_time, Instant::now());
                    }
                    Ok(None) => {}
                    Err(e) => {
                        if PANEL_DRAW_FAILED.first() {
                            log_error!("panel draw failed: {e:?} (repeats suppressed)");
                        }
                    }
                }
            }
            if let Some(cluster) = panels.iter().find(|panel| panel.kind == Kind::Cluster) {
                *throttle_placement_state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = throttle_placement(cluster);
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
