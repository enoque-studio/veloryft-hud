//! Process entry point for the in-game overlay.
//!
//! Release builds target the Windows subsystem, so there is no console: every
//! diagnostic goes through `application::diagnostics`, and startup failures are
//! shown in a message box.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(failure) = veloryft_hud::application::run() {
        std::process::exit(failure.exit_code());
    }
}
