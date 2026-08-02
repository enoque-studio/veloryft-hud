//! Decodifica os pacotes UDP "Data Out" do Forza (Sled / Dash / FH6).
//! Layout idêntico ao de `backend/packet.py`. Leitura por offset, zero-copy:
//! recebemos um `&[u8]` e expomos acessores que lêem little-endian sob demanda.

pub const SLED_SIZE: usize = 232;
pub const DASH_SIZE: usize = 311;
pub const FH6_SIZE: usize = 323;

/// Offsets (bytes) no bloco BASE — todos os 58 campos têm 4 bytes (índice * 4).
mod base {
    pub const IS_RACE_ON: usize = 0;
    pub const TIMESTAMP_MS: usize = 4;
    pub const ENGINE_MAX_RPM: usize = 8;
    pub const ENGINE_IDLE_RPM: usize = 12;
    pub const ENGINE_RPM: usize = 16;
    pub const ACCEL_X: usize = 20; // campo 5 — aceleração lateral (G)
    pub const ACCEL_Z: usize = 28; // campo 7 — aceleração longitudinal (G)
                                   // Sinais de superfície (4 rodas FL/FR/RL/RR), usados para inferir o tipo de
                                   // terreno/via no mapa do mundo. Ficam no bloco BASE → válidos em qualquer formato.
    pub const WHEEL_ON_RUMBLE_FL: usize = 116; // campo 29 — zebra (0/1)
    pub const WHEEL_IN_PUDDLE_FL: usize = 132; // campo 33 — profundidade de poça/água
    pub const SURFACE_RUMBLE_FL: usize = 148; // campo 37 — vibração da superfície
    pub const COMBINED_SLIP_FL: usize = 180; // campo 45
    pub const COMBINED_SLIP_FR: usize = 184;
    pub const COMBINED_SLIP_RL: usize = 188;
    pub const COMBINED_SLIP_RR: usize = 192;
}

/// Offsets relativos ao início do bloco DASH (após BASE, e após FH6_EXTRA no FH6).
mod dash {
    pub const POS_X: usize = 0;
    pub const POS_Y: usize = 4;
    pub const POS_Z: usize = 8;
    pub const SPEED: usize = 12;
    pub const TIRE_TEMP_FL: usize = 24;
    pub const TIRE_TEMP_FR: usize = 28;
    pub const TIRE_TEMP_RL: usize = 32;
    pub const TIRE_TEMP_RR: usize = 36;
    pub const FUEL: usize = 44;
    pub const ACCEL_INPUT: usize = 71; // u8
    pub const BRAKE_INPUT: usize = 72; // u8
    pub const CLUTCH_INPUT: usize = 73; // u8
    pub const GEAR: usize = 75; // u8
    pub const STEER: usize = 76; // i8
}

/// Início do bloco DASH no Dash clássico (logo após os 232 bytes de BASE).
const DASH_BASE_CLASSIC: usize = SLED_SIZE;
/// FH6: car_group (u32) + 2× f32 reservados antes do bloco DASH.
const FH6_CAR_GROUP: usize = SLED_SIZE;
/// Início do bloco DASH no FH6 (BASE + FH6_EXTRA de 12 bytes: car_group + 2 f32).
const DASH_BASE_FH6: usize = SLED_SIZE + 12;
/// BASE campo 53 — ID único make/model.
const BASE_CAR_ORDINAL: usize = 212;
/// DASH rel 16 — potência instantânea (watts).
const DASH_POWER: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Sled,
    Dash,
    Fh6,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Sled => "Sled",
            Format::Dash => "Dash",
            Format::Fh6 => "FH6",
        }
    }
}

#[inline]
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
#[inline]
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
#[inline]
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Visão sobre um datagrama já validado por tamanho.
pub struct Packet<'a> {
    pub format: Format,
    pub sled_only: bool,
    dash_base: usize, // início do bloco DASH (irrelevante se sled_only)
    buf: &'a [u8],
}

impl<'a> Packet<'a> {
    // --- BASE ---
    pub fn is_race_on(&self) -> i32 {
        i32_at(self.buf, base::IS_RACE_ON)
    }
    pub fn timestamp_ms(&self) -> u32 {
        u32_at(self.buf, base::TIMESTAMP_MS)
    }
    pub fn engine_max_rpm(&self) -> f32 {
        f32_at(self.buf, base::ENGINE_MAX_RPM)
    }
    pub fn engine_idle_rpm(&self) -> f32 {
        f32_at(self.buf, base::ENGINE_IDLE_RPM)
    }
    pub fn engine_rpm(&self) -> f32 {
        f32_at(self.buf, base::ENGINE_RPM)
    }
    pub fn accel_x(&self) -> f32 {
        f32_at(self.buf, base::ACCEL_X)
    }
    pub fn accel_z(&self) -> f32 {
        f32_at(self.buf, base::ACCEL_Z)
    }
    pub fn combined_slip(&self) -> [f32; 4] {
        [
            f32_at(self.buf, base::COMBINED_SLIP_FL),
            f32_at(self.buf, base::COMBINED_SLIP_FR),
            f32_at(self.buf, base::COMBINED_SLIP_RL),
            f32_at(self.buf, base::COMBINED_SLIP_RR),
        ]
    }

    /// 4 valores consecutivos de f32 a partir de um offset BASE (FL, FR, RL, RR).
    #[inline]
    fn base_quad(&self, off: usize) -> [f32; 4] {
        [
            f32_at(self.buf, off),
            f32_at(self.buf, off + 4),
            f32_at(self.buf, off + 8),
            f32_at(self.buf, off + 12),
        ]
    }

    /// Vibração da superfície por roda — ~0 em asfalto liso, alto em terra/grama.
    pub fn surface_rumble(&self) -> [f32; 4] {
        self.base_quad(base::SURFACE_RUMBLE_FL)
    }
    /// Profundidade de poça/água por roda — > 0 em vaus e poças.
    pub fn wheel_in_puddle(&self) -> [f32; 4] {
        self.base_quad(base::WHEEL_IN_PUDDLE_FL)
    }
    /// Roda sobre zebra (rumble strip) por roda — pista, não terreno aberto.
    pub fn wheel_on_rumble_strip(&self) -> [f32; 4] {
        self.base_quad(base::WHEEL_ON_RUMBLE_FL)
    }
    pub fn car_ordinal(&self) -> i32 {
        i32_at(self.buf, BASE_CAR_ORDINAL)
    }

    /// Identificador de grupo/classe do carro (FH6). Útil para detectar troca de veículo.
    pub fn car_group(&self) -> Option<u32> {
        if self.format == Format::Fh6 && self.buf.len() >= DASH_BASE_FH6 {
            Some(u32_at(self.buf, FH6_CAR_GROUP))
        } else {
            None
        }
    }

    // --- DASH (válido apenas quando !sled_only) ---
    #[inline]
    fn d_f32(&self, rel: usize) -> f32 {
        f32_at(self.buf, self.dash_base + rel)
    }
    #[inline]
    fn d_u8(&self, rel: usize) -> u8 {
        self.buf[self.dash_base + rel]
    }
    #[inline]
    fn d_i8(&self, rel: usize) -> i8 {
        self.buf[self.dash_base + rel] as i8
    }

    pub fn speed(&self) -> f32 {
        self.d_f32(dash::SPEED)
    }
    pub fn power(&self) -> f32 {
        self.d_f32(DASH_POWER)
    }
    pub fn fuel(&self) -> f32 {
        self.d_f32(dash::FUEL)
    }
    pub fn gear(&self) -> u8 {
        self.d_u8(dash::GEAR)
    }
    pub fn accel_input(&self) -> u8 {
        self.d_u8(dash::ACCEL_INPUT)
    }
    pub fn brake_input(&self) -> u8 {
        self.d_u8(dash::BRAKE_INPUT)
    }
    pub fn clutch_input(&self) -> u8 {
        self.d_u8(dash::CLUTCH_INPUT)
    }
    pub fn steer(&self) -> i8 {
        self.d_i8(dash::STEER)
    }
    pub fn pos(&self) -> [f32; 3] {
        [
            self.d_f32(dash::POS_X),
            self.d_f32(dash::POS_Y),
            self.d_f32(dash::POS_Z),
        ]
    }
    pub fn pos_x(&self) -> f32 {
        self.d_f32(dash::POS_X)
    }
    pub fn pos_y(&self) -> f32 {
        self.d_f32(dash::POS_Y)
    }
    pub fn pos_z(&self) -> f32 {
        self.d_f32(dash::POS_Z)
    }
    pub fn tire_temps(&self) -> [f32; 4] {
        [
            self.d_f32(dash::TIRE_TEMP_FL),
            self.d_f32(dash::TIRE_TEMP_FR),
            self.d_f32(dash::TIRE_TEMP_RL),
            self.d_f32(dash::TIRE_TEMP_RR),
        ]
    }
}

/// Heurística de desempate FH6 × Dash — porta de `_dash_score`.
/// Comparações com NaN retornam `false` em Rust, então caem no ramo "inválido"
/// igual ao `v == v` do Python.
fn dash_score(p: &Packet) -> i32 {
    let mut score = 0;

    let speed = p.speed();
    score += if (0.0..180.0).contains(&speed) { 4 } else { -6 };

    let fuel = p.fuel();
    score += if (-0.01..=1.05).contains(&fuel) {
        3
    } else {
        -4
    };

    score += if p.gear() <= 12 { 2 } else { -2 };

    let tires_ok = p.tire_temps().iter().all(|&t| (-40.0..=500.0).contains(&t));
    score += if tires_ok { 4 } else { -4 };

    let pos_ok = p.pos().iter().all(|&v| v.abs() < 1_000_000.0);
    score += if pos_ok { 3 } else { -4 };

    score
}

/// Decodifica um datagrama. Escolhe o formato pelo tamanho e, quando o pacote é
/// grande o suficiente para ser FH6, desempata por pontuação (igual ao Python).
pub fn parse(data: &[u8]) -> Option<Packet<'_>> {
    let n = data.len();
    if n >= DASH_SIZE {
        let dash = Packet {
            format: Format::Dash,
            sled_only: false,
            dash_base: DASH_BASE_CLASSIC,
            buf: data,
        };
        if n >= FH6_SIZE {
            let fh6 = Packet {
                format: Format::Fh6,
                sled_only: false,
                dash_base: DASH_BASE_FH6,
                buf: data,
            };
            if dash_score(&fh6) > dash_score(&dash) {
                return Some(fh6);
            }
        }
        return Some(dash);
    }
    if n >= SLED_SIZE {
        return Some(Packet {
            format: Format::Sled,
            sled_only: true,
            dash_base: 0,
            buf: data,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_f32(b: &mut [u8], off: usize, v: f32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_i32(b: &mut [u8], off: usize, v: i32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn datagrama_pequeno_e_descartado() {
        assert!(parse(&[0u8; 100]).is_none());
        assert!(parse(&[0u8; SLED_SIZE - 1]).is_none());
    }

    #[test]
    fn tamanho_sled_vira_sled() {
        let buf = vec![0u8; SLED_SIZE];
        let p = parse(&buf).unwrap();
        assert_eq!(p.format, Format::Sled);
        assert!(p.sled_only);
    }

    #[test]
    fn dash_le_campos_nos_offsets_certos() {
        // Offsets calculados à mão (independente das constantes do módulo) para
        // pegar transposições: base[i] = i*4; bloco dash começa em 232.
        let mut buf = vec![0u8; DASH_SIZE];
        put_i32(&mut buf, 0, 1); // is_race_on (campo 0)
        put_f32(&mut buf, 8, 8000.0); // engine_max_rpm (campo 2)
        put_f32(&mut buf, 16, 6543.0); // engine_rpm (campo 4)
        put_i32(&mut buf, BASE_CAR_ORDINAL, 292); // car_ordinal (campo 53)
        put_f32(&mut buf, 232 + 12, 50.0); // speed (dash rel 12)
        put_f32(&mut buf, 232 + DASH_POWER, 745_700.0); // ~1000 cv
        buf[232 + 75] = 4; // gear (dash rel 75)
        buf[232 + 71] = 255; // accel_input (dash rel 71)
        buf[232 + 76] = (-100i8) as u8; // steer (dash rel 76)

        let p = parse(&buf).unwrap();
        assert_eq!(p.format, Format::Dash);
        assert!(!p.sled_only);
        assert_eq!(p.is_race_on(), 1);
        assert_eq!(p.engine_rpm(), 6543.0);
        assert_eq!(p.engine_max_rpm(), 8000.0);
        assert_eq!(p.car_ordinal(), 292);
        assert_eq!(p.speed(), 50.0);
        assert!((p.power() - 745_700.0).abs() < 0.1);
        assert_eq!(p.gear(), 4);
        assert_eq!(p.accel_input(), 255);
        assert_eq!(p.steer(), -100);
    }

    #[test]
    fn payload_converte_unidades() {
        let mut buf = vec![0u8; DASH_SIZE];
        put_f32(&mut buf, 16, 6000.0); // rpm
        put_f32(&mut buf, 232 + 12, 50.0); // speed 50 m/s -> 180 km/h
        buf[232 + 71] = 255; // accel 255 -> ~100%
        let p = parse(&buf).unwrap();
        let t = crate::telemetry::model::build(&p);
        assert_eq!(t.rpm, 6000.0);
        assert!((t.speed.unwrap() - 180.0).abs() < 0.01);
        assert!((t.accel.unwrap() - 100.0).abs() < 0.5);
    }
}
