//! Mapeia o frame interno de telemetria (usado no render) para o DTO de rede
//! compartilhado `veloryft_core::TelemetrySample`, enviado no upload de voltas ao
//! veloryft-back. Fonte única de verdade: o tipo de rede vive no veloryft-core.

use veloryft_core::TelemetrySample;

use super::payload::Telemetry;

impl Telemetry {
    /// Converte o frame para uma amostra de rede. `t_ms` é o offset em ms desde o
    /// início da volta. Campos de dashboard ausentes (formato Sled) viram 0.
    pub fn to_sample(&self, t_ms: u32) -> TelemetrySample {
        TelemetrySample {
            t_ms,
            speed_kmh: self.speed.unwrap_or(0.0),
            rpm: self.rpm,
            gear: self.gear.unwrap_or(0) as i8,
            throttle: self.accel.unwrap_or(0.0),
            brake: self.brake.unwrap_or(0.0),
            steer: self.steer.unwrap_or(0.0),
            lat_g: self.lat_g,
            long_g: self.long_g,
            // TODO: expor a posição do mundo no Telemetry (o packet tem) p/ o mapa.
            pos: [0.0; 3],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapeia_campos_de_dashboard() {
        let t = Telemetry {
            rpm: 6000.0,
            lat_g: 1.2,
            speed: Some(180.0),
            gear: Some(4),
            accel: Some(100.0),
            brake: Some(0.0),
            steer: Some(-25.0),
            ..Default::default()
        };
        let s = t.to_sample(1500);
        assert_eq!(s.t_ms, 1500);
        assert_eq!(s.speed_kmh, 180.0);
        assert_eq!(s.gear, 4);
        assert_eq!(s.throttle, 100.0);
        assert_eq!(s.steer, -25.0);
        assert_eq!(s.rpm, 6000.0);
        assert_eq!(s.lat_g, 1.2);
    }

    #[test]
    fn formato_sled_zera_campos_de_dashboard() {
        // Sem campos de dashboard (Option::None) → 0.0, sem panic.
        let t = Telemetry { sled_only: 1, ..Default::default() };
        let s = t.to_sample(0);
        assert_eq!(s.speed_kmh, 0.0);
        assert_eq!(s.gear, 0);
        assert_eq!(s.throttle, 0.0);
    }
}
