import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./App.css";

/** Frame suavizado emitido pelo núcleo Rust (`forza/display.rs`). */
interface DisplayFrame {
  rpm_frac: number;
  near: boolean;
  gear?: number;
  speed: number;
  accel: number;
  brake: number;
  car_name: string;
  power_cv: number;
  show_power: boolean;
  alpha: number;
  fmt: string;
  rpm_raw: number;
  pkt: number;
}

function gearLabel(gear?: number): string {
  if (gear == null) return "–";
  if (gear === 0) return "R";
  return String(gear);
}

function powerLabel(show: boolean, cv: number): string {
  if (!show || !Number.isFinite(cv)) return "";
  return `${Math.round(cv)} hp`;
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

  const [status, setStatus] = createSignal<{ fmt: string; rpm: number; pkt: number } | null>(null);
  const [hz, setHz] = createSignal(0);

  onMount(() => {
    let unlisten: UnlistenFn | undefined;
    let lastEvt = 0;
    let ema = 0;

    listen<DisplayFrame>("display", (e) => {
      const d = e.payload;
      const now = performance.now();

      if (lastEvt) {
        const dt = now - lastEvt;
        if (dt > 0) ema = ema ? ema * 0.9 + (1000 / dt) * 0.1 : 1000 / dt;
      }
      lastEvt = now;
      setHz(Math.round(ema));
      setStatus({ fmt: d.fmt, rpm: d.rpm_raw, pkt: d.pkt });

      const visible = d.alpha > 0.02;
      if (hudLive) {
        hudLive.style.opacity = d.alpha.toFixed(3);
        hudLive.style.visibility = visible ? "visible" : "hidden";
      }
      if (statusEl) {
        statusEl.style.opacity = d.alpha < 0.35 ? "1" : "0";
        statusEl.style.visibility = d.alpha < 0.35 ? "visible" : "hidden";
      }

      if (rpmFill) {
        rpmFill.style.clipPath = `inset(0 ${((1 - d.rpm_frac) * 100).toFixed(2)}% 0 0)`;
      }
      if (accelFill) accelFill.style.transform = `scaleX(${d.accel.toFixed(4)})`;
      if (brakeFill) brakeFill.style.transform = `scaleX(${d.brake.toFixed(4)})`;

      rpmTrack?.classList.toggle("rpm--redline", d.near);
      gearEl?.classList.toggle("gear--shift", d.near);

      if (speedEl) speedEl.textContent = String(Math.round(d.speed));
      if (gearEl) gearEl.textContent = gearLabel(d.gear);
      if (carEl) carEl.textContent = d.car_name;
      if (powerEl) powerEl.textContent = powerLabel(d.show_power, d.power_cv);
    }).then((f) => (unlisten = f));

    onCleanup(() => unlisten?.());
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
