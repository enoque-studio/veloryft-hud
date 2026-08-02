//! The edit-mode backdrop: the full-screen grid shown while panels are being
//! arranged.
//!
//! It exists only in edit mode. Outside it the window is hidden, so the overlay
//! never covers the screen during play.

use windows::core::Interface;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::Vector2;

use crate::overlay::geometry::{rectf, GRID};
use crate::overlay::graphics::make_dcrt;
use crate::overlay::theme::color;
use crate::overlay::window::{bring_to_top, make_dib, make_window, present};

pub(crate) struct Backdrop {
    pub hwnd: HWND,
    pub memdc: HDC,
    pub bitmap: HBITMAP,
    pub size: SIZE,
    pub dcrt: ID2D1DCRenderTarget,
    pub rt: ID2D1RenderTarget,
    pub dim: ID2D1SolidColorBrush,
    pub line: ID2D1SolidColorBrush,
}

impl Backdrop {
    pub fn new(
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

    pub fn draw(&self) -> Result<()> {
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

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memdc);
        }
    }
}
