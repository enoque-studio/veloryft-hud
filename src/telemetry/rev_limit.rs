//! Per-vehicle rev limit learned from live telemetry.
//!
//! Forza packets carry `engine_max_rpm`, the tachometer's full-scale value,
//! but the real limiter and optimal shift point vary by vehicle and upgrade.
//! This module observes full-throttle telemetry and learns per `car_ordinal`:
//!
//! - **Limiter** — a sustained RPM plateau at full throttle. The plateau is
//!   valid only if RPM climbed to it in the same gear after the post-shift
//!   blackout. During a downshift, the clutch ramp sweeps RPM upward and the
//!   wheels can force the engine above the limiter from a higher, flat or
//!   falling value; none of those signals identify the limiter. High wheel
//!   slip also invalidates a plateau because traction, not the limiter, governs
//!   RPM. A high-gear plateau may be an aerodynamic limit, so a plateau below
//!   `CUT_TRUST_FRAC` counts only when a higher gear has been observed. A
//!   trustworthy plateau far from the saved limiter replaces bad learning, but
//!   must last `PLATEAU_HOLD_REPLACE` so a transient cannot erase good data.
//! - **Power curve and shift RPM drop** — maximum power observed in each RPM
//!   band and the `rpm_after / rpm_before` ratio from upshifts. The optimal
//!   shift is the highest RPM where holding the current gear still produces
//!   more power than the next gear (`P(rpm) ≥ P(rpm·ratio)`). Without a learned
//!   ratio, the recommendation stays just below the limiter.
//!
//! Learning persists in `%APPDATA%/forza-hud-native/rev-limits.json`, so leaving
//! a vehicle and returning preserves calibration. An engine upgrade or swap changes
//! `engine_max_rpm` and invalidates the record so learning starts again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::model::Telemetry;
use crate::diagnostics::{log_info, log_warn, Once};

/// A read-only or full disk fails on every save. Report once.
static SAVE_FAILED: Once = Once::new();

/// Number of RPM bands in the power curve from zero to max RPM.
const BINS: usize = 64;
/// Default shift point as a max-RPM fraction before anything is learned.
pub const DEFAULT_SHIFT_FRAC: f32 = 0.92;
/// Minimum throttle on the 0..100 wire scale for learning samples.
const FULL_THROTTLE: f32 = 88.0;
/// Dead time after a gear change, used to ignore downshift overshoot.
const POST_SHIFT_BLACKOUT: Duration = Duration::from_millis(400);
/// Plateau band half-width as a fraction of max RPM.
const PLATEAU_BAND_FRAC: f32 = 0.008;
/// Minimum time inside the band before declaring a limiter.
const PLATEAU_HOLD: Duration = Duration::from_millis(250);
/// Replacing a saved limiter requires a much longer plateau so automatic
/// correction cannot let a downshift transient erase good calibration.
const PLATEAU_HOLD_REPLACE: Duration = Duration::from_millis(700);
/// Combined slip above this means a spinning or locked wheel. Traction governs
/// RPM in that state, so the plateau is invalid.
const SLIP_TRUST_MAX: f32 = 1.2;
/// A plateau counts as the limiter only above this max-RPM fraction.
const CUT_MIN_FRAC: f32 = 0.82;
/// Above this fraction, a plateau counts even in the highest observed gear.
const CUT_TRUST_FRAC: f32 = 0.93;
/// RPM must have climbed by this max-RPM fraction in the current gear before a
/// plateau. A hard downshift enters from above and must not count as the limiter.
const CLIMB_MIN_FRAC: f32 = 0.02;
/// File version. Incrementing it discards learning from known-bad versions.
/// Version 3 replaces version 2, whose limiters could be corrupted by downshift or slip plateaus.
const STORE_VERSION: u32 = 3;
/// Shift warning lead below the limiter as a fraction, with an 80 RPM minimum.
const SHIFT_MARGIN_FRAC: f32 = 0.015;
/// Minimum populated bands required to trust the power curve.
const MIN_POWER_BINS: usize = 8;
/// Minimum observed upshifts required to trust the RPM-drop ratio.
const MIN_DROPS: usize = 3;
/// Maximum stored ratios in the sliding window.
const MAX_DROPS: usize = 12;
/// Window for measuring post-shift RPM after the blackout.
const UPSHIFT_WINDOW: Duration = Duration::from_millis(1200);
/// Plausible `rpm_after / rpm_before` range for an upshift.
const DROP_RANGE: std::ops::RangeInclusive<f32> = 0.45..=0.95;
/// Minimum interval between saves to disk.
const SAVE_EVERY: Duration = Duration::from_secs(3);

/// Calibration learned for one vehicle.
#[derive(Serialize, Deserialize, Clone)]
pub struct CarRevData {
    /// `engine_max_rpm` at learning time; a change indicates a different engine.
    pub max_rpm: f32,
    /// RPM where the limiter cuts, or `None` when never observed.
    #[serde(default)]
    pub cut_rpm: Option<f32>,
    /// Maximum metric horsepower per `rpm / max_rpm` band; 0.0 means empty.
    #[serde(default)]
    pub power: Vec<f32>,
    /// `rpm_after / rpm_before` ratios from recent full-throttle upshifts.
    #[serde(default)]
    pub drops: Vec<f32>,
}

impl CarRevData {
    fn new(max_rpm: f32) -> Self {
        Self {
            max_rpm,
            cut_rpm: None,
            power: vec![0.0; BINS],
            drops: Vec::new(),
        }
    }
}

/// Values consumed by display/rendering, expressed as fractions of `max_rpm`.
#[derive(Clone, Copy, Debug)]
pub struct ShiftInfo {
    /// Recommended shift point.
    pub shift_frac: f32,
    /// Learned limiter, or 1.0 while unknown.
    pub limit_frac: f32,
    /// Whether the limiter has been observed for this vehicle.
    pub limit_known: bool,
}

impl Default for ShiftInfo {
    fn default() -> Self {
        Self {
            shift_frac: DEFAULT_SHIFT_FRAC,
            limit_frac: 1.0,
            limit_known: false,
        }
    }
}

/// Upshift waiting for RPM to settle in the new gear before measuring its drop.
#[derive(Clone, Copy)]
struct PendingUpshift {
    to_gear: u8,
    rpm_before: f32,
    at: Instant,
}

/// Transient detection state, reset when the vehicle changes.
#[derive(Default)]
struct Transients {
    last_gear: Option<u8>,
    last_rpm: f32,
    /// Lowest RPM in the current gear, proving whether RPM climbed to the plateau.
    gear_min_rpm: Option<f32>,
    gear_changed_at: Option<Instant>,
    pending_upshift: Option<PendingUpshift>,
    /// Highest forward gear seen this session. A plateau in that gear may be an
    /// aerodynamic limit rather than the engine limiter.
    max_gear_seen: u8,
    plat_rpm: f32,
    plat_since: Option<Instant>,
}

impl Transients {
    fn reset_plateau(&mut self) {
        self.plat_rpm = 0.0;
        self.plat_since = None;
    }
}

#[derive(Serialize, Deserialize, Default)]
struct StoreFile {
    version: u32,
    cars: HashMap<String, CarRevData>,
}

pub struct RevLimitTracker {
    cars: HashMap<String, CarRevData>,
    path: Option<PathBuf>,
    dirty: bool,
    last_save: Instant,
    active: Option<String>,
    info: ShiftInfo,
    tr: Transients,
}

fn car_key(snap: &Telemetry) -> Option<String> {
    if snap.car_ordinal != 0 {
        return Some(format!("ord:{}", snap.car_ordinal));
    }
    if snap.max_rpm > 100.0 {
        let sig = (snap.max_rpm.to_bits() as u64) ^ ((snap.idle_rpm.to_bits() as u64) << 1);
        return Some(format!("sig:{sig:x}"));
    }
    None
}

/// Median RPM-drop ratio, or `None` without enough samples.
fn median_drop(drops: &[f32]) -> Option<f32> {
    if drops.len() < MIN_DROPS {
        return None;
    }
    let mut v = drops.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

/// Power in one band, smoothed with immediate neighbours for sparse data.
fn bin_power(power: &[f32], i: usize) -> Option<f32> {
    let lo = i.saturating_sub(1);
    let hi = (i + 1).min(BINS - 1);
    let best = power[lo..=hi].iter().cloned().fold(0.0f32, f32::max);
    (best > 0.0).then_some(best)
}

/// Compute the shift point from learned data. It starts just below the limiter;
/// when the power curve and shift drop are known, it moves earlier to the
/// highest RPM where the current gear still outperforms the next one.
fn compute_info(e: &CarRevData) -> ShiftInfo {
    if e.max_rpm <= 100.0 {
        return ShiftInfo::default();
    }
    let limit_known = e.cut_rpm.is_some();
    let limit = e.cut_rpm.unwrap_or(e.max_rpm).min(e.max_rpm * 1.02);
    let mut shift = if limit_known {
        limit - (limit * SHIFT_MARGIN_FRAC).max(80.0)
    } else {
        e.max_rpm * DEFAULT_SHIFT_FRAC
    };

    let filled = e.power.iter().filter(|&&p| p > 0.0).count();
    if let Some(drop) = median_drop(&e.drops).filter(|_| filled >= MIN_POWER_BINS) {
        let (mut peak_i, mut peak_p) = (0usize, 0.0f32);
        for (i, &p) in e.power.iter().enumerate() {
            if p > peak_p {
                peak_p = p;
                peak_i = i;
            }
        }
        let top_bin = (((shift / e.max_rpm) * BINS as f32) as usize).min(BINS - 1);
        if peak_p > 0.0 && peak_i < top_bin {
            // P(rpm) ≥ P(rpm·drop): holding the current gear is still worthwhile.
            let mut best: Option<usize> = None;
            for b in peak_i..=top_bin {
                let after_bin = (((b as f32 + 0.5) * drop) as usize).min(BINS - 1);
                let (Some(cur), Some(after)) =
                    (bin_power(&e.power, b), bin_power(&e.power, after_bin))
                else {
                    continue;
                };
                if cur >= after {
                    best = Some(b);
                }
            }
            if let Some(b) = best {
                let rpm = (b as f32 + 0.5) / BINS as f32 * e.max_rpm;
                shift = shift.min(rpm);
            }
        }
    }

    let shift_frac = (shift / e.max_rpm).clamp(0.30, 1.0);
    let limit_frac = if limit_known {
        (limit / e.max_rpm).clamp(shift_frac, 1.0)
    } else {
        1.0
    };
    ShiftInfo {
        shift_frac,
        limit_frac,
        limit_known,
    }
}

/// Run one learning step for the active vehicle. Return `true` when new data was
/// learned so the store can be marked dirty.
fn learn(e: &mut CarRevData, tr: &mut Transients, snap: &Telemetry, now: Instant) -> bool {
    let mut dirty = false;
    let throttle = snap.accel.unwrap_or(0.0);

    if snap.gear != tr.last_gear {
        // A full-throttle upshift schedules an RPM-drop measurement. RPM in the
        // new gear is valid only after the blackout, once the engine is engaged.
        tr.pending_upshift = match (tr.last_gear, snap.gear) {
            (Some(prev), Some(new))
                if new == prev + 1
                    && (1..=9).contains(&prev)
                    && throttle >= FULL_THROTTLE
                    && tr.last_rpm > 0.0 =>
            {
                Some(PendingUpshift {
                    to_gear: new,
                    rpm_before: tr.last_rpm,
                    at: now,
                })
            }
            _ => None,
        };
        tr.last_gear = snap.gear;
        tr.gear_changed_at = Some(now);
        tr.gear_min_rpm = None;
        tr.reset_plateau();
    }
    // During the blackout, the clutch ramp sweeps RPM from the old gear to the
    // new one. That minimum does not prove a free climb to a plateau, so
    // tracking starts only after the blackout.
    let in_blackout = tr
        .gear_changed_at
        .is_some_and(|t| now.duration_since(t) < POST_SHIFT_BLACKOUT);
    if snap.rpm.is_finite() {
        tr.last_rpm = snap.rpm;
        if snap.rpm > 0.0 && !in_blackout {
            tr.gear_min_rpm = Some(tr.gear_min_rpm.map_or(snap.rpm, |m| m.min(snap.rpm)));
        }
    }
    let gear = snap.gear.unwrap_or(0);
    if (1..=10).contains(&gear) && gear > tr.max_gear_seen {
        tr.max_gear_seen = gear;
    }

    if let Some(p) = tr.pending_upshift {
        let elapsed = now.duration_since(p.at);
        if snap.gear != Some(p.to_gear) || elapsed > UPSHIFT_WINDOW {
            tr.pending_upshift = None;
        } else if elapsed >= POST_SHIFT_BLACKOUT
            && throttle >= FULL_THROTTLE
            && snap.rpm.is_finite()
            && snap.rpm > 0.0
        {
            let ratio = snap.rpm / p.rpm_before;
            if DROP_RANGE.contains(&ratio) {
                e.drops.push(ratio);
                if e.drops.len() > MAX_DROPS {
                    e.drops.remove(0);
                }
                dirty = true;
            }
            tr.pending_upshift = None;
        }
    }

    let sampling = e.max_rpm > 100.0
        && throttle >= FULL_THROTTLE
        && (1..=10).contains(&gear)
        && !in_blackout
        && snap.rpm.is_finite()
        && snap.rpm > snap.idle_rpm.max(0.0) + 100.0;
    if !sampling {
        tr.reset_plateau();
        return false;
    }

    // Power curve: maximum observed value in each RPM band.
    if let Some(p) = snap.power_hp {
        if p.is_finite() && p > 0.0 {
            let bin = ((snap.rpm / e.max_rpm) * BINS as f32) as usize;
            let bin = bin.min(BINS - 1);
            if p > e.power[bin] {
                e.power[bin] = p;
                dirty = true;
            }
        }
    }

    // Plateau: RPM held in a narrow band at full throttle indicates the limiter.
    // Wheel slip invalidates it because traction, not the limiter, governs RPM.
    if snap.slip.iter().any(|s| s.abs() > SLIP_TRUST_MAX) {
        tr.reset_plateau();
        return dirty;
    }
    let band = e.max_rpm * PLATEAU_BAND_FRAC;
    match tr.plat_since {
        Some(since) if (snap.rpm - tr.plat_rpm).abs() <= band => {
            let held = now.duration_since(since);
            if held >= PLATEAU_HOLD {
                let frac = tr.plat_rpm / e.max_rpm;
                let trusted = frac >= CUT_TRUST_FRAC || gear < tr.max_gear_seen;
                // Did RPM climb here in this gear? A hard downshift enters from
                // above the limiter while flat or falling and cannot identify it.
                let climbed = tr
                    .gear_min_rpm
                    .is_some_and(|m| tr.plat_rpm - m >= e.max_rpm * CLIMB_MIN_FRAC);
                if frac >= CUT_MIN_FRAC && trusted && climbed {
                    let cut = tr.plat_rpm.min(e.max_rpm * 1.02);
                    // Near the saved limiter, refine upward to the bounce peak.
                    // Far away, replace bad learning only after a long plateau;
                    // a transient cannot erase the saved calibration.
                    let new_cut = match e.cut_rpm {
                        Some(old) if (old - cut).abs() <= band * 2.0 => Some(old.max(cut)),
                        Some(_) if held < PLATEAU_HOLD_REPLACE => None,
                        _ => Some(cut),
                    };
                    if let Some(nc) = new_cut {
                        if e.cut_rpm != Some(nc) {
                            e.cut_rpm = Some(nc);
                            dirty = true;
                        }
                    }
                }
            }
        }
        _ => {
            tr.plat_rpm = snap.rpm;
            tr.plat_since = Some(now);
        }
    }

    dirty
}

impl RevLimitTracker {
    /// In-memory tracker without persistence, used by tests and fallback paths.
    pub fn new() -> Self {
        Self {
            cars: HashMap::new(),
            path: None,
            dirty: false,
            last_save: Instant::now(),
            active: None,
            info: ShiftInfo::default(),
            tr: Transients::default(),
        }
    }

    /// Load saved learning and persist new observations to `path`.
    pub fn with_persistence(path: PathBuf) -> Self {
        let mut t = Self::new();
        t.cars = load_store(&path);
        t.path = Some(path);
        t
    }

    /// `%APPDATA%/forza-hud-native/rev-limits.json`.
    #[must_use]
    pub fn default_path() -> PathBuf {
        crate::paths::config_dir().join("rev-limits.json")
    }

    /// Shift point and limiter for the active vehicle, using defaults without data.
    pub fn info(&self) -> ShiftInfo {
        self.info
    }

    pub fn update(&mut self, snap: &Telemetry, active: bool) {
        self.update_at(snap, active, Instant::now());
    }

    pub fn update_at(&mut self, snap: &Telemetry, active: bool, now: Instant) {
        let Some(key) = car_key(snap) else {
            self.tr.reset_plateau();
            return;
        };
        if self.active.as_deref() != Some(key.as_str()) {
            self.active = Some(key.clone());
            self.tr = Transients::default();
        }

        let entry = self
            .cars
            .entry(key)
            .or_insert_with(|| CarRevData::new(snap.max_rpm));
        // Engine upgrade or swap changed the full-scale RPM, so recalibrate from scratch.
        if snap.max_rpm > 100.0 && (entry.max_rpm - snap.max_rpm).abs() > 1.0 {
            *entry = CarRevData::new(snap.max_rpm);
            self.dirty = true;
        }

        if active {
            self.dirty |= learn(entry, &mut self.tr, snap, now);
        } else {
            self.tr.reset_plateau();
        }
        self.info = compute_info(entry);
        self.maybe_save(now);
    }

    fn maybe_save(&mut self, now: Instant) {
        if self.dirty && now.duration_since(self.last_save) >= SAVE_EVERY {
            self.save_now();
        }
    }

    /// Save immediately if anything changed; call during application shutdown.
    pub fn save_now(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.as_deref() else {
            self.dirty = false;
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = StoreFile {
            version: STORE_VERSION,
            cars: self.cars.clone(),
        };
        match serde_json::to_string(&file) {
            Ok(json) => {
                if let Err(cause) = std::fs::write(path, json) {
                    if SAVE_FAILED.first() {
                        log_warn!(
                            "rev-limit calibration could not be saved to {}: {cause}                              (repeats suppressed)",
                            path.display()
                        );
                    }
                }
            }
            Err(cause) => log_warn!("rev-limit calibration could not be serialized: {cause}"),
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }
}

impl Default for RevLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn load_store(path: &Path) -> HashMap<String, CarRevData> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    match serde_json::from_str::<StoreFile>(&text) {
        Ok(stored) if stored.version < STORE_VERSION => {
            // A superseded schema is discarded rather than migrated: the data is
            // re-learned within a few laps, so there is nothing worth migrating.
            log_info!(
                "rev-limit store {} is v{} (< v{STORE_VERSION}); discarding and re-learning",
                path.display(),
                stored.version
            );
            HashMap::new()
        }
        Ok(mut stored) => {
            for car in stored.cars.values_mut() {
                car.power.resize(BINS, 0.0);
            }
            log_info!(
                "rev-limit calibration: {} cars from {}",
                stored.cars.len(),
                path.display()
            );
            stored.cars
        }
        Err(cause) => {
            log_warn!("rev-limit store {} is unreadable: {cause}", path.display());
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(rpm: f32, gear: u8, accel: f32) -> Telemetry {
        Telemetry {
            race: 1,
            rpm,
            max_rpm: 8000.0,
            idle_rpm: 800.0,
            car_ordinal: 42,
            gear: Some(gear),
            accel: Some(accel),
            power_hp: Some(300.0),
            ..Default::default()
        }
    }

    /// Feed samples every 50 ms starting at `t0 + start_ms`.
    fn feed(tr: &mut RevLimitTracker, t0: Instant, start_ms: u64, samples: &[Telemetry]) {
        for (i, s) in samples.iter().enumerate() {
            let t = t0 + Duration::from_millis(start_ms + i as u64 * 50);
            tr.update_at(s, true, t);
        }
    }

    /// Climb from `from` to `plateau` in 250-RPM steps without accidental
    /// plateaus, then hold for `hold` samples to model a real limiter approach.
    fn climb_then_hold(gear: u8, from: f32, plateau: f32, hold: usize) -> Vec<Telemetry> {
        let mut v = Vec::new();
        let mut rpm = from;
        while rpm < plateau {
            v.push(snap(rpm, gear, 100.0));
            rpm += 250.0;
        }
        v.extend((0..hold).map(|_| snap(plateau, gear, 100.0)));
        v
    }

    #[test]
    fn a_full_throttle_plateau_becomes_the_limiter() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Climb into the limiter at 7800 (97.5% of 8000 ≥ CUT_TRUST_FRAC).
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        let info = tr.info();
        assert!(info.limit_known, "the limiter should have been learned");
        assert!((info.limit_frac - 7800.0 / 8000.0).abs() < 0.015);
        assert!(info.shift_frac < info.limit_frac);
    }

    #[test]
    fn a_hard_downshift_above_the_limiter_is_not_learned() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Accelerating in third gear...
        let up: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 200.0, 3, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &up);
        // ...then a full-throttle hard downshift to second lets the wheels hold
        // the engine at 8100 for over a second, well beyond the blackout. RPM
        // entered from above instead of climbing there, so this is not a limiter.
        let over: Vec<Telemetry> = (0..16).map(|_| snap(8100.0, 2, 100.0)).collect();
        feed(&mut tr, t0, 500, &over);
        assert!(!tr.info().limit_known, "downshift overrev is not a limiter");
    }

    #[test]
    fn bad_saved_limiter_is_corrected_downward() {
        let mut tr = RevLimitTracker::new();
        tr.cars.insert(
            "ord:42".into(),
            CarRevData {
                max_rpm: 8000.0,
                cut_rpm: Some(8160.0), // bad learning, such as an old overrev
                power: vec![0.0; BINS],
                drops: Vec::new(),
            },
        );
        let t0 = Instant::now();
        // Replacement requires a long PLATEAU_HOLD_REPLACE plateau of one second.
        let samples = climb_then_hold(3, 5000.0, 7800.0, 20);
        feed(&mut tr, t0, 0, &samples);
        let cut = tr.cars.get("ord:42").unwrap().cut_rpm.unwrap();
        assert!(
            cut > 7650.0 && cut < 7900.0,
            "the limiter should fall to about 7800, but is {cut}"
        );
    }

    #[test]
    fn downshift_ramp_during_blackout_does_not_count_as_a_climb() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Accelerating in fourth without lifting...
        let up: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 150.0, 4, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &up);
        // ...then downshift two gears. The clutch sweeps RPM upward inside the
        // blackout and the wheels hold the engine above the limiter for over a
        // second. The ramp is not a free engine climb and cannot validate it.
        let mut down: Vec<Telemetry> = [6600.0, 7200.0, 7800.0, 8100.0]
            .iter()
            .map(|&r| snap(r, 2, 100.0))
            .collect();
        down.extend((0..20).map(|_| snap(8150.0, 2, 100.0)));
        feed(&mut tr, t0, 500, &down);
        assert!(
            !tr.info().limit_known,
            "a ramped downshift overrev is not a limiter"
        );
    }

    #[test]
    fn a_slipping_plateau_does_not_replace_a_good_limiter() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Learn the real 7800 limiter during a clean pull in third.
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        assert!(tr.info().limit_known);
        // Downshift to second while TCS holds a slipping wheel at 7000 (87.5%).
        // RPM climbs after the blackout (eight samples = 400 ms at 6000) and
        // stays flat for over a second; without the slip gate this would replace
        // the good limiter with 7000.
        let spin: Vec<Telemetry> = std::iter::repeat_n(6000.0, 8)
            .chain([6500.0, 6800.0])
            .chain(std::iter::repeat_n(7000.0, 22))
            .map(|r| {
                let mut s = snap(r, 2, 100.0);
                s.slip = [1.8, 1.8, 2.4, 2.4];
                s
            })
            .collect();
        feed(&mut tr, t0, 2000, &spin);
        let cut = tr.cars.get("ord:42").unwrap().cut_rpm.unwrap();
        assert!(
            (cut - 7800.0).abs() < 100.0,
            "wheel slip should not alter the limiter, but it became {cut}"
        );
    }

    #[test]
    fn downshift_overshoot_is_ignored() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Settle in third well below the limiter while varying RPM to avoid a plateau.
        let cruise: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 150.0, 3, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &cruise);
        // Downshift to second and overshoot the limiter for about 300 ms, inside the blackout.
        let over: Vec<Telemetry> = (0..6).map(|_| snap(8100.0, 2, 100.0)).collect();
        feed(&mut tr, t0, 500, &over);
        assert!(!tr.info().limit_known, "overshoot is not a limiter");
    }

    #[test]
    fn an_aerodynamic_plateau_in_top_gear_is_rejected() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Only fifth gear has been observed, climbing to 6800 (85% < 93%), so
        // the plateau may be aerodynamic rather than the engine limiter.
        let samples = climb_then_hold(5, 5000.0, 6800.0, 12);
        feed(&mut tr, t0, 0, &samples);
        assert!(!tr.info().limit_known);
    }

    #[test]
    fn a_low_gear_plateau_counts_below_the_top_gear_threshold() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Fourth gear was seen earlier (`max_gear_seen = 4`)...
        let shift_up: Vec<Telemetry> = (0..3).map(|i| snap(4000.0, 4 - i as u8, 30.0)).collect();
        feed(&mut tr, t0, 0, &shift_up);
        // ...so climbing and holding at 6900 (86%) in second is a real limiter.
        let samples = climb_then_hold(2, 5000.0, 6900.0, 12);
        feed(&mut tr, t0, 1000, &samples);
        let info = tr.info();
        assert!(info.limit_known);
        assert!((info.limit_frac - 6900.0 / 8000.0).abs() < 0.015);
    }

    #[test]
    fn a_full_throttle_upshift_measures_rpm_drop() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Climb from 5000 to 7000 in second over ten samples, then shift to third at 4900.
        let up: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 222.0, 2, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &up);
        let third: Vec<Telemetry> = (0..12).map(|_| snap(4900.0, 3, 100.0)).collect();
        feed(&mut tr, t0, 500, &third);
        let e = tr.cars.get("ord:42").unwrap();
        assert_eq!(e.drops.len(), 1);
        assert!((e.drops[0] - 4900.0 / 6998.0).abs() < 0.02);
    }

    #[test]
    fn power_curve_and_rpm_drop_move_the_shift_earlier() {
        // A parabola peaks at about 62.5% of max RPM. With a 0.7 drop ratio, the
        // optimum P(rpm) = P(0.7·rpm) is near 70%, well before the limiter.
        let mut e = CarRevData::new(8000.0);
        e.cut_rpm = Some(7800.0);
        e.drops = vec![0.7, 0.7, 0.7];
        for i in 8..=62 {
            let f = (i as f32 + 0.5) / 64.0;
            let d = (f - 0.625) / 0.4;
            e.power[i] = (1000.0 * (1.0 - d * d)).max(1.0);
        }
        let info = compute_info(&e);
        assert!(info.limit_known);
        assert!(
            info.shift_frac > 0.64 && info.shift_frac < 0.78,
            "shift_frac = {}",
            info.shift_frac
        );
    }

    #[test]
    fn rising_power_until_the_limiter_shifts_at_the_limiter() {
        // Power rises all the way to the limiter, so shift just before it.
        let mut e = CarRevData::new(8000.0);
        e.cut_rpm = Some(7800.0);
        e.drops = vec![0.7, 0.7, 0.7];
        for i in 8..=62 {
            e.power[i] = 100.0 + i as f32 * 10.0;
        }
        let info = compute_info(&e);
        let cut_margin = (7800.0 - 117.0) / 8000.0;
        assert!(
            (info.shift_frac - cut_margin).abs() < 0.02,
            "shift_frac = {}",
            info.shift_frac
        );
    }

    #[test]
    fn a_power_curve_without_drop_ratio_does_not_move_the_shift() {
        // The curve peaks early but no shift has been observed. Without knowing
        // where the next gear lands, keep the recommendation at the limiter.
        let mut e = CarRevData::new(8000.0);
        e.cut_rpm = Some(7800.0);
        for i in 8..=62 {
            let f = (i as f32 + 0.5) / 64.0;
            let d = (f - 0.5) / 0.35;
            e.power[i] = (900.0 * (1.0 - d * d)).max(1.0);
        }
        let info = compute_info(&e);
        let cut_margin = (7800.0 - 117.0) / 8000.0;
        assert!((info.shift_frac - cut_margin).abs() < 0.001);
    }

    #[test]
    fn missing_data_uses_the_default() {
        let mut tr = RevLimitTracker::new();
        tr.update_at(&snap(3000.0, 2, 0.0), true, Instant::now());
        let info = tr.info();
        assert!(!info.limit_known);
        assert!((info.shift_frac - DEFAULT_SHIFT_FRAC).abs() < 1e-6);
        assert_eq!(info.limit_frac, 1.0);
    }

    #[test]
    fn an_engine_upgrade_invalidates_learning() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        assert!(tr.info().limit_known);
        // Same vehicle, new engine: max RPM 9000 triggers recalibration.
        let mut s2 = snap(4000.0, 2, 0.0);
        s2.max_rpm = 9000.0;
        tr.update_at(&s2, true, t0 + Duration::from_secs(5));
        assert!(!tr.info().limit_known);
    }

    #[test]
    fn learning_persists_and_reloads() {
        let dir = std::env::temp_dir().join(format!("revlimit-test-{}", std::process::id()));
        let path = dir.join("rev-limits.json");
        let _ = std::fs::remove_file(&path);

        let mut tr = RevLimitTracker::with_persistence(path.clone());
        let t0 = Instant::now();
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        assert!(tr.info().limit_known);
        tr.save_now();

        let mut tr2 = RevLimitTracker::with_persistence(path.clone());
        tr2.update_at(&snap(1000.0, 1, 0.0), false, Instant::now());
        let info = tr2.info();
        assert!(info.limit_known, "the limiter should reload from disk");
        assert!((info.limit_frac - 7800.0 / 8000.0).abs() < 0.015);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
