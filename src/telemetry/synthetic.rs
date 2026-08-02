//! Building Forza-shaped datagrams, for tests and for driving the HUD without
//! the game.
//!
//! This is the inverse of [`super::packet::parse`] and it deliberately lives
//! next to it: both read the same offset tables, so a change to the wire format
//! cannot update one and silently leave the other behind. The round-trip tests
//! at the bottom of this module are what enforce that.
//!
//! No real telemetry capture is redistributed with this project. Everything a
//! test or the `synthetic_telemetry` example feeds the parser is generated here
//! from plain numbers, so the fixtures carry no personal data and no content
//! owned by anyone else.

use super::packet::{DASH_SIZE, FH6_SIZE, SLED_SIZE};

/// A frame to encode. Fields left at their defaults encode as zero, which is
/// what an idle car sitting in a menu looks like on the wire.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// `is_race_on`: 0 in menus, pause screens, replays and photo mode.
    pub race_on: i32,
    pub timestamp_ms: u32,
    pub engine_max_rpm: f32,
    pub engine_idle_rpm: f32,
    pub engine_rpm: f32,
    /// Lateral acceleration, in g.
    pub accel_x: f32,
    /// Longitudinal acceleration, in g.
    pub accel_z: f32,
    pub car_ordinal: i32,
    /// FH6 only; ignored by the Sled and Dash encoders.
    pub car_group: u32,
    /// World position `[x, y, z]`, in metres.
    pub position: [f32; 3],
    /// Speed in metres per second, as the game sends it.
    pub speed_ms: f32,
    /// Instantaneous engine power, in watts.
    pub power_watts: f32,
    /// Fuel remaining, 0.0 to 1.0.
    pub fuel: f32,
    /// Tire temperatures in °C, ordered front-left, front-right, rear-left,
    /// rear-right.
    pub tire_temps: [f32; 4],
    /// Combined slip per wheel, same ordering as [`Frame::tire_temps`].
    pub combined_slip: [f32; 4],
    /// Surface rumble per wheel: near zero on smooth asphalt.
    pub surface_rumble: [f32; 4],
    /// Puddle depth per wheel: above zero through fords and standing water.
    pub wheel_in_puddle: [f32; 4],
    /// Throttle, 0 to 255.
    pub accel_input: u8,
    /// Brake, 0 to 255.
    pub brake_input: u8,
    /// Clutch, 0 to 255.
    pub clutch_input: u8,
    /// Gear; 0 is reverse and 1 is first.
    pub gear: u8,
    /// Steering, -127 to 127.
    pub steer: i8,
}

impl Frame {
    /// A plausible mid-corner frame: third gear, part throttle, on the brakes.
    ///
    /// Used as the starting point for tests that only care about one or two
    /// fields, so that the rest stay in ranges the parser considers sane.
    #[must_use]
    pub fn driving() -> Self {
        Self {
            race_on: 1,
            timestamp_ms: 1_000,
            engine_max_rpm: 7_500.0,
            engine_idle_rpm: 800.0,
            engine_rpm: 5_200.0,
            accel_x: 0.8,
            accel_z: -0.4,
            car_ordinal: 247,
            car_group: 3,
            position: [120.5, 42.0, -318.25],
            speed_ms: 44.0,
            power_watts: 220_000.0,
            fuel: 0.75,
            tire_temps: [82.0, 85.5, 79.0, 80.5],
            combined_slip: [0.2, 0.25, 0.15, 0.18],
            surface_rumble: [0.01, 0.01, 0.01, 0.01],
            wheel_in_puddle: [0.0; 4],
            accel_input: 180,
            brake_input: 40,
            clutch_input: 0,
            gear: 3,
            steer: -35,
        }
    }
}

// Offsets. These mirror `super::packet`; see the module docs for why they are
// written out twice rather than shared through a helper — the encoder must fail
// loudly in a round-trip test if the two ever disagree.
const BASE_IS_RACE_ON: usize = 0;
const BASE_TIMESTAMP_MS: usize = 4;
const BASE_ENGINE_MAX_RPM: usize = 8;
const BASE_ENGINE_IDLE_RPM: usize = 12;
const BASE_ENGINE_RPM: usize = 16;
const BASE_ACCEL_X: usize = 20;
const BASE_ACCEL_Z: usize = 28;
const BASE_WHEEL_IN_PUDDLE_FL: usize = 132;
const BASE_SURFACE_RUMBLE_FL: usize = 148;
const BASE_COMBINED_SLIP_FL: usize = 180;
const BASE_CAR_ORDINAL: usize = 212;

const DASH_POS_X: usize = 0;
const DASH_SPEED: usize = 12;
const DASH_POWER: usize = 16;
const DASH_TIRE_TEMP_FL: usize = 24;
const DASH_FUEL: usize = 44;
const DASH_ACCEL_INPUT: usize = 71;
const DASH_BRAKE_INPUT: usize = 72;
const DASH_CLUTCH_INPUT: usize = 73;
const DASH_GEAR: usize = 75;
const DASH_STEER: usize = 76;

/// Start of the DASH block in a classic Dash packet.
const DASH_BASE_CLASSIC: usize = SLED_SIZE;
/// FH6 inserts `car_group` plus two reserved floats before the DASH block.
const DASH_BASE_FH6: usize = SLED_SIZE + 12;

/// Encodes `frame` as a Sled packet: the BASE block only, no dash fields.
#[must_use]
pub fn encode_sled(frame: &Frame) -> Vec<u8> {
    let mut buffer = vec![0_u8; SLED_SIZE];
    write_base(&mut buffer, frame);
    buffer
}

/// Encodes `frame` as a classic Dash packet.
#[must_use]
pub fn encode_dash(frame: &Frame) -> Vec<u8> {
    let mut buffer = vec![0_u8; DASH_SIZE];
    write_base(&mut buffer, frame);
    write_dash(&mut buffer, DASH_BASE_CLASSIC, frame);
    buffer
}

/// Encodes `frame` as a Forza Horizon 6 packet.
#[must_use]
pub fn encode_fh6(frame: &Frame) -> Vec<u8> {
    let mut buffer = vec![0_u8; FH6_SIZE];
    write_base(&mut buffer, frame);
    write_u32(&mut buffer, SLED_SIZE, frame.car_group);
    write_dash(&mut buffer, DASH_BASE_FH6, frame);
    buffer
}

fn write_base(buffer: &mut [u8], frame: &Frame) {
    write_i32(buffer, BASE_IS_RACE_ON, frame.race_on);
    write_u32(buffer, BASE_TIMESTAMP_MS, frame.timestamp_ms);
    write_f32(buffer, BASE_ENGINE_MAX_RPM, frame.engine_max_rpm);
    write_f32(buffer, BASE_ENGINE_IDLE_RPM, frame.engine_idle_rpm);
    write_f32(buffer, BASE_ENGINE_RPM, frame.engine_rpm);
    write_f32(buffer, BASE_ACCEL_X, frame.accel_x);
    write_f32(buffer, BASE_ACCEL_Z, frame.accel_z);
    write_quad(buffer, BASE_WHEEL_IN_PUDDLE_FL, frame.wheel_in_puddle);
    write_quad(buffer, BASE_SURFACE_RUMBLE_FL, frame.surface_rumble);
    write_quad(buffer, BASE_COMBINED_SLIP_FL, frame.combined_slip);
    write_i32(buffer, BASE_CAR_ORDINAL, frame.car_ordinal);
}

fn write_dash(buffer: &mut [u8], dash_base: usize, frame: &Frame) {
    for (index, value) in frame.position.iter().enumerate() {
        write_f32(buffer, dash_base + DASH_POS_X + index * 4, *value);
    }
    write_f32(buffer, dash_base + DASH_SPEED, frame.speed_ms);
    write_f32(buffer, dash_base + DASH_POWER, frame.power_watts);
    write_quad(buffer, dash_base + DASH_TIRE_TEMP_FL, frame.tire_temps);
    write_f32(buffer, dash_base + DASH_FUEL, frame.fuel);
    buffer[dash_base + DASH_ACCEL_INPUT] = frame.accel_input;
    buffer[dash_base + DASH_BRAKE_INPUT] = frame.brake_input;
    buffer[dash_base + DASH_CLUTCH_INPUT] = frame.clutch_input;
    buffer[dash_base + DASH_GEAR] = frame.gear;
    buffer[dash_base + DASH_STEER] = frame.steer as u8;
}

fn write_quad(buffer: &mut [u8], offset: usize, values: [f32; 4]) {
    for (index, value) in values.iter().enumerate() {
        write_f32(buffer, offset + index * 4, *value);
    }
}

fn write_f32(buffer: &mut [u8], offset: usize, value: f32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_i32(buffer: &mut [u8], offset: usize, value: i32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::packet::{self, Format};

    #[test]
    fn an_fh6_frame_survives_a_round_trip_through_the_parser() {
        let frame = Frame::driving();
        let datagram = encode_fh6(&frame);
        let parsed = packet::parse(&datagram).expect("an FH6-sized datagram parses");

        assert_eq!(parsed.format, Format::Fh6);
        assert_eq!(parsed.is_race_on(), frame.race_on);
        assert_eq!(parsed.timestamp_ms(), frame.timestamp_ms);
        assert_eq!(parsed.engine_max_rpm(), frame.engine_max_rpm);
        assert_eq!(parsed.engine_idle_rpm(), frame.engine_idle_rpm);
        assert_eq!(parsed.engine_rpm(), frame.engine_rpm);
        assert_eq!(parsed.accel_x(), frame.accel_x);
        assert_eq!(parsed.accel_z(), frame.accel_z);
        assert_eq!(parsed.car_ordinal(), frame.car_ordinal);
        assert_eq!(parsed.car_group(), Some(frame.car_group));
        assert_eq!(parsed.pos(), frame.position);
        assert_eq!(parsed.speed(), frame.speed_ms);
        assert_eq!(parsed.power(), frame.power_watts);
        assert_eq!(parsed.fuel(), frame.fuel);
        assert_eq!(parsed.tire_temps(), frame.tire_temps);
        assert_eq!(parsed.combined_slip(), frame.combined_slip);
        assert_eq!(parsed.surface_rumble(), frame.surface_rumble);
        assert_eq!(parsed.wheel_in_puddle(), frame.wheel_in_puddle);
        assert_eq!(parsed.accel_input(), frame.accel_input);
        assert_eq!(parsed.brake_input(), frame.brake_input);
        assert_eq!(parsed.clutch_input(), frame.clutch_input);
        assert_eq!(parsed.gear(), frame.gear);
        assert_eq!(parsed.steer(), frame.steer);
    }

    #[test]
    fn a_dash_frame_survives_a_round_trip_through_the_parser() {
        let frame = Frame::driving();
        let datagram = encode_dash(&frame);
        let parsed = packet::parse(&datagram).expect("a Dash-sized datagram parses");

        assert_eq!(parsed.format, Format::Dash);
        assert_eq!(parsed.speed(), frame.speed_ms);
        assert_eq!(parsed.tire_temps(), frame.tire_temps);
        assert_eq!(parsed.gear(), frame.gear);
        assert_eq!(parsed.steer(), frame.steer);
        assert_eq!(parsed.pos(), frame.position);
    }

    #[test]
    fn a_sled_frame_carries_base_fields_and_nothing_else() {
        let frame = Frame::driving();
        let datagram = encode_sled(&frame);
        let parsed = packet::parse(&datagram).expect("a Sled-sized datagram parses");

        assert_eq!(parsed.format, Format::Sled);
        assert!(parsed.sled_only);
        assert_eq!(parsed.engine_rpm(), frame.engine_rpm);
        assert_eq!(parsed.car_ordinal(), frame.car_ordinal);
        // Sled has no dash block, so there is no car group to report.
        assert_eq!(parsed.car_group(), None);
    }

    #[test]
    fn the_encoders_produce_exactly_the_documented_sizes() {
        let frame = Frame::default();

        assert_eq!(encode_sled(&frame).len(), SLED_SIZE);
        assert_eq!(encode_dash(&frame).len(), DASH_SIZE);
        assert_eq!(encode_fh6(&frame).len(), FH6_SIZE);
    }

    #[test]
    fn fh6_and_dash_place_the_dash_block_at_different_offsets() {
        // If the parser ever confuses the two, speed and gear read as garbage.
        // This is the property the format sniffing exists to protect.
        let frame = Frame::driving();

        let fh6 = encode_fh6(&frame);
        let dash = encode_dash(&frame);
        assert_ne!(
            &fh6[SLED_SIZE..DASH_SIZE],
            &dash[SLED_SIZE..DASH_SIZE],
            "the twelve extra FH6 bytes must shift the dash block"
        );
    }
}
