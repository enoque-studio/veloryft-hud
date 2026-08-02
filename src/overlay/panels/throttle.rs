//! The critical throttle bar.
//!
//! Throttle input is the fastest-changing thing on the HUD and the one a driver
//! reacts to, so it does not share the main render loop. It owns a separate
//! layered surface driven by a dedicated thread at `THROTTLE_FRAME_MS`
//! (125 Hz) running at `THREAD_PRIORITY_HIGHEST` — high, deliberately not
//! realtime: the work per tick is short and the thread sleeps between frames,
//! so it never competes with the game for CPU.
//!
//! [`ThrottlePlacement`] is the only state shared with the main loop. The
//! render loop republishes it whenever the cluster moves; the throttle thread
//! reads it once per frame and reshapes its window to match.
//!
//! Do not add allocation, locking beyond the existing placement mutex, or
//! logging to the per-frame path here — see
//! `docs/architecture/rendering_pipeline.md`.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::Panel;
use crate::diagnostics::{log_error, log_warn, Once};
use crate::overlay::frame_pacing::{THROTTLE_FRAME_MS, THROTTLE_TAU_ATTACK, THROTTLE_TAU_RELEASE};
use crate::overlay::geometry::{
    rectf, rounded, CL_ACCEL_Y, CL_H, CL_PEDAL_H, CL_RIGHT_X, CL_W, MIN_SZ, TH_H, TH_W,
};
use crate::overlay::graphics::{make_dcrt_with_type, make_fonts, Brushes, Fonts};
use crate::overlay::runtime::{panel_proc, EDIT, HUD_HIDDEN};
use crate::overlay::theme::color;
use crate::overlay::window::{bring_to_top, create_dib_bitmap, make_dib, make_window, present};
use crate::telemetry::display;
use crate::telemetry::snapshot::Shared;
use crate::telemetry::visibility;

/// Latches for failures that would otherwise repeat every frame.
static THROTTLE_DRAW_FAILED: Once = Once::new();
static THROTTLE_PLACEMENT_FAILED: Once = Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ThrottlePlacement {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

pub(crate) fn throttle_placement(cluster: &Panel) -> ThrottlePlacement {
    let sx = cluster.size.cx as f32 / CL_W as f32;
    let sy = cluster.size.cy as f32 / CL_H as f32;
    ThrottlePlacement {
        x: cluster.pos.x + (CL_RIGHT_X * sx).round() as i32,
        y: cluster.pos.y + ((CL_ACCEL_Y - TH_H as f32 / 2.0) * sy).round() as i32,
        w: (TH_W as f32 * sx).round().max(MIN_SZ as f32) as i32,
        h: (TH_H as f32 * sy).round().max(16.0) as i32,
    }
}

/// Minimal independent throttle surface. Rasterization is forced to software;
/// only the small final bitmap crosses the DWM boundary.
struct CriticalThrottleSurface {
    hwnd: HWND,
    memdc: HDC,
    bitmap: HBITMAP,
    dcrt: ID2D1DCRenderTarget,
    rt: ID2D1RenderTarget,
    brushes: Brushes,
    fonts: Fonts,
    label: Vec<u16>,
    placement: ThrottlePlacement,
    shown: bool,
    paint_key: u64,
}

impl CriticalThrottleSurface {
    fn new(class: PCWSTR, instance: HINSTANCE, placement: ThrottlePlacement) -> Result<Self> {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let fonts = make_fonts()?;
        let hwnd = make_window(
            class,
            instance,
            placement.x,
            placement.y,
            placement.w,
            placement.h,
        )?;
        let (memdc, bitmap) = make_dib(placement.w, placement.h)?;
        let dcrt = make_dcrt_with_type(&factory, D2D1_RENDER_TARGET_TYPE_SOFTWARE)?;
        let rt: ID2D1RenderTarget = dcrt.cast()?;
        unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
        let brushes = Brushes::new(&rt, TH_H as f32)?;
        let label = crate::localization::text()
            .throttle
            .encode_utf16()
            .collect();
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        Ok(Self {
            hwnd,
            memdc,
            bitmap,
            dcrt,
            rt,
            brushes,
            fonts,
            label,
            placement,
            shown: false,
            paint_key: 0,
        })
    }

    fn set_shown(&mut self, shown: bool) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        unsafe {
            let _ = ShowWindow(self.hwnd, if shown { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        if shown {
            bring_to_top(self.hwnd);
        }
    }

    fn sync_placement(&mut self, placement: ThrottlePlacement) -> Result<()> {
        if placement == self.placement {
            return Ok(());
        }
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                placement.x,
                placement.y,
                placement.w,
                placement.h,
                SWP_NOACTIVATE,
            )?;
        }
        if placement.w != self.placement.w || placement.h != self.placement.h {
            let new_bitmap = create_dib_bitmap(self.memdc, placement.w, placement.h)?;
            unsafe {
                let old = SelectObject(self.memdc, HGDIOBJ(new_bitmap.0));
                let _ = DeleteObject(old);
            }
            self.bitmap = new_bitmap;
        }
        self.placement = placement;
        self.paint_key = 0;
        Ok(())
    }

    fn draw(&mut self, accel: f32, alpha: f32, visible: bool) -> Result<()> {
        self.set_shown(visible);
        if !visible {
            self.paint_key = 0;
            return Ok(());
        }
        let key = ((accel.clamp(0.0, 1.0) * 1024.0).round() as u64)
            | (((alpha.clamp(0.0, 1.0) * 255.0).round() as u64) << 16);
        if key == self.paint_key {
            return Ok(());
        }
        self.paint_key = key;

        let w = self.placement.w as f32;
        let h = self.placement.h as f32;
        let sx = w / TH_W as f32;
        let label_w = 64.0 * sx;
        let gap = 8.0 * sx;
        let bar_x = label_w + gap;
        let bar_w = w - bar_x;
        let thickness = (CL_PEDAL_H * sx).max(2.0);
        let y = h / 2.0;
        let full = RECT {
            left: 0,
            top: 0,
            right: self.placement.w,
            bottom: self.placement.h,
        };
        unsafe {
            self.dcrt.BindDC(self.memdc, &full)?;
            self.rt.BeginDraw();
            self.rt.Clear(Some(&color(0.0, 0.0, 0.0, 0.0)));
            self.rt.DrawText(
                &self.label,
                &self.fonts.small_left,
                &rectf(0.0, 0.0, label_w, h),
                &self.brushes.gray,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let track = rounded(
                rectf(
                    bar_x,
                    y - thickness / 2.0,
                    bar_x + bar_w,
                    y + thickness / 2.0,
                ),
                thickness / 2.0,
            );
            self.rt.FillRoundedRectangle(&track, &self.brushes.track);
            self.rt
                .DrawRoundedRectangle(&track, &self.brushes.track_edge, 0.75, None);
            let filled = bar_w * accel.clamp(0.0, 1.0);
            if filled > 0.5 {
                self.rt.FillRoundedRectangle(
                    &rounded(
                        rectf(
                            bar_x,
                            y - thickness / 2.0,
                            bar_x + filled,
                            y + thickness / 2.0,
                        ),
                        thickness / 2.0,
                    ),
                    &self.brushes.accel,
                );
            }
            self.rt.EndDraw(None, None)?;
        }
        present(
            self.hwnd,
            self.memdc,
            POINT {
                x: self.placement.x,
                y: self.placement.y,
            },
            SIZE {
                cx: self.placement.w,
                cy: self.placement.h,
            },
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }
}

impl Drop for CriticalThrottleSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memdc);
        }
    }
}

pub(crate) fn spawn_critical_throttle(
    shared: Arc<Shared>,
    placement: Arc<Mutex<ThrottlePlacement>>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("critical-throttle-d2d".into())
        .spawn(move || unsafe {
            // High priority, not realtime: this thread does brief work and blocks
            // between frames, avoiding sustained CPU competition with the game.
            if SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST).is_err() {
                log_warn!("could not raise the critical throttle thread priority");
            }
            let instance = match GetModuleHandleW(None) {
                Ok(module) => HINSTANCE(module.0),
                Err(e) => {
                    log_error!("critical throttle: GetModuleHandleW failed: {e:?}");
                    return;
                }
            };
            let class = w!("VeloryftCriticalThrottleClass");
            RegisterClassW(&WNDCLASSW {
                hInstance: instance,
                lpszClassName: class,
                lpfnWndProc: Some(panel_proc),
                hbrBackground: HBRUSH::default(),
                ..Default::default()
            });
            let initial = *placement.lock().unwrap_or_else(|p| p.into_inner());
            let mut surface = match CriticalThrottleSurface::new(class, instance, initial) {
                Ok(surface) => surface,
                Err(e) => {
                    log_error!("critical throttle: surface creation failed: {e:?}");
                    return;
                }
            };
            let mut displayed = 0.0f32;
            let mut alpha = 0.0f32;
            let mut last_tick = Instant::now();
            let mut last_packet = 0u64;
            let mut last_change = Instant::now();
            let mut msg = MSG::default();
            loop {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let now = Instant::now();
                let dt = (now - last_tick).as_secs_f32().min(0.1);
                last_tick = now;
                let snap = shared.latest();
                if snap._pkt != last_packet {
                    last_packet = snap._pkt;
                    last_change = now;
                }
                let age = (now - last_change).as_secs_f32();
                let fresh = age < visibility::STALE_S;
                let driving = visibility::is_hud_visible(&snap, age);
                let target = if fresh {
                    snap.accel.unwrap_or(0.0) / 100.0
                } else {
                    0.0
                };
                let throttle_tau = if target >= displayed {
                    THROTTLE_TAU_ATTACK
                } else {
                    THROTTLE_TAU_RELEASE
                };
                displayed = display::exp_smooth(displayed, target, dt, throttle_tau);
                let target_alpha = if driving { 1.0 } else { 0.0 };
                let alpha_tau = if target_alpha > alpha {
                    display::TAU_FADE_IN
                } else {
                    display::TAU_FADE_OUT
                };
                alpha = display::exp_smooth(alpha, target_alpha, dt, alpha_tau);
                if target_alpha == 0.0 && alpha < 0.015 {
                    alpha = 0.0;
                }
                let next_placement = *placement.lock().unwrap_or_else(|p| p.into_inner());
                if let Err(e) = surface.sync_placement(next_placement) {
                    if THROTTLE_PLACEMENT_FAILED.first() {
                        log_error!(
                            "critical throttle: placement failed: {e:?} (repeats suppressed)"
                        );
                    }
                }
                let visible = !HUD_HIDDEN.load(Ordering::Relaxed)
                    && !EDIT.load(Ordering::Relaxed)
                    && alpha > 0.01;
                if let Err(e) = surface.draw(displayed, alpha, visible) {
                    if THROTTLE_DRAW_FAILED.first() {
                        log_error!("critical throttle: draw failed: {e:?} (repeats suppressed)");
                    }
                }
                let elapsed = last_tick.elapsed();
                if elapsed < Duration::from_millis(THROTTLE_FRAME_MS) {
                    thread::sleep(Duration::from_millis(THROTTLE_FRAME_MS) - elapsed);
                }
            }
        })
        .expect("failed to start the critical throttle thread")
}
