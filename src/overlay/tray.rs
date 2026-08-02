//! The system-tray icon and its context menu.
//!
//! The tray owns a message-only window: it never appears on screen and exists
//! purely to receive the icon's notifications. Windows files new icons under
//! "hidden icons" until the user pins them, which is why the quick-start guide
//! tells people to look there first.

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};

use windows::Win32::UI::WindowsAndMessaging::*;

/// Private window message the tray icon posts to the message-only window.
pub(crate) const WM_TRAYICON: u32 = WM_APP + 1;
/// Context-menu command ids.
pub(crate) const ID_TRAY_TOGGLE: usize = 1001;
pub(crate) const ID_TRAY_EXIT: usize = 1002;

/// Set by the tray menu and the icon's double-click, drained by the render
/// loop. An atomic rather than a direct call because the menu runs inside a
/// nested message loop, where touching the panels would re-enter the renderer.
pub(crate) static TRAY_TOGGLE_HIDE: AtomicBool = AtomicBool::new(false);

/// Codifica uma string curta como `szTip`/`szInfo` (UTF-16, terminado em nulo).
pub(crate) fn wide_buf<const N: usize>(s: &str) -> [u16; N] {
    let mut buf = [0u16; N];
    for (i, c) in s.encode_utf16().take(N - 1).enumerate() {
        buf[i] = c;
    }
    buf
}

/// Menu de contexto do ícone na bandeja (clique direito): mostrar/ocultar e sair.
/// Os comandos só sinalizam via atomics/`PostQuitMessage` — o loop principal
/// (dono de `panels`/`backdrop`/`toolbar`) é quem de fato aplica o efeito.
pub(crate) unsafe fn show_tray_menu(hwnd: HWND) {
    let Ok(hmenu) = CreatePopupMenu() else { return };
    let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_TOGGLE, w!("Show/Hide HUD"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_EXIT, w!("Exit"));

    let mut pt = POINT { x: 0, y: 0 };
    let _ = GetCursorPos(&mut pt);
    // Necessário para o menu fechar sozinho ao perder o foco (peculiaridade do Win32).
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenuEx(
        hmenu,
        (TPM_RIGHTBUTTON | TPM_RETURNCMD).0,
        pt.x,
        pt.y,
        hwnd,
        None,
    );
    let _ = DestroyMenu(hmenu);

    match cmd.0 as usize {
        ID_TRAY_TOGGLE => TRAY_TOGGLE_HIDE.store(true, Ordering::Relaxed),
        ID_TRAY_EXIT => PostQuitMessage(0),
        _ => {}
    }
}
