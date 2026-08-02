//! Win32 window creation, layered presentation, and click-through behaviour.
//!
//! Every panel is a small `WS_EX_LAYERED | WS_EX_TRANSPARENT` top-level window.
//! Two properties matter and neither is negotiable:
//!
//! * **Per-pixel alpha.** Direct2D renders into a premultiplied 32-bit DIB and
//!   [`present`] hands that buffer to the compositor with `UpdateLayeredWindow`.
//!   The DWM then composites it on the GPU, so the HUD does not judder the way a
//!   fullscreen transparent surface does.
//! * **Cross-process click-through.** `WS_EX_TRANSPARENT` is what makes the
//!   mouse reach the game underneath. `HTTRANSPARENT` alone does not cross a
//!   process boundary. [`set_interactive`] drops the flag only in edit mode,
//!   when the user is supposed to be able to grab the panels.
//!
//! See `docs/decisions/adr_0001_native_direct2d_overlay.md`.

//! # Unsafe in this module
//!
//! Every `unsafe` block here is a call into the Win32 API. The invariants are
//! the same throughout and are stated once rather than repeated at each call:
//!
//! * Window handles come from `CreateWindowExW` in this module and are used on
//!   the thread that created them, before `Drop` destroys them.
//! * Device contexts and bitmaps are owned by their `Panel`, `Toolbar`, or
//!   `Backdrop`, which release them in `Drop`.
//! * Pointers passed to Win32 refer to locals that outlive the call; none is
//!   retained by the API after it returns.
//!
//! Individual blocks carry a `SAFETY:` comment only where the reasoning goes
//! beyond that.

use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Create a topmost, click-through layered window without a taskbar entry.
pub(crate) fn make_window(
    class: PCWSTR,
    instance: HINSTANCE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Result<HWND> {
    let title: Vec<u16> = crate::localization::text()
        .app_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            PCWSTR(title.as_ptr()),
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

/// Create a top-down, premultiplied 32-bpp DIB bound to a device context.
pub(crate) fn create_dib_bitmap(memdc: HDC, w: i32, h: i32) -> Result<HBITMAP> {
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
        // SAFETY: `bits` receives the pixel buffer CreateDIBSection allocates.
        // We never read or write through it: Direct2D owns the pixels via the
        // DC render target, and the bitmap's lifetime is tied to the HBITMAP,
        // which its owner deletes in Drop.
        let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
        CreateDIBSection(Some(memdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
    }
}

/// Memory DC with a selected DIB, used as the D2D target and ULW source.
pub(crate) fn make_dib(w: i32, h: i32) -> Result<(HDC, HBITMAP)> {
    unsafe {
        let memdc = CreateCompatibleDC(None);
        let bitmap = create_dib_bitmap(memdc, w, h)?;
        SelectObject(memdc, HGDIOBJ(bitmap.0));
        Ok((memdc, bitmap))
    }
}

pub(crate) fn window_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        r
    }
}

/// The panel's design size, stored in the window's `GWLP_USERDATA` (high 16
/// bits width, low 16 bits height). Used to lock the aspect ratio on resize.
///
/// # Safety
///
/// `hwnd` must be a panel window created by [`make_window`], whose user data
/// was set by `Panel::new`. Reading it from any other window returns whatever
/// that window stores there, which is meaningless here.
pub(crate) unsafe fn design_of(hwnd: HWND) -> (i32, i32) {
    // SAFETY: the caller guarantees hwnd is one of our panel windows, so the
    // user-data slot holds the packed size and not a pointer we would deref.
    let packed = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    (((packed >> 16) & 0xFFFF) as i32, (packed & 0xFFFF) as i32)
}

/// Update the layered window with content already drawn into `memdc`.
pub(crate) fn present(hwnd: HWND, memdc: HDC, pos: POINT, size: SIZE, alpha: u8) -> Result<()> {
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

pub(crate) fn set_interactive(hwnd: HWND, interactive: bool) {
    unsafe {
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mask = WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
        let new = if interactive { cur & !mask } else { cur | mask };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new as isize);
        // Without SWP_FRAMECHANGED the new ex-style is not re-evaluated during
        // hit testing, so click-through would not actually change.
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

pub(crate) fn bring_to_top(hwnd: HWND) {
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
