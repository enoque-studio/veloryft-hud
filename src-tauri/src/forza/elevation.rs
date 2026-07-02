//! Perfil de elevação da volta — mapeia o traçado na 1ª volta e reutiliza o
//! perfil fechado nas voltas seguintes (só a bolinha volta ao início).

use super::payload::Telemetry;

const STEP_M: f32 = 3.0;
const MIN_JUMP: f32 = 0.3;
const MAX_JUMP: f32 = 40.0;
const MIN_LOOP_M: f32 = 150.0;
const CLOSE_R: f32 = 30.0;
const LOOP_EXIT_R: f32 = 40.0;
const LOOP_EXIT_CONFIRM_M: f32 = 25.0;
const MIN_BUILD_PTS: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct ElevPoint {
    pub d: f32,
    pub y: f32,
    x: f32,
    z: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ElevationProfile {
    /// Perfil exibido — referência fechada ou traçado em construção.
    pub pts: Vec<ElevPoint>,
    pub min: f32,
    pub max: f32,
    pub cur: f32,
    /// Posição da bolinha ao longo do perfil (m).
    pub marker_d: f32,
    pub marker_y: f32,
    /// Progresso da volta atual (km).
    pub lap_km: f32,
    /// Comprimento do loop fechado (km); 0 enquanto mapeia.
    pub loop_km: f32,
    pub loop_closed: bool,
}

pub struct ElevationTracker {
    loop_pts: Vec<ElevPoint>,
    loop_len: f32,
    building_pts: Vec<ElevPoint>,
    loop_start: Option<(f32, f32)>,
    last_xz: Option<(f32, f32)>,
    off_loop_start: Option<(f32, f32, f32)>,
    off_loop_dist: f32,
    dist: f32,
    last_sample_dist: f32,
    cur_y: f32,
}

impl ElevationTracker {
    pub fn new() -> Self {
        Self {
            loop_pts: Vec::new(),
            loop_len: 0.0,
            building_pts: Vec::new(),
            loop_start: None,
            last_xz: None,
            off_loop_start: None,
            off_loop_dist: 0.0,
            dist: 0.0,
            last_sample_dist: 0.0,
            cur_y: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn active_pts(&self) -> &[ElevPoint] {
        if self.loop_len > 0.0 {
            &self.loop_pts
        } else {
            &self.building_pts
        }
    }

    fn min_max_y(pts: &[ElevPoint]) -> (f32, f32) {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for p in pts {
            if p.y.is_finite() {
                min = min.min(p.y);
                max = max.max(p.y);
            }
        }
        if min.is_finite() && max.is_finite() {
            (min, max)
        } else {
            (0.0, 0.0)
        }
    }

    fn y_at_distance(pts: &[ElevPoint], d: f32) -> f32 {
        if pts.is_empty() {
            return 0.0;
        }
        if d <= pts[0].d {
            return pts[0].y;
        }
        for w in pts.windows(2) {
            let a = &w[0];
            let b = &w[1];
            if d >= a.d && d <= b.d {
                let span = (b.d - a.d).max(0.001);
                let t = (d - a.d) / span;
                return a.y + t * (b.y - a.y);
            }
        }
        pts.last().map(|p| p.y).unwrap_or(0.0)
    }

    fn distance_xz(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    fn point_segment_projection(
        px: f32,
        pz: f32,
        ax: f32,
        az: f32,
        bx: f32,
        bz: f32,
    ) -> (f32, f32) {
        let vx = bx - ax;
        let vz = bz - az;
        let len2 = vx * vx + vz * vz;
        if len2 <= 0.001 {
            let dist = Self::distance_xz((px, pz), (ax, az));
            return (dist, 0.0);
        }
        let t = (((px - ax) * vx + (pz - az) * vz) / len2).clamp(0.0, 1.0);
        let nx = ax + vx * t;
        let nz = az + vz * t;
        (Self::distance_xz((px, pz), (nx, nz)), t)
    }

    fn nearest_loop_position(&self, x: f32, z: f32) -> Option<(f32, f32)> {
        if self.loop_pts.len() < 2 {
            return None;
        }
        let mut best = (f32::INFINITY, 0.0);
        for w in self.loop_pts.windows(2) {
            let a = &w[0];
            let b = &w[1];
            let (dist, t) = Self::point_segment_projection(x, z, a.x, a.z, b.x, b.z);
            if dist < best.0 {
                best = (dist, a.d + (b.d - a.d) * t);
            }
        }
        best.0.is_finite().then_some(best)
    }

    fn push_build_sample(&mut self, d: f32, x: f32, y: f32, z: f32) {
        self.building_pts.push(ElevPoint { d, y, x, z });
    }

    fn clear_off_loop(&mut self) {
        self.off_loop_start = None;
        self.off_loop_dist = 0.0;
    }

    fn start_new_path_from_off_loop(&mut self, x: f32, y: f32, z: f32) {
        let (sx, sy, sz) = self.off_loop_start.unwrap_or((x, y, z));
        self.loop_pts.clear();
        self.building_pts.clear();
        self.loop_len = 0.0;
        self.dist = 0.0;
        self.last_sample_dist = 0.0;
        self.loop_start = Some((sx, sz));
        self.last_xz = Some((sx, sz));
        self.cur_y = sy;
        self.push_build_sample(0.0, sx, sy, sz);

        let d = Self::distance_xz((sx, sz), (x, z));
        if d >= MIN_JUMP {
            self.dist = d;
            self.last_xz = Some((x, z));
            self.cur_y = y;
            if d >= STEP_M {
                self.last_sample_dist = d;
                self.push_build_sample(d, x, y, z);
            }
        }
        self.clear_off_loop();
    }

    fn try_close_loop(&mut self, x: f32, y: f32, z: f32) -> bool {
        let Some((sx, sz)) = self.loop_start else {
            return false;
        };
        if self.dist < MIN_LOOP_M {
            return false;
        }
        let close = ((x - sx).powi(2) + (z - sz).powi(2)).sqrt();
        if close >= CLOSE_R {
            return false;
        }

        if self.loop_len <= 0.0 && self.building_pts.len() >= MIN_BUILD_PTS {
            self.loop_len = self
                .building_pts
                .last()
                .map(|p| p.d)
                .filter(|&d| d >= MIN_LOOP_M)
                .unwrap_or(0.0);
            if self.loop_len > 0.0 {
                if self.dist - self.last_sample_dist >= MIN_JUMP {
                    self.push_build_sample(self.dist, x, y, z);
                    self.loop_len = self.dist;
                }
                self.loop_pts = self.building_pts.clone();
                self.building_pts.clear();
                self.clear_off_loop();
            }
        }

        self.dist = 0.0;
        self.last_sample_dist = 0.0;
        true
    }

    fn hard_reset_path(&mut self, x: f32, z: f32, y: f32) {
        self.loop_pts.clear();
        self.building_pts.clear();
        self.loop_len = 0.0;
        self.dist = 0.0;
        self.last_sample_dist = 0.0;
        self.loop_start = Some((x, z));
        self.last_xz = Some((x, z));
        self.clear_off_loop();
        self.cur_y = y;
        self.push_build_sample(0.0, x, y, z);
    }

    /// Integra um pacote quando `active` (dirigindo / corrida).
    pub fn update(&mut self, t: &Telemetry, active: bool) {
        if !active || t.sled_only != 0 {
            self.last_xz = None;
            self.clear_off_loop();
            return;
        }
        let Some([x, y, z]) = t.pos else {
            return;
        };
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return;
        }

        if self.loop_start.is_none() {
            self.hard_reset_path(x, z, y);
            return;
        }

        let mut step_jump = 0.0;
        if let Some((lx, lz)) = self.last_xz {
            let jump = Self::distance_xz((x, z), (lx, lz));
            step_jump = jump;
            if jump > MAX_JUMP {
                // Loop já fechado nunca reconstrói: um salto (rewind/teleporte/respawn)
                // só reancora a posição. Assim, depois de fechar, a telemetria fica só
                // deslizando a bolinha em vez de voltar a "carregar" o perfil do zero.
                if self.loop_len > 0.0 {
                    self.last_xz = Some((x, z));
                    self.cur_y = y;
                    self.clear_off_loop();
                    return;
                }
                self.hard_reset_path(x, z, y);
                return;
            }
            if jump >= MIN_JUMP {
                self.dist += jump;
            }
        }

        self.last_xz = Some((x, z));
        self.cur_y = y;

        if self.loop_len > 0.0 {
            if let Some((dist_to_loop, loop_d)) = self.nearest_loop_position(x, z) {
                if dist_to_loop <= LOOP_EXIT_R {
                    self.dist = loop_d.rem_euclid(self.loop_len);
                    self.last_sample_dist = self.dist;
                    self.clear_off_loop();
                    return;
                }
            }

            if self.off_loop_start.is_none() {
                self.off_loop_start = Some((x, y, z));
                self.off_loop_dist = 0.0;
            } else {
                self.off_loop_dist += step_jump;
            }
            if self.off_loop_dist >= LOOP_EXIT_CONFIRM_M {
                self.start_new_path_from_off_loop(x, y, z);
                return;
            }

            if self.dist >= self.loop_len {
                self.dist = self.dist.rem_euclid(self.loop_len);
                self.last_sample_dist = self.dist;
            }
            return;
        }

        if self.try_close_loop(x, y, z) {
            return;
        }

        if self.building_pts.is_empty() || self.dist - self.last_sample_dist >= STEP_M {
            self.last_sample_dist = self.dist;
            self.push_build_sample(self.dist, x, y, z);
        }
    }

    pub fn profile(&self) -> ElevationProfile {
        let pts = self.active_pts().to_vec();
        let (min, max) = Self::min_max_y(&pts);
        let loop_closed = self.loop_len > 0.0;
        let marker_d = if loop_closed {
            self.dist.rem_euclid(self.loop_len)
        } else {
            self.dist
        };
        let marker_y = if loop_closed {
            Self::y_at_distance(&self.loop_pts, marker_d)
        } else if pts.is_empty() {
            self.cur_y
        } else {
            Self::y_at_distance(&pts, marker_d)
        };

        ElevationProfile {
            lap_km: marker_d / 1000.0,
            loop_km: self.loop_len / 1000.0,
            loop_closed,
            pts,
            min,
            max,
            cur: self.cur_y,
            marker_d,
            marker_y,
        }
    }
}

impl Default for ElevationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dash_pos(x: f32, y: f32, z: f32) -> Telemetry {
        Telemetry {
            sled_only: 0,
            race: 1,
            pos: Some([x, y, z]),
            ..Default::default()
        }
    }

    #[test]
    fn acumula_distancia_e_amostras() {
        let mut tr = ElevationTracker::new();
        tr.update(&dash_pos(0.0, 100.0, 0.0), true);
        tr.update(&dash_pos(5.0, 101.0, 0.0), true);
        tr.update(&dash_pos(10.0, 102.0, 0.0), true);
        let p = tr.profile();
        assert!(p.lap_km > 0.0);
        assert!(!p.pts.is_empty());
        assert_eq!(p.cur, 102.0);
    }

    #[test]
    fn fecha_loop_e_reutiliza_perfil() {
        let mut tr = ElevationTracker::new();
        for i in 0..25 {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        let len_building = tr.profile().pts.len();
        assert!(len_building >= MIN_BUILD_PTS);

        for i in (0..25).rev() {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        tr.update(&dash_pos(20.0, 100.0, 0.0), true);

        let closed = tr.profile();
        assert!(
            closed.loop_closed,
            "loop deveria fechar ao voltar ao início"
        );
        let len_at_close = closed.pts.len();
        assert!(len_at_close >= MIN_BUILD_PTS);
        assert!(closed.loop_km > 0.0);

        for i in 1..=8 {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        let after = tr.profile();
        assert_eq!(after.pts.len(), len_at_close);
        assert!(after.marker_d > closed.marker_d);
    }

    #[test]
    fn salto_apos_fechar_mantem_perfil() {
        let mut tr = ElevationTracker::new();
        for i in 0..25 {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        for i in (0..25).rev() {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        tr.update(&dash_pos(20.0, 100.0, 0.0), true);
        let closed = tr.profile();
        assert!(closed.loop_closed);
        let pts_len = closed.pts.len();
        let loop_km = closed.loop_km;

        // Teleporte/rewind (salto > MAX_JUMP) depois de fechar não pode reconstruir
        // o perfil — só reancora; a telemetria segue deslizando a bolinha.
        tr.update(&dash_pos(20.0 + 5000.0, 130.0, 0.0), true);
        let after = tr.profile();
        assert!(
            after.loop_closed,
            "loop fechado não pode reabrir após salto"
        );
        assert_eq!(after.pts.len(), pts_len, "perfil deve ser preservado");
        assert_eq!(after.loop_km, loop_km);
    }

    #[test]
    fn loop_fechado_so_move_marcador_circular() {
        let mut tr = ElevationTracker::new();
        for i in 0..25 {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        for i in (0..25).rev() {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        tr.update(&dash_pos(20.0, 100.0, 0.0), true);
        let closed = tr.profile();
        assert!(closed.loop_closed);
        let pts_len = closed.pts.len();
        let loop_m = closed.loop_km * 1000.0;

        for _ in 0..3 {
            for i in 1..25 {
                tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
            }
            for i in (0..25).rev() {
                tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
            }
        }

        let after = tr.profile();
        assert!(after.loop_closed);
        assert_eq!(after.pts.len(), pts_len);
        assert!(after.marker_d >= 0.0 && after.marker_d < loop_m);
        assert!(after.lap_km < after.loop_km);
    }

    #[test]
    fn sair_do_loop_reinicia_metrificacao() {
        let mut tr = ElevationTracker::new();
        for i in 0..25 {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        for i in (0..25).rev() {
            tr.update(&dash_pos(20.0 + i as f32 * 4.0, 100.0, 0.0), true);
        }
        tr.update(&dash_pos(20.0, 100.0, 0.0), true);
        assert!(tr.profile().loop_closed);

        for i in 1..=28 {
            tr.update(&dash_pos(20.0, 100.0 + i as f32, i as f32 * 4.0), true);
        }

        let after = tr.profile();
        assert!(!after.loop_closed, "sair do loop deve iniciar novo perfil");
        assert_eq!(after.loop_km, 0.0);
        assert!(after.lap_km > 0.02);
        assert!(!after.pts.is_empty());
    }

    #[test]
    fn menu_nao_integra() {
        let mut tr = ElevationTracker::new();
        tr.update(&dash_pos(0.0, 50.0, 0.0), true);
        tr.update(
            &Telemetry {
                race: 0,
                pos: Some([5.0, 51.0, 0.0]),
                ..Default::default()
            },
            false,
        );
        assert!(tr.last_xz.is_none());
    }
}
