//! Quando o HUD Direct2D deve aparecer — espelha `isRacing` do apex-flow.
//!
//! `is_race_on` = 0 em menu, pausa, foto e cinemáticas. Em free roam exige
//! movimento real do jogador (não só velocidade scriptada de cutscene).

use super::model::Telemetry;

/// Idade máxima do pacote (s) para considerar fallback de movimento.
pub const DRIVING_AGE_S: f32 = 0.9;
/// Sem pacote além disso → HUD some (jogo fechado / UDP parado).
pub const STALE_S: f32 = 1.5;

#[inline]
fn has_driver_input(snap: &Telemetry) -> bool {
    snap.accel.unwrap_or(0.0) > 1.0
        || snap.brake.unwrap_or(0.0) > 1.0
        || snap.clutch.unwrap_or(0.0) > 1.0
        || snap.steer.map(|s| s.abs() > 2.0).unwrap_or(false)
}

/// HUD visível apenas enquanto o jogador está dirigindo (não em menu/cinemática).
pub fn is_hud_visible(snap: &Telemetry, age_s: f32) -> bool {
    if snap.sled_only != 0 || age_s > STALE_S {
        return false;
    }
    // Corrida/crono ativo — some quando o jogo zera `is_race_on` (menu/pausa).
    if snap.race != 0 {
        return true;
    }
    // Free roam: menu, pausa e cutscenes usam race=0.
    if age_s > DRIVING_AGE_S {
        return false;
    }
    let spd = snap.speed.unwrap_or(0.0);
    let rpm = snap.rpm;
    let idle = snap.idle_rpm;

    // Parado no menu / tela de pausa.
    if spd <= 6.0 && rpm <= idle + 200.0 {
        return false;
    }

    // Cinemática: carro se move mas o jogador não manda input (câmera scriptada).
    if spd > 6.0 && !has_driver_input(snap) && rpm <= idle + 400.0 {
        return false;
    }

    spd > 6.0 || rpm > idle + 200.0
}

/// Alias usado pelo gravador de pista / elevação (mesma regra de “dirigindo”).
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
    fn race_ligado_sempre_visivel() {
        let mut t = dash();
        t.race = 1;
        assert!(is_hud_visible(&t, 0.0));
    }

    #[test]
    fn menu_parado_esconde() {
        let t = dash();
        assert!(!is_hud_visible(&t, 0.0));
    }

    #[test]
    fn pausa_zera_race_esconde() {
        let mut t = dash();
        t.race = 1;
        assert!(!is_hud_visible(&t, STALE_S + 0.1));
        t.race = 0;
        t.speed = Some(40.0);
        assert!(!is_hud_visible(&t, DRIVING_AGE_S + 0.1));
    }

    #[test]
    fn free_roam_dirigindo_mostra() {
        let mut t = dash();
        t.speed = Some(20.0);
        t.rpm = 3500.0;
        assert!(is_hud_visible(&t, 0.1));
    }

    #[test]
    fn cinematica_sem_input_esconde() {
        let mut t = dash();
        t.speed = Some(80.0);
        t.rpm = 900.0;
        t.accel = Some(0.0);
        t.brake = Some(0.0);
        t.steer = Some(0.0);
        assert!(!is_hud_visible(&t, 0.0));
    }

    #[test]
    fn free_roam_com_acelerador_mostra() {
        let mut t = dash();
        t.speed = Some(80.0);
        t.rpm = 900.0;
        t.accel = Some(50.0);
        assert!(is_hud_visible(&t, 0.0));
    }

    #[test]
    fn sled_only_nunca_visivel() {
        let mut t = dash();
        t.sled_only = 1;
        t.race = 1;
        assert!(!is_hud_visible(&t, 0.0));
    }
}
