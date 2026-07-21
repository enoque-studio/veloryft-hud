//! HUD in-game: overlay nativo Direct2D (`render.rs`). Binário único do crate.
//! O beta roda sem verificação de licença; o módulo `license` permanece na lib
//! para a versão paga pós-beta.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        if let Err(e) = forza_hud_native_lib::render::run() {
            eprintln!("[overlay] erro: {e:?}");
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("HUD overlay nativo só é suportado no Windows");
        std::process::exit(1);
    }
}
