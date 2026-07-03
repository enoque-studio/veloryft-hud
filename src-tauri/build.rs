//! Embute os recursos Windows no exe: ícone e version info (ProductName,
//! copyright etc.). Sem isso o binário aparece "sem identidade" no Explorer,
//! no Gerenciador de Tarefas e no SmartScreen.

fn main() {
    // Só quando o ALVO é Windows (build script roda no host).
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let icon = "../installer/assets/veloryft.ico";
    println!("cargo:rerun-if-changed={icon}");
    println!("cargo:rerun-if-changed=build.rs");

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon);
    res.set("ProductName", "Veloryft");
    res.set("FileDescription", "Veloryft (Beta) — Forza telemetry overlay");
    res.set("CompanyName", "Veloryft");
    res.set("LegalCopyright", "\u{a9} 2026 Veloryft. Todos os direitos reservados.");
    res.set("InternalName", "forza-hud-native");
    res.set("OriginalFilename", "Veloryft.exe");
    res.set("ProductVersion", &format!("{version}-beta"));
    res.set("FileVersion", &version);
    res.compile().expect("falha ao embutir recursos do Windows (rc.exe do SDK)");
}
