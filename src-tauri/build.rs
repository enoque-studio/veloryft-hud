//! Embute os recursos Windows no exe: ícone e version info (ProductName,
//! copyright etc.). Sem isso o binário aparece "sem identidade" no Explorer,
//! no Gerenciador de Tarefas e no SmartScreen.

fn main() {
    write_license_config();

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
    res.set(
        "FileDescription",
        "Veloryft (Beta) — Forza telemetry overlay",
    );
    res.set("CompanyName", "Veloryft");
    res.set(
        "LegalCopyright",
        "\u{a9} 2026 Veloryft. Todos os direitos reservados.",
    );
    res.set("InternalName", "forza-hud-native");
    res.set("OriginalFilename", "Veloryft.exe");
    let windows_version = format!("{version}.0");
    res.set("ProductVersion", &windows_version);
    res.set("FileVersion", &windows_version);
    res.compile()
        .expect("falha ao embutir recursos do Windows (rc.exe do SDK)");
}

/// Produz constantes privadas no OUT_DIR. Nunca lê valores em tempo de
/// execução: a origem e as raízes de confiança fazem parte do artefato assinado.
/// No beta o binário não executa o gate de licença, então as variáveis são
/// opcionais em qualquer perfil — ausentes, o módulo fica "não provisionado".
fn write_license_config() {
    const CONFIG_VARS: [&str; 4] = [
        "VELORYFT_LICENSE_ORIGIN",
        "VELORYFT_LICENSE_APP_HMAC_SECRET",
        "VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS",
        "VELORYFT_LICENSE_SPKI_PINS",
    ];
    for name in CONFIG_VARS {
        println!("cargo:rerun-if-env-changed={name}");
    }

    let origin = build_value("VELORYFT_LICENSE_ORIGIN");
    let app_hmac_secret = build_value("VELORYFT_LICENSE_APP_HMAC_SECRET");
    let response_keys = split_values(build_value("VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS"));
    let spki_pins = split_values(build_value("VELORYFT_LICENSE_SPKI_PINS"));

    let content = format!(
        "pub const LICENSE_ORIGIN: Option<&str> = {};\n\
         pub const LICENSE_APP_HMAC_SECRET: Option<&str> = {};\n\
         pub const LICENSE_RESPONSE_PUBLIC_KEYS: &[&str] = {};\n\
         pub const LICENSE_SPKI_PINS: &[&str] = {};\n",
        as_rust_option(origin.as_deref()),
        as_rust_option(app_hmac_secret.as_deref()),
        as_rust_list(&response_keys),
        as_rust_list(&spki_pins),
    );
    let out_dir = std::env::var("OUT_DIR").expect("Cargo must set OUT_DIR");
    std::fs::write(
        std::path::Path::new(&out_dir).join("license_config.rs"),
        content,
    )
    .expect("writing generated license configuration");
}

fn build_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn split_values(value: Option<String>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|text| {
            text.split(';')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn as_rust_option(value: Option<&str>) -> String {
    value
        .map(|item| format!("Some({item:?})"))
        .unwrap_or_else(|| "None".into())
}

fn as_rust_list(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|item| format!("{item:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{values}]")
}
