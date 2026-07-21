//! Nomes de carros por `car_ordinal` — carrega `data/car-ordinals.json`
//! (formato legado: `"Nome do Carro": "247"`).
//!
//! O JSON é embutido no binário na compilação, então builds instalados e
//! portable sempre têm o catálogo; um arquivo em disco (env/cwd/exe) apenas
//! sobrepõe o embutido, para atualizar nomes sem recompilar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static CATALOG: OnceLock<HashMap<i32, String>> = OnceLock::new();

static EMBEDDED_CATALOG: &str = include_str!("../../../data/car-ordinals.json");

/// Watts → cv (mesma base do apex-flow: potência mecânica em hp, exibida como cv).
pub const WATTS_PER_CV: f32 = 745.7;

pub fn init() {
    let _ = CATALOG.set(load());
}

pub fn lookup(ordinal: i32) -> Option<String> {
    if ordinal == 0 {
        return None;
    }
    CATALOG.get()?.get(&ordinal).cloned()
}

pub fn len() -> usize {
    CATALOG.get().map(HashMap::len).unwrap_or(0)
}

fn load() -> HashMap<i32, String> {
    for path in candidate_paths() {
        match load_file(&path) {
            Ok(map) if !map.is_empty() => {
                eprintln!("[catalog] {} carros ← {}", map.len(), path.display());
                return map;
            }
            Ok(_) => eprintln!("[catalog] {} vazio", path.display()),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                eprintln!("[catalog] {}: {e}", path.display())
            }
            Err(_) => {}
        }
    }
    match parse(EMBEDDED_CATALOG) {
        Ok(map) if !map.is_empty() => {
            eprintln!("[catalog] {} carros ← embutido", map.len());
            map
        }
        _ => {
            eprintln!("[catalog] catálogo embutido inválido — modelo indisponível");
            HashMap::new()
        }
    }
}

fn load_file(path: &Path) -> std::io::Result<HashMap<i32, String>> {
    parse(&std::fs::read_to_string(path)?)
}

fn parse(text: &str) -> std::io::Result<HashMap<i32, String>> {
    let raw: HashMap<String, String> = serde_json::from_str(text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut out = HashMap::with_capacity(raw.len());
    for (name, ord_str) in raw {
        if let Ok(ord) = ord_str.parse::<i32>() {
            out.insert(ord, name);
        }
    }
    Ok(out)
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(p) = std::env::var("FORZA_CAR_ORDINALS") {
        paths.push(PathBuf::from(p));
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("data/car-ordinals.json"));
        paths.push(cwd.join("../data/car-ordinals.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("data/car-ordinals.json"));
            paths.push(dir.join("../data/car-ordinals.json"));
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converte_watts_para_cv() {
        assert!((750_000.0 / WATTS_PER_CV - 1006.0).abs() < 1.0);
    }

    #[test]
    fn catalogo_embutido_carrega() {
        let map = parse(EMBEDDED_CATALOG).unwrap();
        assert!(!map.is_empty());
        assert_eq!(map.get(&247).map(String::as_str), Some("1969 Toyota 2000 GT"));
    }
}
