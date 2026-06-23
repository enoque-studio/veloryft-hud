import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isDriving } from "./lib/driving";
import "./App.css";

interface Telemetry {
  race: number;
  fmt: string;
  ts: number;
  rpm: number;
  max_rpm: number;
  idle_rpm: number;
  car_ordinal: number;
  car_name?: string;
  car_group?: number;
  sled_only: number;
  speed?: number;
  gear?: number;
  accel?: number;
  brake?: number;
  clutch?: number;
  steer?: number;
  fuel?: number;
  power_hp?: number;
  _pkt: number;
}

const SHIFT_AT = 0.92;
const LEARN_MIN_THROTTLE = 90;
const LEARN_MIN_OVER_IDLE = 2200;
const LEARN_MIN_REPORTED_FRAC = 0.45;
const LEARN_SUSPICIOUS_REPORTED_FRAC = 0.88;
const TAU_BAR = 0.032;
const TAU_TEXT = 0.11;
const TAU_FADE = 0.07;
const STALE_MS = 1500;

function expSmooth(cur: number, target: number, dt: number, tau: number): number {
  if (tau <= 0 || dt <= 0) return target;
  return cur + (target - cur) * (1 - Math.exp(-dt / tau));
}

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

function carSignature(d: Telemetry): number {
  const maxB = d.max_rpm > 100 ? Math.fround(d.max_rpm) : 0;
  const idleB = d.idle_rpm > 0 ? Math.fround(d.idle_rpm) : 0;
  const group = d.car_group ?? 0;
  const ord = d.car_ordinal ?? 0;
  return maxB * 1e6 + idleB * 1e3 + group + ord * 1e9;
}

function carLabel(d: Telemetry): string {
  if (d.car_name) return d.car_name;
  if (d.car_ordinal) return `#${d.car_ordinal}`;
  return "–";
}

function powerLabel(hp?: number): string {
  if (hp == null || !Number.isFinite(hp)) return "";
  return `${Math.round(hp)} hp`;
}

function App() {
  let hudLive: HTMLDivElement | undefined;
  let statusEl: HTMLDivElement | undefined;
  let rpmTrack: HTMLDivElement | undefined;
  let rpmFill: HTMLDivElement | undefined;
  let accelFill: HTMLDivElement | undefined;
  let brakeFill: HTMLDivElement | undefined;
  let gearEl: HTMLDivElement | undefined;
  let speedEl: HTMLSpanElement | undefined;
  let carEl: HTMLSpanElement | undefined;
  let powerEl: HTMLSpanElement | undefined;

  const raw = {
    rpm: 0,
    rpmFrac: 0,
    accel: 0,
    brake: 0,
    speed: 0,
    gear: null as number | null,
    lastAt: 0,
  };
  const disp = { rpmFrac: 0, accel: 0, brake: 0, speed: 0, visible: 0 };
  let stickyMaxRpm = 0;
  let observedPeakRpm = 0;
  let learnedMaxRpm = 0;
  let lastRawRpm = 0;
  let lastRawGear: number | null = null;
  let carSig = 0;
  let lastTel: Telemetry | null = null;
  let lastNear: boolean | null = null;
  let lastGear = "";
  let lastSpeed = "";
  let lastCar = "";
  let lastPower = "";

  const [status, setStatus] = createSignal<{ fmt: string; rpm: number; pkt: number } | null>(null);
  const [hz, setHz] = createSignal(0);

  function resetDisplay() {
    disp.rpmFrac = raw.rpmFrac;
    disp.accel = raw.accel;
    disp.brake = raw.brake;
    disp.speed = raw.speed;
  }

  function effectiveMaxRpm(): number {
    return learnedMaxRpm > 100 ? learnedMaxRpm : stickyMaxRpm;
  }

  function resetCarLearning(d: Telemetry) {
    observedPeakRpm = Math.max(0, d.rpm);
    learnedMaxRpm = 0;
    lastRawRpm = Math.max(0, d.rpm);
    lastRawGear = d.gear ?? null;
  }

  function commitObservedPeak(d: Telemetry) {
    const peak = observedPeakRpm;
    const reported = stickyMaxRpm;
    if (peak <= d.idle_rpm + LEARN_MIN_OVER_IDLE || reported <= 100) return;
    const frac = peak / reported;
    const looksLikeLowRevCar =
      frac >= LEARN_MIN_REPORTED_FRAC && frac < LEARN_SUSPICIOUS_REPORTED_FRAC;
    if (looksLikeLowRevCar) learnedMaxRpm = Math.max(learnedMaxRpm, peak / SHIFT_AT);
  }

  function learnEffectiveRedline(d: Telemetry) {
    const rpm = Math.max(0, d.rpm);
    const gear = d.gear ?? null;
    const upshift = lastRawGear != null && gear != null && gear > lastRawGear && lastRawGear > 0;
    if (upshift && (d.accel ?? 0) >= LEARN_MIN_THROTTLE) {
      commitObservedPeak(d);
      observedPeakRpm = rpm;
    }
    if ((d.accel ?? 0) >= LEARN_MIN_THROTTLE && gear !== 0) {
      observedPeakRpm = Math.max(observedPeakRpm, rpm);
    }
    lastRawRpm = rpm;
    lastRawGear = gear;
  }

  function applyTelemetry(d: Telemetry) {
    lastTel = d;
    if (d.max_rpm > 100) stickyMaxRpm = d.max_rpm;

    const sig = carSignature(d);
    if (sig !== 0 && carSig !== 0 && sig !== carSig) {
      resetCarLearning(d);
      const max = effectiveMaxRpm();
      raw.rpm = d.rpm;
      raw.rpmFrac = max ? clamp01(d.rpm / max) : 0;
      raw.accel = (d.accel ?? 0) / 100;
      raw.brake = (d.brake ?? 0) / 100;
      raw.speed = d.speed ?? 0;
      resetDisplay();
    }
    if (sig !== 0) {
      if (carSig === 0) resetCarLearning(d);
      carSig = sig;
    }
    learnEffectiveRedline(d);

    const max = effectiveMaxRpm();
    raw.rpm = d.rpm;
    raw.rpmFrac = max ? clamp01(d.rpm / max) : 0;
    raw.accel = (d.accel ?? 0) / 100;
    raw.brake = (d.brake ?? 0) / 100;
    raw.speed = d.speed ?? 0;
    raw.gear = d.gear ?? null;
    raw.lastAt = performance.now();
  }

  onMount(() => {
    let unlisten: UnlistenFn | undefined;
    let lastEvt = 0;
    let ema = 0;

    listen<Telemetry>("telemetry", (e) => {
      const d = e.payload;
      applyTelemetry(d);

      if (lastEvt) {
        const dt = raw.lastAt - lastEvt;
        if (dt > 0) ema = ema ? ema * 0.9 + (1000 / dt) * 0.1 : 1000 / dt;
      }
      lastEvt = raw.lastAt;
      setHz(Math.round(ema));
      setStatus({ fmt: d.fmt, rpm: Math.round(d.rpm), pkt: d._pkt });

      const car = carLabel(d);
      if (carEl && car !== lastCar) {
        carEl.textContent = car;
        lastCar = car;
      }
      const pow = powerLabel(d.power_hp);
      if (powerEl && pow !== lastPower) {
        powerEl.textContent = pow;
        lastPower = pow;
      }
    }).then((f) => (unlisten = f));

    let raf = 0;
    let lastFrame = 0;
    const tick = (now: number) => {
      const dt = lastFrame ? Math.min(0.1, (now - lastFrame) / 1000) : 0;
      lastFrame = now;

      const ageMs = raw.lastAt ? now - raw.lastAt : Infinity;
      const fresh = ageMs < STALE_MS;
      const driving = lastTel ? isDriving(lastTel, ageMs) : false;
      disp.visible = expSmooth(disp.visible, driving ? 1 : 0, dt, TAU_FADE);

      if (hudLive) {
        const v = disp.visible;
        hudLive.style.opacity = v.toFixed(3);
        hudLive.style.visibility = v < 0.02 ? "hidden" : "visible";
      }
      if (statusEl) {
        const showStatus = disp.visible < 0.35;
        statusEl.style.opacity = showStatus ? "1" : "0";
        statusEl.style.visibility = showStatus ? "visible" : "hidden";
      }

      disp.rpmFrac = expSmooth(disp.rpmFrac, fresh ? raw.rpmFrac : 0, dt, TAU_BAR);
      disp.accel = expSmooth(disp.accel, fresh ? raw.accel : 0, dt, TAU_BAR);
      disp.brake = expSmooth(disp.brake, fresh ? raw.brake : 0, dt, TAU_BAR);
      disp.speed = expSmooth(disp.speed, fresh ? raw.speed : 0, dt, TAU_TEXT);

      if (rpmFill) rpmFill.style.clipPath = `inset(0 ${((1 - disp.rpmFrac) * 100).toFixed(2)}% 0 0)`;
      if (accelFill) accelFill.style.transform = `scaleX(${disp.accel.toFixed(4)})`;
      if (brakeFill) brakeFill.style.transform = `scaleX(${disp.brake.toFixed(4)})`;

      const near = disp.rpmFrac >= SHIFT_AT;
      if (near !== lastNear) {
        rpmTrack?.classList.toggle("rpm--redline", near);
        gearEl?.classList.toggle("gear--shift", near);
        lastNear = near;
      }

      if (speedEl) {
        const s = String(Math.round(disp.speed));
        if (s !== lastSpeed) {
          speedEl.textContent = s;
          lastSpeed = s;
        }
      }
      if (gearEl) {
        const g = raw.gear == null ? "–" : raw.gear === 0 ? "R" : String(raw.gear);
        if (g !== lastGear) {
          gearEl.textContent = g;
          lastGear = g;
        }
      }

      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);

    onCleanup(() => {
      unlisten?.();
      cancelAnimationFrame(raf);
    });
  });

  return (
    <div class="hud">
      <div class="hud__live" ref={hudLive}>
        <div class="cluster">
          <div class="gear cluster__card" ref={gearEl}>–</div>
          <div class="cluster__right cluster__card">
            <div class="rpm" ref={rpmTrack}>
              <div class="rpm__fill" ref={rpmFill} />
            </div>
            <div class="cluster__meta">
              <span class="cluster__car" ref={carEl}>–</span>
              <span class="cluster__power" ref={powerEl} />
            </div>
            <div class="speed">
              <span class="speed__val" ref={speedEl}>0</span>
              <span class="speed__unit">km/h</span>
            </div>
            <div class="pedal">
              <span class="pedal__label">BRAKE</span>
              <div class="pedal__track">
                <div class="pedal__fill pedal__fill--brake" ref={brakeFill} />
              </div>
            </div>
            <div class="pedal">
              <span class="pedal__label">THROTTLE</span>
              <div class="pedal__track">
                <div class="pedal__fill pedal__fill--accel" ref={accelFill} />
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="status" ref={statusEl}>
        <Show
          when={status()}
          fallback={<span class="status__off">waiting for UDP 20066 — Data Out → 127.0.0.1:20066</span>}
        >
          {(s) => (
            <span>
              {s().fmt} · {s().rpm} rpm · {hz()} Hz · pkt {s().pkt}
            </span>
          )}
        </Show>
      </div>
    </div>
  );
}

export default App;
