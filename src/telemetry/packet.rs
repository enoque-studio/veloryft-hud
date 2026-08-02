//! Decode Forza Data Out UDP packets in Sled, Dash, and FH6 formats.
//! Layout compatible with `backend/packet.py`. Offset-based, zero-copy reads:
//! the parser receives a `&[u8]` and exposes little-endian accessors on demand.

pub const SLED_SIZE: usize = 232;
pub const DASH_SIZE: usize = 311;
pub const FH6_SIZE: usize = 323;

/// Byte offsets in the BASE block; all 58 fields are four bytes (`index * 4`).
mod base {
    pub const IS_RACE_ON: usize = 0;
    pub const TIMESTAMP_MS: usize = 4;
    pub const ENGINE_MAX_RPM: usize = 8;
    pub const ENGINE_IDLE_RPM: usize = 12;
    pub const ENGINE_RPM: usize = 16;
    pub const ACCEL_X: usize = 20; // field 5: lateral acceleration (G)
    pub const ACCEL_Z: usize = 28; // field 7: longitudinal acceleration (G)
                                   // Surface signals for FL/FR/RL/RR infer terrain in the world map.
                                   // They live in BASE, so every packet format carries them.
    pub const WHEEL_ON_RUMBLE_FL: usize = 116; // field 29: rumble strip (0/1)
    pub const WHEEL_IN_PUDDLE_FL: usize = 132; // field 33: puddle/water depth
    pub const SURFACE_RUMBLE_FL: usize = 148; // field 37: surface rumble
    pub const COMBINED_SLIP_FL: usize = 180; // campo 45
    pub const COMBINED_SLIP_FR: usize = 184;
    pub const COMBINED_SLIP_RL: usize = 188;
    pub const COMBINED_SLIP_RR: usize = 192;
}

/// Offsets relative to the DASH block (after BASE and, for FH6, FH6_EXTRA).
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

/// Start of the DASH block in classic Dash packets, after the 232-byte BASE.
const DASH_BASE_CLASSIC: usize = SLED_SIZE;
/// FH6: `car_group` (`u32`) plus two reserved `f32` values before the DASH block.
const FH6_CAR_GROUP: usize = SLED_SIZE;
/// Start of the FH6 DASH block: BASE plus 12-byte FH6_EXTRA (`car_group` + two `f32`s).
const DASH_BASE_FH6: usize = SLED_SIZE + 12;
/// BASE field 53: unique make/model ID.
const BASE_CAR_ORDINAL: usize = 212;
/// DASH relative offset 16: instantaneous power in watts.
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

/// A view over a datagram whose size has already been validated.
pub struct Packet<'a> {
    pub format: Format,
    pub sled_only: bool,
    dash_base: usize, // DASH block start; irrelevant for Sled-only packets
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

    /// Four consecutive `f32` values from a BASE offset (FL, FR, RL, RR).
    #[inline]
    fn base_quad(&self, off: usize) -> [f32; 4] {
        [
            f32_at(self.buf, off),
            f32_at(self.buf, off + 4),
            f32_at(self.buf, off + 8),
            f32_at(self.buf, off + 12),
        ]
    }

    /// Per-wheel surface rumble: near zero on smooth asphalt, high on dirt/grass.
    pub fn surface_rumble(&self) -> [f32; 4] {
        self.base_quad(base::SURFACE_RUMBLE_FL)
    }
    /// Per-wheel puddle/water depth, greater than zero in puddles and fords.
    pub fn wheel_in_puddle(&self) -> [f32; 4] {
        self.base_quad(base::WHEEL_IN_PUDDLE_FL)
    }
    /// Per-wheel rumble-strip contact, which indicates track rather than open terrain.
    pub fn wheel_on_rumble_strip(&self) -> [f32; 4] {
        self.base_quad(base::WHEEL_ON_RUMBLE_FL)
    }
    pub fn car_ordinal(&self) -> i32 {
        i32_at(self.buf, BASE_CAR_ORDINAL)
    }

    /// FH6 car group/class identifier, useful for detecting vehicle changes.
    pub fn car_group(&self) -> Option<u32> {
        if self.format == Format::Fh6 && self.buf.len() >= DASH_BASE_FH6 {
            Some(u32_at(self.buf, FH6_CAR_GROUP))
        } else {
            None
        }
    }

    // --- DASH (valid only when `!sled_only`) ---
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

/// Heuristic that distinguishes FH6 from Dash packets, ported from `_dash_score`.
/// Rust comparisons with NaN return `false`, so they take the invalid branch just
/// like Python's `v == v` check.
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

/// Decode a datagram by size, using the score to disambiguate packets large
/// enough to be FH6. This matches the Python implementation.
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
    fn too_short_datagram_is_rejected() {
        assert!(parse(&[0u8; 100]).is_none());
        assert!(parse(&[0u8; SLED_SIZE - 1]).is_none());
    }

    #[test]
    fn sled_size_is_classified_as_sled() {
        let buf = vec![0u8; SLED_SIZE];
        let p = parse(&buf).unwrap();
        assert_eq!(p.format, Format::Sled);
        assert!(p.sled_only);
    }

    #[test]
    fn dash_reads_fields_at_the_expected_offsets() {
        // Hand-calculated offsets, independent of module constants, catch
        // transpositions: base[i] = i*4 and the Dash block starts at 232.
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
    fn telemetry_converts_wire_units() {
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
