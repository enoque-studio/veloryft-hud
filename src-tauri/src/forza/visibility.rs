//! Quando o HUD deve aparecer — espelha `isRacing` do apex-flow.
//! `is_race_on` = 0 em menu/pausa/foto; fallback por velocidade/RPM recentes.

use super::payload::Telemetry;

/// Idade máxima do pacote (s) para considerar fallback de movimento.
pub const DRIVING_AGE_S: f32 = 0.9;
/// Sem pacote além disso → HUD some (jogo fechado / UDP parado).
pub const STALE_S: f32 = 1.5;

pub fn is_driving(snap: &Telemetry, age_s: f32) -> bool {
    if snap.sled_only != 0 || age_s > STALE_S {
        return false;
    }
    if snap.race != 0 {
        return true;
    }
    if age_s > DRIVING_AGE_S {
        return false;
    }
    let spd = snap.speed.unwrap_or(0.0);
    let rpm = snap.rpm;
    let idle = snap.idle_rpm;
    spd > 6.0 || rpm > idle + 200.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forza::payload::Telemetry;

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
    fn race_ligado_sempre_dirige() {
        let mut t = dash();
        t.race = 1;
        assert!(is_driving(&t, 0.0));
    }

    #[test]
    fn menu_sem_movimento_esconde() {
        let t = dash();
        assert!(!is_driving(&t, 0.0));
    }

    #[test]
    fn pacote_velho_esconde() {
        let mut t = dash();
        t.race = 1;
        assert!(!is_driving(&t, STALE_S + 0.1));
        t.race = 0;
        t.speed = Some(40.0);
        assert!(!is_driving(&t, DRIVING_AGE_S + 0.1));
    }

    #[test]
    fn fallback_velocidade() {
        let mut t = dash();
        t.speed = Some(20.0);
        assert!(is_driving(&t, 0.1));
    }

    #[test]
    fn sled_only_nunca_dirige() {
        let mut t = dash();
        t.sled_only = 1;
        t.race = 1;
        assert!(!is_driving(&t, 0.0));
    }
}
