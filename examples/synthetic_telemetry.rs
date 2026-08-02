//! Sends synthetic Forza telemetry so the overlay can be exercised without the
//! game.
//!
//! Run the overlay in one terminal and this in another:
//!
//! ```text
//! cargo run
//! cargo run --example synthetic_telemetry
//! ```
//!
//! The HUD should appear within a second and show a car accelerating through
//! the gears around a closed loop, with tire temperatures warming up.
//!
//! Everything sent here is generated from the formulas below. No recorded
//! telemetry is redistributed with this project, so nothing here carries
//! anyone's driving data.
//!
//! ## Options
//!
//! ```text
//! --port <number>   destination port          (default 20066)
//! --format sled|dash|fh6                      (default fh6)
//! --idle            send with is_race_on = 0, which hides the HUD
//! --seconds <n>     stop after n seconds      (default: run until Ctrl+C)
//! ```
//!
//! This example is not part of any release build: `cargo build --release`
//! does not compile it.

use std::env;
use std::net::UdpSocket;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use veloryft_hud::telemetry::synthetic::{self, Frame};

/// Matches the overlay's default listening port.
const DEFAULT_PORT: u16 = 20066;
/// 60 Hz, close to what the game sends.
const SEND_INTERVAL: Duration = Duration::from_millis(16);

/// Length of the simulated lap, in seconds, before the loop repeats.
const LAP_SECONDS: f32 = 40.0;
const REDLINE_RPM: f32 = 7_500.0;
const IDLE_RPM: f32 = 800.0;
/// Radius of the circular course, in metres.
const COURSE_RADIUS_M: f32 = 400.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wire {
    Sled,
    Dash,
    Fh6,
}

struct Options {
    port: u16,
    wire: Wire,
    idle: bool,
    seconds: Option<f32>,
}

fn main() -> ExitCode {
    let options = match parse_arguments() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("usage: cargo run --example synthetic_telemetry -- [--port N] [--format sled|dash|fh6] [--idle] [--seconds N]");
            return ExitCode::FAILURE;
        }
    };

    // Bind an ephemeral port; the overlay only cares where packets arrive.
    let socket = match UdpSocket::bind(("127.0.0.1", 0)) {
        Ok(socket) => socket,
        Err(cause) => {
            eprintln!("error: could not open a UDP socket: {cause}");
            return ExitCode::FAILURE;
        }
    };
    let destination = ("127.0.0.1", options.port);

    println!(
        "sending synthetic {} telemetry to 127.0.0.1:{} every {} ms{}",
        match options.wire {
            Wire::Sled => "Sled",
            Wire::Dash => "Dash",
            Wire::Fh6 => "FH6",
        },
        options.port,
        SEND_INTERVAL.as_millis(),
        if options.idle {
            ", with is_race_on = 0 (the HUD stays hidden)"
        } else {
            ""
        }
    );
    println!("press Ctrl+C to stop");

    let started = Instant::now();
    loop {
        let elapsed = started.elapsed().as_secs_f32();
        if options.seconds.is_some_and(|limit| elapsed >= limit) {
            println!("done after {elapsed:.1}s");
            return ExitCode::SUCCESS;
        }

        let frame = simulate(elapsed, options.idle);
        let datagram = match options.wire {
            Wire::Sled => synthetic::encode_sled(&frame),
            Wire::Dash => synthetic::encode_dash(&frame),
            Wire::Fh6 => synthetic::encode_fh6(&frame),
        };

        if let Err(cause) = socket.send_to(&datagram, destination) {
            eprintln!("error: send failed: {cause}");
            return ExitCode::FAILURE;
        }
        thread::sleep(SEND_INTERVAL);
    }
}

/// One frame of a car lapping a circular course, accelerating through the gears
/// and braking into the second half of each lap.
fn simulate(elapsed_seconds: f32, idle: bool) -> Frame {
    let lap_position = (elapsed_seconds % LAP_SECONDS) / LAP_SECONDS;

    // Throttle for the first two thirds of the lap, brake into the last third.
    let on_power = lap_position < 0.66;
    let throttle = if on_power { 1.0 } else { 0.0 };
    let brake = if on_power { 0.0 } else { 0.7 };

    // Six gears, each sweeping the rev range once.
    let gear = 1 + ((lap_position * 6.0) as u8).min(5);
    let within_gear = (lap_position * 6.0).fract();
    let rpm = if on_power {
        IDLE_RPM + within_gear * (REDLINE_RPM - IDLE_RPM)
    } else {
        IDLE_RPM + (1.0 - lap_position) * 2_000.0
    };

    let speed_ms = 8.0 + f32::from(gear) * 12.0 * if on_power { 1.0 } else { 0.6 };

    // Position walks a circle so the elevation profile and, when enabled, the
    // track map see a closed loop rather than a straight line to infinity.
    let angle = lap_position * std::f32::consts::TAU;
    let position = [
        angle.cos() * COURSE_RADIUS_M,
        // A gentle rise and fall, so the elevation panel has something to draw.
        (angle * 2.0).sin() * 12.0,
        angle.sin() * COURSE_RADIUS_M,
    ];

    // Tires warm up over the first few laps and settle, fronts hotter than rears.
    let warmth = (elapsed_seconds / 90.0).min(1.0);
    let base_temp = 40.0 + warmth * 45.0;
    let cornering = (angle.sin().abs()) * 12.0;

    Frame {
        race_on: i32::from(!idle),
        timestamp_ms: (elapsed_seconds * 1_000.0) as u32,
        engine_max_rpm: REDLINE_RPM,
        engine_idle_rpm: IDLE_RPM,
        engine_rpm: rpm,
        // Lateral load peaks where the circle turns hardest.
        accel_x: angle.sin() * 1.1,
        accel_z: if on_power { 0.45 } else { -0.9 },
        car_ordinal: 247,
        car_group: 3,
        position,
        speed_ms,
        power_watts: rpm / REDLINE_RPM * 260_000.0 * throttle,
        fuel: (1.0 - elapsed_seconds / 1_800.0).max(0.0),
        tire_temps: [
            base_temp + cornering + 6.0,
            base_temp + cornering + 4.0,
            base_temp + cornering,
            base_temp + cornering - 2.0,
        ],
        combined_slip: [0.2, 0.22, 0.18, 0.19],
        // Smooth asphalt all the way round.
        surface_rumble: [0.01; 4],
        wheel_in_puddle: [0.0; 4],
        accel_input: (throttle * 255.0) as u8,
        brake_input: (brake * 255.0) as u8,
        clutch_input: 0,
        gear,
        steer: (angle.sin() * 60.0) as i8,
    }
}

fn parse_arguments() -> Result<Options, String> {
    let mut options = Options {
        port: DEFAULT_PORT,
        wire: Wire::Fh6,
        idle: false,
        seconds: None,
    };

    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--port" => {
                let value = arguments.next().ok_or("--port needs a number")?;
                options.port = value.parse().map_err(|_| format!("bad port: {value}"))?;
            }
            "--format" => {
                let value = arguments.next().ok_or("--format needs a value")?;
                options.wire = match value.as_str() {
                    "sled" => Wire::Sled,
                    "dash" => Wire::Dash,
                    "fh6" => Wire::Fh6,
                    other => return Err(format!("unknown format: {other}")),
                };
            }
            "--idle" => options.idle = true,
            "--seconds" => {
                let value = arguments.next().ok_or("--seconds needs a number")?;
                options.seconds = Some(
                    value
                        .parse()
                        .map_err(|_| format!("bad duration: {value}"))?,
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(options)
}
