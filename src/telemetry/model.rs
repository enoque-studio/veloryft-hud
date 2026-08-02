//! Telemetry derived from Forza packets. Field names remain compatible with the
//! original `backend/payload.py` contract ported from the Python parser.

use serde::Serialize;

use super::packet::Packet;
use super::vehicle_catalog;

#[derive(Serialize, Clone, Default)]
pub struct Telemetry {
    /// `is_race_on`, which is zero in free roam.
    pub race: i32,
    /// "Sled" | "Dash" | "FH6".
    pub fmt: &'static str,
    pub ts: u32,
    pub rpm: f32,
    pub max_rpm: f32,
    pub idle_rpm: f32,
    /// Unique make/model ID from the BASE block.
    pub car_ordinal: i32,
    /// Human-readable name resolved through `data/car_ordinals.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub car_name: Option<String>,
    /// FH6 car group/class, which changes when the vehicle changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub car_group: Option<u32>,
    /// One for a Sled-only packet with no dashboard fields.
    pub sled_only: u8,

    // G-forces and slip come from BASE and are present even in Sled packets.
    pub lat_g: f32,
    pub long_g: f32,
    /// Per-wheel combined slip [FL, FR, RL, RR]; approximately 1.0 is the grip limit.
    pub slip: [f32; 4],

    // Dashboard fields, omitted from the Sled format.
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
    /// Instantaneous metric horsepower, Dash-only, calculated as watts / 745.7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_hp: Option<f32>,
    /// Tire temperatures [FL, FR, RL, RR], Dash-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tires: Option<[f32; 4]>,
    /// World position [x, y, z] in metres (Dash-only); `y` is elevation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 3]>,
    /// Per-wheel surface rumble [FL, FR, RL, RR], used to infer the surface type.
    /// Near zero means asphalt; high values indicate dirt or grass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surf_rumble: Option<[f32; 4]>,
    /// Per-wheel puddle depth [FL, FR, RL, RR], used for the map's water class.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub puddle: Option<[f32; 4]>,

    /// Monotonic packet counter for deduplication and diagnostics.
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
        car_name: vehicle_catalog::lookup(ordinal),
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
        t.surf_rumble = Some(p.surface_rumble());
        t.puddle = Some(p.wheel_in_puddle());
        let power_w = p.power();
        if power_w.is_finite() && power_w >= 0.0 {
            t.power_hp = Some(power_w / vehicle_catalog::WATTS_PER_CV);
        }
    }

    t
}
