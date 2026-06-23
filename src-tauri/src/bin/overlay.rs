//! Overlay nativo Direct2D/DirectComposition — alternativa ao WebView2 para as
//! barras de alta frequência, sem o judder do compositor transparente.
//! Reaproveita o núcleo `forza` (UDP + parsing) via a lib.
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
    eprintln!("overlay nativo só é suportado no Windows");
}
