//! HUD in-game: overlay nativo Direct2D (`render.rs`). Binário único do crate.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        if let Err(error) = forza_hud_native_lib::license::authorize_startup() {
            show_license_error(&error);
            std::process::exit(1);
        }
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

#[cfg(windows)]
fn show_license_error(error: &forza_hud_native_lib::license::LicenseError) {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let message = match error {
        forza_hud_native_lib::license::LicenseError::ExpiredEntitlement => {
            w!("Sua avaliacao ou licenca do Veloryft expirou.")
        }
        forza_hud_native_lib::license::LicenseError::NotProvisioned
        | forza_hud_native_lib::license::LicenseError::InvalidConfiguration => {
            w!("Esta instalacao do Veloryft nao foi provisionada para o servico de licencas.")
        }
        forza_hud_native_lib::license::LicenseError::FingerprintUnavailable => {
            w!("Nao foi possivel identificar esta maquina para validar a licenca.")
        }
        forza_hud_native_lib::license::LicenseError::NetworkUnavailable => {
            w!("Nao foi possivel conectar ao servico de licencas do Veloryft.\n\nVerifique sua conexao com a internet. Se voce usa um antivirus com inspecao SSL/HTTPS (ex: Norton, Kaspersky), adicione o Veloryft como excecao \u{2014} a conexao do Veloryft e protegida contra interceptacao e sera recusada nesses casos.")
        }
        _ => w!("Nao foi possivel validar a licenca do Veloryft."),
    };
    unsafe {
        let _ = MessageBoxW(None, message, w!("Veloryft"), MB_ICONERROR | MB_OK);
    }
}
