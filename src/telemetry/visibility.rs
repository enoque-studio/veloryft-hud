//! Decide when the Direct2D HUD should appear, mirroring apex-flow's `isRacing`.
//!
//! `is_race_on == 0` in menus, pause, photo mode, and cinematics. Free roam
//! requires real driver activity rather than scripted cutscene motion.

use super::model::Telemetry;

/// Maximum packet age in seconds for considering the movement fallback.
pub const DRIVING_AGE_S: f32 = 0.9;
/// Without a packet beyond this threshold, hide the HUD (game closed or UDP stopped).
pub const STALE_S: f32 = 1.5;

#[inline]
fn has_driver_input(snap: &Telemetry) -> bool {
    snap.accel.unwrap_or(0.0) > 1.0
        || snap.brake.unwrap_or(0.0) > 1.0
        || snap.clutch.unwrap_or(0.0) > 1.0
        || snap.steer.map(|s| s.abs() > 2.0).unwrap_or(false)
}

/// Show the HUD only while the player is driving, not in menus or cinematics.
pub fn is_hud_visible(snap: &Telemetry, age_s: f32) -> bool {
    if snap.sled_only != 0 || age_s > STALE_S {
        return false;
    }
    // An active race/timer remains visible until the game clears `is_race_on`.
    if snap.race != 0 {
        return true;
    }
    // In free roam, menus, pause, and cutscenes use race=0.
    if age_s > DRIVING_AGE_S {
        return false;
    }
    let spd = snap.speed.unwrap_or(0.0);
    let rpm = snap.rpm;
    let idle = snap.idle_rpm;

    // Parked in a menu or pause screen.
    if spd <= 6.0 && rpm <= idle + 200.0 {
        return false;
    }

    // Cinematic: the car moves without driver input under a scripted camera.
    if spd > 6.0 && !has_driver_input(snap) && rpm <= idle + 400.0 {
        return false;
    }

    spd > 6.0 || rpm > idle + 200.0
}

/// Alias used by track/elevation recording, with the same driving rule.
pub fn is_driving(snap: &Telemetry, age_s: f32) -> bool {
    is_hud_visible(snap, age_s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::model::Telemetry;

    fn dash() -> Telemetry {
        Telemetry {
            sled_only: 0,
            race: 0,
            rpm: 500.0,
            idle_rpm: 800.0,
            speed: Some(0.0),
            ..Default::default()
        }
    }

    #[test]
    fn an_active_race_is_always_visible() {
        let mut t = dash();
        t.race = 1;
        assert!(is_hud_visible(&t, 0.0));
    }

    #[test]
    fn a_stationary_menu_hides_the_hud() {
        let t = dash();
        assert!(!is_hud_visible(&t, 0.0));
    }

    #[test]
    fn clearing_race_state_on_pause_hides_the_hud() {
        let mut t = dash();
        t.race = 1;
        assert!(!is_hud_visible(&t, STALE_S + 0.1));
        t.race = 0;
        t.speed = Some(40.0);
        assert!(!is_hud_visible(&t, DRIVING_AGE_S + 0.1));
    }

    #[test]
    fn driving_in_free_roam_shows_the_hud() {
        let mut t = dash();
        t.speed = Some(20.0);
        t.rpm = 3500.0;
        assert!(is_hud_visible(&t, 0.1));
    }

    #[test]
    fn a_cinematic_without_input_hides_the_hud() {
        let mut t = dash();
        t.speed = Some(80.0);
        t.rpm = 900.0;
        t.accel = Some(0.0);
        t.brake = Some(0.0);
        t.steer = Some(0.0);
        assert!(!is_hud_visible(&t, 0.0));
    }

    #[test]
    fn throttle_input_in_free_roam_shows_the_hud() {
        let mut t = dash();
        t.speed = Some(80.0);
        t.rpm = 900.0;
        t.accel = Some(50.0);
        assert!(is_hud_visible(&t, 0.0));
    }

    #[test]
    fn a_sled_only_packet_is_never_visible() {
        let mut t = dash();
        t.sled_only = 1;
        t.race = 1;
        assert!(!is_hud_visible(&t, 0.0));
    }
}
