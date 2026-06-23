/** Telemetria mínima para decidir se o HUD deve aparecer. */
export interface DrivingTelemetry {
  race: number;
  sled_only: number;
  rpm: number;
  idle_rpm: number;
  speed?: number;
}

/** HUD visível quando o Forza indica corrida/dirigindo (0 = menu/pausa/foto). */
export function isDriving(cur: DrivingTelemetry, ageMs: number): boolean {
  if (cur.sled_only) return false;
  if (ageMs > 1500) return false;
  if (cur.race) return true;
  if (ageMs > 900) return false;
  const spd = cur.speed ?? 0;
  const idle = cur.idle_rpm || 0;
  return spd > 6 || cur.rpm > idle + 200;
}
