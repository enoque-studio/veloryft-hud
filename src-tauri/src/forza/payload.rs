//! Telemetria derivada, serializável para a webview.
//! Subconjunto da Fase 1 (fatia vertical) — nomes de campo compatíveis com o
//! payload de `backend/payload.py` para reaproveitar o front Solid na Fase 2.

use serde::Serialize;

use super::catalog;
use super::packet::Packet;

#[derive(Serialize, Clone, Default)]
pub struct Telemetry {
    /// is_race_on (0 em free roam).
    pub race: i32,
    /// "Sled" | "Dash" | "FH6".
    pub fmt: &'static str,
    pub ts: u32,
    pub rpm: f32,
    pub max_rpm: f32,
    pub idle_rpm: f32,
    /// ID único make/model (bloco BASE).
    pub car_ordinal: i32,
    /// Nome legível — resolvido via `data/car-ordinals.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub car_name: Option<String>,
    /// Grupo/classe do carro (FH6) — muda ao trocar de veículo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub car_group: Option<u32>,
    /// 1 quando o pacote é apenas Sled (sem campos de dashboard).
    pub sled_only: u8,

    // Forças G e slip vêm do bloco BASE (presentes mesmo em Sled).
    pub lat_g: f32,
    pub long_g: f32,
    /// combined_slip por roda [FL, FR, RL, RR] — ~1.0 = limite de aderência.
    pub slip: [f32; 4],

    // Campos de dashboard — omitidos no formato Sled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>, // km/h
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gear: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accel: Option<f32>, // 0..100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brake: Option<f32>, // 0..100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clutch: Option<f32>, // 0..100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steer: Option<f32>, // -100..100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel: Option<f32>, // 0..100
    /// Potência instantânea (cv) — dash-only, watts / 745.7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_hp: Option<f32>,
    /// Temperatura dos pneus [FL, FR, RL, RR] (dash-only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tires: Option<[f32; 4]>,
    /// Posição mundial [x, y, z] em metros (dash-only). `y` = elevação.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 3]>,

    /// Contador monotônico de pacotes — dedupe/diagnóstico no front.
    pub _pkt: u64,
}

const G: f32 = 9.80665;

pub fn car_label(ordinal: i32, name: Option<&str>) -> String {
    if let Some(n) = name {
        return n.to_string();
    }
    if ordinal != 0 {
        return format!("#{ordinal}");
    }
    "–".to_string()
}

pub fn build(p: &Packet) -> Telemetry {
    let ordinal = p.car_ordinal();
    let mut t = Telemetry {
        race: p.is_race_on(),
        fmt: p.format.as_str(),
        ts: p.timestamp_ms(),
        rpm: p.engine_rpm(),
        max_rpm: p.engine_max_rpm(),
        idle_rpm: p.engine_idle_rpm(),
        car_ordinal: ordinal,
        car_name: catalog::lookup(ordinal),
        car_group: p.car_group(),
        sled_only: p.sled_only as u8,
        lat_g: p.accel_x() / G,
        long_g: p.accel_z() / G,
        slip: p.combined_slip(),
        ..Default::default()
    };

    if !p.sled_only {
        t.speed = Some(p.speed() * 3.6); // m/s -> km/h
        t.gear = Some(p.gear());
        t.accel = Some(p.accel_input() as f32 / 2.55);
        t.brake = Some(p.brake_input() as f32 / 2.55);
        t.clutch = Some(p.clutch_input() as f32 / 2.55);
        t.steer = Some(p.steer() as f32 / 1.27);
        t.fuel = Some(p.fuel() * 100.0);
        t.tires = Some(p.tire_temps());
        t.pos = Some(p.pos());
        let power_w = p.power();
        if power_w.is_finite() && power_w >= 0.0 {
            t.power_hp = Some(power_w / catalog::WATTS_PER_CV);
        }
    }

    t
}
