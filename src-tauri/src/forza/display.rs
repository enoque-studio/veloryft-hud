//! Suavização, aprendizado de redline e visibilidade do HUD — fonte única usada
//! pelo overlay Direct2D e pela webview Tauri (sem duplicar lógica no front).

use std::time::Instant;

use serde::Serialize;

use super::elevation::{ElevationProfile, ElevationTracker};
use super::payload::{self, Telemetry};
use super::tires::{TireRange, TireTracker};
use super::visibility;

pub const TAU_BAR: f32 = 0.032;
pub const TAU_TEXT: f32 = 0.11;
pub const TAU_FADE: f32 = 0.07;
pub const SHIFT_AT: f32 = 0.92;
const LEARN_MIN_THROTTLE: f32 = 90.0;
const LEARN_MIN_OVER_IDLE: f32 = 2200.0;
const LEARN_MIN_REPORTED_FRAC: f32 = 0.45;
const LEARN_SUSPICIOUS_REPORTED_FRAC: f32 = 0.88;

const ANIM_RPM: f32 = 0.0005;
const ANIM_PEDAL: f32 = 0.0005;
const ANIM_SPEED: f32 = 0.05;
const ANIM_G: f32 = 0.002;
const ANIM_ALPHA: f32 = 0.005;

#[derive(Clone, Debug, Default, Serialize)]
pub struct DisplayFrame {
    pub rpm_frac: f32,
    pub near: bool,
    pub gear: Option<u8>,
    pub speed: f32,
    pub accel: f32,
    pub brake: f32,
    pub car_name: String,
    pub power_cv: f32,
    pub show_power: bool,
    pub slip: [f32; 4],
    pub tires: Option<[f32; 4]>,
    pub tire_min: Option<[f32; 4]>,
    pub tire_max: Option<[f32; 4]>,
    pub lat: f32,
    pub long: f32,
    pub fresh: bool,
    pub alpha: f32,
    /// Ainda interpolando — o render deve continuar desenhando.
    pub animating: bool,
    /// Diagnóstico para a webview (status bar).
    pub fmt: String,
    pub rpm_raw: u32,
    pub pkt: u64,
    /// Elevação atual (m), suavizada.
    pub elev_m: f32,
    pub elev_min: f32,
    pub elev_max: f32,
    pub lap_km: f32,
}

pub fn exp_smooth(cur: f32, target: f32, dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 || dt <= 0.0 {
        return target;
    }
    cur + (target - cur) * (1.0 - (-dt / tau).exp())
}

pub fn car_signature(snap: &Telemetry) -> u64 {
    let max_b = if snap.max_rpm > 100.0 {
        snap.max_rpm.to_bits() as u64
    } else {
        0
    };
    let idle_b = if snap.idle_rpm > 0.0 {
        snap.idle_rpm.to_bits() as u64
    } else {
        0
    };
    let group = snap.car_group.unwrap_or(0) as u64;
    let ord = if snap.car_ordinal != 0 {
        snap.car_ordinal as u64
    } else {
        0
    };
    max_b ^ (idle_b << 1) ^ (group << 2) ^ (ord << 3)
}

pub struct DisplaySmoother {
    frame: DisplayFrame,
    d_rpm: f32,
    d_accel: f32,
    d_brake: f32,
    d_speed: f32,
    d_lat: f32,
    d_long: f32,
    d_slip: [f32; 4],
    d_power: f32,
    last_pkt: u64,
    last_change: Instant,
    last_frame: Instant,
    sticky_max_rpm: f32,
    observed_peak_rpm: f32,
    learned_max_rpm: f32,
    last_raw_rpm: f32,
    last_gear: Option<u8>,
    car_sig: u64,
    d_visible: f32,
    d_elev: f32,
    elevation: ElevationTracker,
    tires: TireTracker,
}

impl DisplaySmoother {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            frame: DisplayFrame::default(),
            d_rpm: 0.0,
            d_accel: 0.0,
            d_brake: 0.0,
            d_speed: 0.0,
            d_lat: 0.0,
            d_long: 0.0,
            d_slip: [0.0; 4],
            d_power: 0.0,
            last_pkt: 0,
            last_change: now,
            last_frame: now,
            sticky_max_rpm: 0.0,
            observed_peak_rpm: 0.0,
            learned_max_rpm: 0.0,
            last_raw_rpm: 0.0,
            last_gear: None,
            car_sig: 0,
            d_visible: 0.0,
            d_elev: 0.0,
            elevation: ElevationTracker::new(),
            tires: TireTracker::new(),
        }
    }

    pub fn elevation(&self) -> ElevationProfile {
        self.elevation.profile()
    }

    pub fn tire_range(&self) -> &TireRange {
        self.tires.range()
    }

    pub fn frame(&self) -> &DisplayFrame {
        &self.frame
    }

    fn reset_to(
        &mut self,
        t_rpm: f32,
        accel: f32,
        brake: f32,
        speed: f32,
        lat: f32,
        long: f32,
        slip: [f32; 4],
    ) {
        self.d_rpm = t_rpm;
        self.d_accel = accel;
        self.d_brake = brake;
        self.d_speed = speed;
        self.d_lat = lat;
        self.d_long = long;
        self.d_slip = slip;
    }

    fn effective_max_rpm(&self) -> f32 {
        if self.learned_max_rpm > 100.0 {
            self.learned_max_rpm
        } else {
            self.sticky_max_rpm
        }
    }

    fn reset_car_learning(&mut self, rpm: f32, gear: Option<u8>) {
        self.observed_peak_rpm = rpm.max(0.0);
        self.learned_max_rpm = 0.0;
        self.last_raw_rpm = rpm.max(0.0);
        self.last_gear = gear;
    }

    fn commit_observed_peak(&mut self, idle_rpm: f32) {
        let peak = self.observed_peak_rpm;
        let reported = self.sticky_max_rpm;
        if peak <= idle_rpm + LEARN_MIN_OVER_IDLE || reported <= 100.0 {
            return;
        }
        let frac = peak / reported;
        let looks_like_low_rev_car =
            frac >= LEARN_MIN_REPORTED_FRAC && frac < LEARN_SUSPICIOUS_REPORTED_FRAC;
        if looks_like_low_rev_car {
            self.learned_max_rpm = self.learned_max_rpm.max(peak / SHIFT_AT);
        }
    }

    fn learn_effective_redline(&mut self, snap: &Telemetry, fresh: bool) {
        if !fresh {
            return;
        }
        let gear = snap.gear;
        let accel = snap.accel.unwrap_or(0.0);
        let rpm = snap.rpm.max(0.0);
        let upshift = matches!(
            (self.last_gear, gear),
            (Some(prev), Some(next)) if next > prev && prev > 0
        );

        if upshift && accel >= LEARN_MIN_THROTTLE {
            self.commit_observed_peak(snap.idle_rpm);
            self.observed_peak_rpm = rpm;
        }
        if accel >= LEARN_MIN_THROTTLE && gear != Some(0) {
            self.observed_peak_rpm = self.observed_peak_rpm.max(rpm);
        }
        self.last_raw_rpm = rpm;
        self.last_gear = gear;
    }

    /// Avança um passo de suavização. `force_visible` mantém o HUD visível (modo edição).
    pub fn tick(&mut self, snap: &Telemetry, force_visible: bool) -> &DisplayFrame {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if snap._pkt != self.last_pkt {
            self.last_pkt = snap._pkt;
            self.last_change = now;
        }
        let age_s = (now - self.last_change).as_secs_f32();
        let fresh = age_s < visibility::STALE_S;
        let driving = force_visible || visibility::is_driving(snap, age_s);
        let target_vis = if driving { 1.0 } else { 0.0 };
        let prev_visible = self.d_visible;
        self.d_visible = exp_smooth(self.d_visible, target_vis, dt, TAU_FADE);

        if snap.max_rpm > 100.0 {
            self.sticky_max_rpm = snap.max_rpm;
        }
        let sig = car_signature(snap);
        if sig != 0 && self.car_sig != 0 && sig != self.car_sig {
            self.reset_car_learning(snap.rpm, snap.gear);
            self.elevation.reset();
            self.tires.reset();
            let max = self.effective_max_rpm();
            let t_rpm = if fresh && max > 0.0 {
                (snap.rpm / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let accel = if fresh {
                snap.accel.unwrap_or(0.0) / 100.0
            } else {
                0.0
            };
            let brake = if fresh {
                snap.brake.unwrap_or(0.0) / 100.0
            } else {
                0.0
            };
            let speed = if fresh { snap.speed.unwrap_or(0.0) } else { 0.0 };
            let lat = if fresh { snap.lat_g } else { 0.0 };
            let long = if fresh { snap.long_g } else { 0.0 };
            let slip = if fresh { snap.slip } else { [0.0; 4] };
            self.reset_to(t_rpm, accel, brake, speed, lat, long, slip);
            self.d_power = if fresh {
                snap.power_hp.unwrap_or(0.0)
            } else {
                0.0
            };
        }
        if sig != 0 {
            if self.car_sig == 0 {
                self.reset_car_learning(snap.rpm, snap.gear);
            }
            self.car_sig = sig;
        }
        self.learn_effective_redline(snap, fresh);

        let max = self.effective_max_rpm();
        let t_rpm = if fresh && max > 0.0 {
            (snap.rpm / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let t_accel = if fresh {
            snap.accel.unwrap_or(0.0) / 100.0
        } else {
            0.0
        };
        let t_brake = if fresh {
            snap.brake.unwrap_or(0.0) / 100.0
        } else {
            0.0
        };
        let t_speed = if fresh { snap.speed.unwrap_or(0.0) } else { 0.0 };
        let t_lat = if fresh { snap.lat_g } else { 0.0 };
        let t_long = if fresh { snap.long_g } else { 0.0 };
        let t_slip = if fresh { snap.slip } else { [0.0; 4] };
        let power_target = snap.power_hp.unwrap_or(0.0);

        self.d_rpm = exp_smooth(self.d_rpm, t_rpm, dt, TAU_BAR);
        self.d_accel = exp_smooth(self.d_accel, t_accel, dt, TAU_BAR);
        self.d_brake = exp_smooth(self.d_brake, t_brake, dt, TAU_BAR);
        self.d_speed = exp_smooth(self.d_speed, t_speed, dt, TAU_TEXT);
        self.d_lat = exp_smooth(self.d_lat, t_lat, dt, TAU_BAR);
        self.d_long = exp_smooth(self.d_long, t_long, dt, TAU_BAR);
        for i in 0..4 {
            self.d_slip[i] = exp_smooth(self.d_slip[i], t_slip[i], dt, TAU_BAR);
        }
        self.d_power = exp_smooth(self.d_power, if fresh { power_target } else { 0.0 }, dt, TAU_TEXT);

        let elev_target = snap.pos.map(|p| p[1]).unwrap_or(0.0);
        self.elevation.update(snap, driving && fresh);
        self.tires.update(snap.tires, driving && fresh);
        self.d_elev = exp_smooth(self.d_elev, if fresh { elev_target } else { self.d_elev }, dt, TAU_TEXT);
        let elev_prof = self.elevation.profile();
        let tire_rng = self.tires.range();
        let (tire_min, tire_max) = if tire_rng.any() {
            (Some(tire_rng.min), Some(tire_rng.max))
        } else {
            (None, None)
        };

        let mut animating = (self.d_visible - target_vis).abs() > ANIM_ALPHA
            || (prev_visible - self.d_visible).abs() > ANIM_ALPHA;
        animating |= (self.d_rpm - t_rpm).abs() > ANIM_RPM;
        animating |= (self.d_accel - t_accel).abs() > ANIM_PEDAL;
        animating |= (self.d_brake - t_brake).abs() > ANIM_PEDAL;
        animating |= (self.d_speed - t_speed).abs() > ANIM_SPEED;
        animating |= (self.d_lat - t_lat).abs() > ANIM_G;
        animating |= (self.d_long - t_long).abs() > ANIM_G;
        for i in 0..4 {
            animating |= (self.d_slip[i] - t_slip[i]).abs() > ANIM_G;
        }
        animating |= (self.d_elev - elev_target).abs() > ANIM_SPEED;

        self.frame = DisplayFrame {
            rpm_frac: self.d_rpm,
            near: self.d_rpm >= SHIFT_AT,
            gear: snap.gear,
            speed: self.d_speed,
            accel: self.d_accel,
            brake: self.d_brake,
            car_name: payload::car_label(snap.car_ordinal, snap.car_name.as_deref()),
            power_cv: self.d_power,
            show_power: snap.power_hp.is_some(),
            slip: self.d_slip,
            tires: if fresh { snap.tires } else { None },
            tire_min,
            tire_max,
            lat: self.d_lat,
            long: self.d_long,
            fresh,
            alpha: self.d_visible,
            animating,
            fmt: snap.fmt.to_string(),
            rpm_raw: snap.rpm.round() as u32,
            pkt: snap._pkt,
            elev_m: self.d_elev,
            elev_min: elev_prof.min,
            elev_max: elev_prof.max,
            lap_km: elev_prof.lap_km,
        };
        &self.frame
    }
}

impl Default for DisplaySmoother {
    fn default() -> Self {
        Self::new()
    }
}

/// Chave de paint por painel — evita `UpdateLayeredWindow` quando nada mudou.
pub fn panel_paint_key(
    kind: PanelKind,
    f: &DisplayFrame,
    edit: bool,
    accent_bits: u32,
    elev: Option<&ElevationProfile>,
) -> u64 {
    fn mix(h: u64, v: u64) -> u64 {
        h.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(v)
    }

    let mut h = if edit { 1u64 } else { 0 };
    h = mix(h, (f.alpha * 255.0).round() as u64);
    h = mix(h, accent_bits as u64);
    match kind {
        PanelKind::Cluster => {
            h = mix(h, (f.rpm_frac * 1024.0).round() as u64);
            h = mix(h, (f.accel * 512.0).round() as u64);
            h = mix(h, (f.brake * 512.0).round() as u64);
            h = mix(h, f.speed.round() as u64);
            h = mix(h, f.gear.unwrap_or(255) as u64);
            h = mix(h, if f.near { 1 } else { 0 });
            h = mix(h, f.car_name.len() as u64);
            for c in f.car_name.bytes().take(8) {
                h = mix(h, c as u64);
            }
            if f.show_power {
                h = mix(h, f.power_cv.round() as u64);
            }
        }
        PanelKind::Tires => {
            for slip in f.slip {
                h = mix(h, (slip * 256.0).round() as u64);
            }
            if let Some(t) = f.tires {
                for v in t {
                    h = mix(h, v.round() as u64);
                }
            }
            if let Some(m) = f.tire_min {
                for v in m {
                    h = mix(h, v.round() as u64);
                }
            }
            if let Some(m) = f.tire_max {
                for v in m {
                    h = mix(h, v.round() as u64);
                }
            }
        }
        PanelKind::GForce => {
            h = mix(h, (f.lat * 256.0).round() as i64 as u64);
            h = mix(h, (f.long * 256.0).round() as i64 as u64);
        }
        PanelKind::Elevation => {
            h = mix(h, f.elev_m.round() as u64);
            h = mix(h, f.elev_min.round() as u64);
            h = mix(h, f.elev_max.round() as u64);
            h = mix(h, (f.lap_km * 1000.0).round() as u64);
            if let Some(ep) = elev {
                h = mix(h, if ep.loop_closed { 1 } else { 0 });
                h = mix(h, (ep.loop_km * 1000.0).round() as u64);
                h = mix(h, ep.pts.len() as u64);
                h = mix(h, ep.marker_d.round() as u64);
                h = mix(h, ep.marker_y.round() as u64);
            }
        }
    }
    h
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Cluster,
    Tires,
    GForce,
    Elevation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suaviza_rpm_em_direcao_ao_alvo() {
        let mut s = DisplaySmoother::new();
        let mut t = Telemetry {
            sled_only: 0,
            race: 1,
            rpm: 5000.0,
            max_rpm: 8000.0,
            idle_rpm: 900.0,
            speed: Some(120.0),
            gear: Some(3),
            accel: Some(80.0),
            _pkt: 1,
            ..Default::default()
        };
        let f1 = s.tick(&t, false).rpm_frac;
        t._pkt = 2;
        let f2 = s.tick(&t, false).rpm_frac;
        assert!(f2 >= f1);
    }

    #[test]
    fn paint_key_nao_panica_com_nome_longo() {
        let f = DisplayFrame {
            car_name: "A".repeat(64),
            show_power: true,
            power_cv: 999.0,
            alpha: 1.0,
            ..Default::default()
        };
        let _ = panel_paint_key(PanelKind::Cluster, &f, false, 0, None);
        let _ = panel_paint_key(PanelKind::Tires, &f, false, 0, None);
    }

    #[test]
    fn esconde_quando_parado_no_menu() {
        let mut s = DisplaySmoother::new();
        let t = Telemetry {
            sled_only: 0,
            race: 0,
            rpm: 800.0,
            idle_rpm: 800.0,
            speed: Some(0.0),
            _pkt: 1,
            ..Default::default()
        };
        let f = s.tick(&t, false);
        assert!(f.alpha < 0.5);
    }
}
