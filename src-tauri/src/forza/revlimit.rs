//! Limite de giro por carro, aprendido em tempo real (redline learning).
//!
//! O pacote do Forza traz `engine_max_rpm` (fundo de escala do conta-giros),
//! mas o **corte real** (limitador) e o ponto ótimo de troca variam por carro
//! e por upgrade. Este módulo observa a telemetria a todo gás e aprende, por
//! carro (`car_ordinal`):
//!
//! - **Corte** — platô de RPM sustentado com o acelerador cravado = limitador.
//!   O platô só vale se o giro **subiu** até ele na mesma marcha *depois* do
//!   blackout pós-troca: numa reduzida a rampa da embreagem varre o RPM para
//!   cima e a roda pode forçar o motor *acima* do corte (entra por cima,
//!   flat/caindo) — nada disso é limitador. Roda patinando (`slip` alto)
//!   também invalida o platô: o RPM ali é regido pela tração (TCS/terra), não
//!   pelo corte. Platôs em marcha alta podem ser limite aerodinâmico (não de
//!   giro), então só valem abaixo de `CUT_TRUST_FRAC` quando há marcha acima
//!   da atual. Um platô confiável bem diferente do corte salvo o substitui
//!   (auto-correção de aprendizado ruim), mas precisa segurar
//!   `PLATEAU_HOLD_REPLACE` — transiente curto não derruba calibração boa.
//! - **Curva de potência + queda de RPM por troca** — potência máxima
//!   observada por faixa de RPM e a razão `rpm_depois / rpm_antes` dos
//!   upshifts. A troca ideal é o maior RPM em que segurar a marcha ainda
//!   rende mais potência do que a próxima renderia (`P(rpm) ≥ P(rpm·razão)`);
//!   sem razão aprendida, recomenda logo antes do corte.
//!
//! O aprendizado persiste em `%APPDATA%/forza-hud-native/rev-limits.json`:
//! trocar de carro e voltar mantém a calibração. Upgrade/swap de motor muda
//! `engine_max_rpm` e invalida o registro (reaprende do zero).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::payload::Telemetry;

/// Nº de faixas de RPM da curva de potência (0..max_rpm).
const BINS: usize = 64;
/// Ponto de troca default (fração de max_rpm) enquanto nada foi aprendido.
pub const DEFAULT_SHIFT_FRAC: f32 = 0.92;
/// Acelerador mínimo (0..100) para amostrar aprendizado.
const FULL_THROTTLE: f32 = 88.0;
/// Janela morta após troca de marcha — ignora overshoot de downshift.
const POST_SHIFT_BLACKOUT: Duration = Duration::from_millis(400);
/// Meia-largura da banda do platô (fração de max_rpm).
const PLATEAU_BAND_FRAC: f32 = 0.008;
/// Tempo mínimo dentro da banda para declarar limitador.
const PLATEAU_HOLD: Duration = Duration::from_millis(250);
/// Substituir um corte já salvo exige platô bem mais longo — a auto-correção
/// não pode deixar um transiente de reduzida derrubar calibração boa.
const PLATEAU_HOLD_REPLACE: Duration = Duration::from_millis(700);
/// Slip combinado acima disto = roda patinando/travada — o RPM é regido pela
/// tração (TCS, terra), não pelo limitador, e o platô não vale.
const SLIP_TRUST_MAX: f32 = 1.2;
/// Platô só vale como corte acima desta fração de max_rpm.
const CUT_MIN_FRAC: f32 = 0.82;
/// Acima desta fração o platô vale mesmo na marcha mais alta já vista.
const CUT_TRUST_FRAC: f32 = 0.93;
/// O giro precisa ter subido isto (fração de max_rpm) na marcha atual antes do
/// platô — reduzida forte entra por cima do corte e não conta como limitador.
const CLIMB_MIN_FRAC: f32 = 0.02;
/// Versão do arquivo — bump descarta aprendizado de versões com bugs.
/// v3: cortes de v2 podiam ser corrompidos por platô de reduzida/patinada.
const STORE_VERSION: u32 = 3;
/// O aviso de troca antecede o corte por esta fração (mínimo 80 RPM).
const SHIFT_MARGIN_FRAC: f32 = 0.015;
/// Bins preenchidos mínimos para confiar na curva de potência.
const MIN_POWER_BINS: usize = 8;
/// Upshifts observados mínimos para confiar na razão de queda de RPM.
const MIN_DROPS: usize = 3;
/// Máximo de razões guardadas (janela deslizante).
const MAX_DROPS: usize = 12;
/// Janela para medir o RPM pós-troca (após o blackout).
const UPSHIFT_WINDOW: Duration = Duration::from_millis(1200);
/// Faixa sã de `rpm_depois / rpm_antes` num upshift.
const DROP_RANGE: std::ops::RangeInclusive<f32> = 0.45..=0.95;
/// Intervalo mínimo entre saves em disco.
const SAVE_EVERY: Duration = Duration::from_secs(3);

/// Calibração aprendida de um carro.
#[derive(Serialize, Deserialize, Clone)]
pub struct CarRevData {
    /// `engine_max_rpm` na época do aprendizado — mudou = motor trocado.
    pub max_rpm: f32,
    /// RPM onde o limitador corta (`None` = nunca observado).
    #[serde(default)]
    pub cut_rpm: Option<f32>,
    /// Potência máxima (cv) observada por bin de `rpm / max_rpm`; 0.0 = vazio.
    #[serde(default)]
    pub power: Vec<f32>,
    /// Razões `rpm_depois / rpm_antes` dos últimos upshifts a todo gás.
    #[serde(default)]
    pub drops: Vec<f32>,
}

impl CarRevData {
    fn new(max_rpm: f32) -> Self {
        Self {
            max_rpm,
            cut_rpm: None,
            power: vec![0.0; BINS],
            drops: Vec::new(),
        }
    }
}

/// Resposta consumida pelo display/render — frações de `max_rpm`.
#[derive(Clone, Copy, Debug)]
pub struct ShiftInfo {
    /// Ponto de troca recomendado.
    pub shift_frac: f32,
    /// Corte aprendido (1.0 enquanto desconhecido).
    pub limit_frac: f32,
    /// O corte já foi observado para este carro.
    pub limit_known: bool,
}

impl Default for ShiftInfo {
    fn default() -> Self {
        Self {
            shift_frac: DEFAULT_SHIFT_FRAC,
            limit_frac: 1.0,
            limit_known: false,
        }
    }
}

/// Upshift aguardando o RPM estabilizar na marcha nova para medir a queda.
#[derive(Clone, Copy)]
struct PendingUpshift {
    to_gear: u8,
    rpm_before: f32,
    at: Instant,
}

/// Estado transitório da detecção (zerado ao trocar de carro).
#[derive(Default)]
struct Transients {
    last_gear: Option<u8>,
    last_rpm: f32,
    /// Menor RPM visto na marcha atual — prova de que o giro subiu até o platô.
    gear_min_rpm: Option<f32>,
    gear_changed_at: Option<Instant>,
    pending_upshift: Option<PendingUpshift>,
    /// Maior marcha à frente vista nesta sessão — platô na marcha mais alta
    /// pode ser limite aerodinâmico, não o limitador.
    max_gear_seen: u8,
    plat_rpm: f32,
    plat_since: Option<Instant>,
}

impl Transients {
    fn reset_plateau(&mut self) {
        self.plat_rpm = 0.0;
        self.plat_since = None;
    }
}

#[derive(Serialize, Deserialize, Default)]
struct StoreFile {
    version: u32,
    cars: HashMap<String, CarRevData>,
}

pub struct RevLimitTracker {
    cars: HashMap<String, CarRevData>,
    path: Option<PathBuf>,
    dirty: bool,
    last_save: Instant,
    active: Option<String>,
    info: ShiftInfo,
    tr: Transients,
}

fn car_key(snap: &Telemetry) -> Option<String> {
    if snap.car_ordinal != 0 {
        return Some(format!("ord:{}", snap.car_ordinal));
    }
    if snap.max_rpm > 100.0 {
        let sig = (snap.max_rpm.to_bits() as u64) ^ ((snap.idle_rpm.to_bits() as u64) << 1);
        return Some(format!("sig:{sig:x}"));
    }
    None
}

/// Mediana das razões de queda de RPM (None sem amostras suficientes).
fn median_drop(drops: &[f32]) -> Option<f32> {
    if drops.len() < MIN_DROPS {
        return None;
    }
    let mut v = drops.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

/// Potência num bin, suavizada pelos vizinhos imediatos (bins esparsos).
fn bin_power(power: &[f32], i: usize) -> Option<f32> {
    let lo = i.saturating_sub(1);
    let hi = (i + 1).min(BINS - 1);
    let best = power[lo..=hi].iter().cloned().fold(0.0f32, f32::max);
    (best > 0.0).then_some(best)
}

/// Ponto de troca a partir do aprendido: logo antes do corte; se a curva de
/// potência e a queda de RPM das trocas forem conhecidas, antecipa para o
/// maior RPM em que a marcha atual ainda rende mais que a próxima renderia.
fn compute_info(e: &CarRevData) -> ShiftInfo {
    if e.max_rpm <= 100.0 {
        return ShiftInfo::default();
    }
    let limit_known = e.cut_rpm.is_some();
    let limit = e.cut_rpm.unwrap_or(e.max_rpm).min(e.max_rpm * 1.02);
    let mut shift = if limit_known {
        limit - (limit * SHIFT_MARGIN_FRAC).max(80.0)
    } else {
        e.max_rpm * DEFAULT_SHIFT_FRAC
    };

    let filled = e.power.iter().filter(|&&p| p > 0.0).count();
    if let Some(drop) = median_drop(&e.drops).filter(|_| filled >= MIN_POWER_BINS) {
        let (mut peak_i, mut peak_p) = (0usize, 0.0f32);
        for (i, &p) in e.power.iter().enumerate() {
            if p > peak_p {
                peak_p = p;
                peak_i = i;
            }
        }
        let top_bin = (((shift / e.max_rpm) * BINS as f32) as usize).min(BINS - 1);
        if peak_p > 0.0 && peak_i < top_bin {
            // P(rpm) ≥ P(rpm·drop): segurar a marcha ainda vale a pena.
            let mut best: Option<usize> = None;
            for b in peak_i..=top_bin {
                let after_bin = (((b as f32 + 0.5) * drop) as usize).min(BINS - 1);
                let (Some(cur), Some(after)) =
                    (bin_power(&e.power, b), bin_power(&e.power, after_bin))
                else {
                    continue;
                };
                if cur >= after {
                    best = Some(b);
                }
            }
            if let Some(b) = best {
                let rpm = (b as f32 + 0.5) / BINS as f32 * e.max_rpm;
                shift = shift.min(rpm);
            }
        }
    }

    let shift_frac = (shift / e.max_rpm).clamp(0.30, 1.0);
    let limit_frac = if limit_known {
        (limit / e.max_rpm).clamp(shift_frac, 1.0)
    } else {
        1.0
    };
    ShiftInfo {
        shift_frac,
        limit_frac,
        limit_known,
    }
}

/// Um passo de aprendizado sobre a entrada do carro ativo. Retorna `true` se
/// algo novo foi aprendido (marca o store como dirty).
fn learn(e: &mut CarRevData, tr: &mut Transients, snap: &Telemetry, now: Instant) -> bool {
    let mut dirty = false;
    let throttle = snap.accel.unwrap_or(0.0);

    if snap.gear != tr.last_gear {
        // Upshift a todo gás: agenda a medição da queda de RPM (rpm da marcha
        // nova só vale depois do blackout, com o motor já engatado).
        tr.pending_upshift = match (tr.last_gear, snap.gear) {
            (Some(prev), Some(new))
                if new == prev + 1
                    && (1..=9).contains(&prev)
                    && throttle >= FULL_THROTTLE
                    && tr.last_rpm > 0.0 =>
            {
                Some(PendingUpshift {
                    to_gear: new,
                    rpm_before: tr.last_rpm,
                    at: now,
                })
            }
            _ => None,
        };
        tr.last_gear = snap.gear;
        tr.gear_changed_at = Some(now);
        tr.gear_min_rpm = None;
        tr.reset_plateau();
    }
    // A rampa da embreagem no blackout varre o RPM da marcha antiga para a
    // nova — esse mínimo não é evidência de que o motor subiu livre até um
    // platô, então o rastreio só começa depois do blackout.
    let in_blackout = tr
        .gear_changed_at
        .is_some_and(|t| now.duration_since(t) < POST_SHIFT_BLACKOUT);
    if snap.rpm.is_finite() {
        tr.last_rpm = snap.rpm;
        if snap.rpm > 0.0 && !in_blackout {
            tr.gear_min_rpm = Some(tr.gear_min_rpm.map_or(snap.rpm, |m| m.min(snap.rpm)));
        }
    }
    let gear = snap.gear.unwrap_or(0);
    if (1..=10).contains(&gear) && gear > tr.max_gear_seen {
        tr.max_gear_seen = gear;
    }

    if let Some(p) = tr.pending_upshift {
        let elapsed = now.duration_since(p.at);
        if snap.gear != Some(p.to_gear) || elapsed > UPSHIFT_WINDOW {
            tr.pending_upshift = None;
        } else if elapsed >= POST_SHIFT_BLACKOUT
            && throttle >= FULL_THROTTLE
            && snap.rpm.is_finite()
            && snap.rpm > 0.0
        {
            let ratio = snap.rpm / p.rpm_before;
            if DROP_RANGE.contains(&ratio) {
                e.drops.push(ratio);
                if e.drops.len() > MAX_DROPS {
                    e.drops.remove(0);
                }
                dirty = true;
            }
            tr.pending_upshift = None;
        }
    }

    let sampling = e.max_rpm > 100.0
        && throttle >= FULL_THROTTLE
        && (1..=10).contains(&gear)
        && !in_blackout
        && snap.rpm.is_finite()
        && snap.rpm > snap.idle_rpm.max(0.0) + 100.0;
    if !sampling {
        tr.reset_plateau();
        return false;
    }

    // Curva de potência: máxima observada por faixa de RPM.
    if let Some(p) = snap.power_hp {
        if p.is_finite() && p > 0.0 {
            let bin = ((snap.rpm / e.max_rpm) * BINS as f32) as usize;
            let bin = bin.min(BINS - 1);
            if p > e.power[bin] {
                e.power[bin] = p;
                dirty = true;
            }
        }
    }

    // Platô: RPM preso numa banda estreita com acelerador cravado = corte.
    // Roda patinando não vale — ali o RPM é regido pela tração, não pelo corte.
    if snap.slip.iter().any(|s| s.abs() > SLIP_TRUST_MAX) {
        tr.reset_plateau();
        return dirty;
    }
    let band = e.max_rpm * PLATEAU_BAND_FRAC;
    match tr.plat_since {
        Some(since) if (snap.rpm - tr.plat_rpm).abs() <= band => {
            let held = now.duration_since(since);
            if held >= PLATEAU_HOLD {
                let frac = tr.plat_rpm / e.max_rpm;
                let trusted = frac >= CUT_TRUST_FRAC || gear < tr.max_gear_seen;
                // O giro subiu até aqui nesta marcha? Reduzida forte entra por
                // cima do corte (flat/caindo) e não pode virar limitador.
                let climbed = tr
                    .gear_min_rpm
                    .is_some_and(|m| tr.plat_rpm - m >= e.max_rpm * CLIMB_MIN_FRAC);
                if frac >= CUT_MIN_FRAC && trusted && climbed {
                    let cut = tr.plat_rpm.min(e.max_rpm * 1.02);
                    // Perto do corte salvo: refina p/ cima (topo do bounce).
                    // Longe dele: substitui — auto-corrige aprendizado ruim —
                    // mas só com platô longo; transiente não derruba o salvo.
                    let new_cut = match e.cut_rpm {
                        Some(old) if (old - cut).abs() <= band * 2.0 => Some(old.max(cut)),
                        Some(_) if held < PLATEAU_HOLD_REPLACE => None,
                        _ => Some(cut),
                    };
                    if let Some(nc) = new_cut {
                        if e.cut_rpm != Some(nc) {
                            e.cut_rpm = Some(nc);
                            dirty = true;
                        }
                    }
                }
            }
        }
        _ => {
            tr.plat_rpm = snap.rpm;
            tr.plat_since = Some(now);
        }
    }

    dirty
}

impl RevLimitTracker {
    /// Tracker em memória (sem persistência) — testes e fallback.
    pub fn new() -> Self {
        Self {
            cars: HashMap::new(),
            path: None,
            dirty: false,
            last_save: Instant::now(),
            active: None,
            info: ShiftInfo::default(),
            tr: Transients::default(),
        }
    }

    /// Carrega o aprendizado salvo e persiste as novidades em `path`.
    pub fn with_persistence(path: PathBuf) -> Self {
        let mut t = Self::new();
        t.cars = load_store(&path);
        t.path = Some(path);
        t
    }

    pub fn default_path() -> PathBuf {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base)
            .join("forza-hud-native")
            .join("rev-limits.json")
    }

    /// Ponto de troca / corte do carro ativo (default enquanto não há dados).
    pub fn info(&self) -> ShiftInfo {
        self.info
    }

    pub fn update(&mut self, snap: &Telemetry, active: bool) {
        self.update_at(snap, active, Instant::now());
    }

    pub fn update_at(&mut self, snap: &Telemetry, active: bool, now: Instant) {
        let Some(key) = car_key(snap) else {
            self.tr.reset_plateau();
            return;
        };
        if self.active.as_deref() != Some(key.as_str()) {
            self.active = Some(key.clone());
            self.tr = Transients::default();
        }

        let entry = self
            .cars
            .entry(key)
            .or_insert_with(|| CarRevData::new(snap.max_rpm));
        // Upgrade/swap de motor: fundo de escala mudou → recalibra do zero.
        if snap.max_rpm > 100.0 && (entry.max_rpm - snap.max_rpm).abs() > 1.0 {
            *entry = CarRevData::new(snap.max_rpm);
            self.dirty = true;
        }

        if active {
            self.dirty |= learn(entry, &mut self.tr, snap, now);
        } else {
            self.tr.reset_plateau();
        }
        self.info = compute_info(entry);
        self.maybe_save(now);
    }

    fn maybe_save(&mut self, now: Instant) {
        if self.dirty && now.duration_since(self.last_save) >= SAVE_EVERY {
            self.save_now();
        }
    }

    /// Grava imediatamente se houver algo novo (chamar ao sair do app).
    pub fn save_now(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.as_deref() else {
            self.dirty = false;
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = StoreFile {
            version: STORE_VERSION,
            cars: self.cars.clone(),
        };
        match serde_json::to_string(&file) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    eprintln!("[revlimit] falha ao salvar {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[revlimit] serialize: {e}"),
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }
}

impl Default for RevLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn load_store(path: &Path) -> HashMap<String, CarRevData> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    match serde_json::from_str::<StoreFile>(&text) {
        Ok(f) if f.version < STORE_VERSION => {
            eprintln!(
                "[revlimit] {} v{} < v{STORE_VERSION} — descartado (reaprende)",
                path.display(),
                f.version
            );
            HashMap::new()
        }
        Ok(mut f) => {
            for e in f.cars.values_mut() {
                e.power.resize(BINS, 0.0);
            }
            eprintln!("[revlimit] {} carros ← {}", f.cars.len(), path.display());
            f.cars
        }
        Err(e) => {
            eprintln!("[revlimit] {}: {e}", path.display());
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(rpm: f32, gear: u8, accel: f32) -> Telemetry {
        Telemetry {
            race: 1,
            rpm,
            max_rpm: 8000.0,
            idle_rpm: 800.0,
            car_ordinal: 42,
            gear: Some(gear),
            accel: Some(accel),
            power_hp: Some(300.0),
            ..Default::default()
        }
    }

    /// Alimenta amostras a cada 50 ms a partir de `t0 + start_ms`.
    fn feed(tr: &mut RevLimitTracker, t0: Instant, start_ms: u64, samples: &[Telemetry]) {
        for (i, s) in samples.iter().enumerate() {
            let t = t0 + Duration::from_millis(start_ms + i as u64 * 50);
            tr.update_at(s, true, t);
        }
    }

    /// Sobe de `from` até `plateau` (passos de 250, sem platô acidental) e
    /// segura o platô por `hold` amostras — o caminho real até o limitador.
    fn climb_then_hold(gear: u8, from: f32, plateau: f32, hold: usize) -> Vec<Telemetry> {
        let mut v = Vec::new();
        let mut rpm = from;
        while rpm < plateau {
            v.push(snap(rpm, gear, 100.0));
            rpm += 250.0;
        }
        v.extend((0..hold).map(|_| snap(plateau, gear, 100.0)));
        v
    }

    #[test]
    fn plato_a_todo_gas_vira_corte() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Sobe até bater no limitador em 7800 (97.5% de 8000 ≥ CUT_TRUST_FRAC).
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        let info = tr.info();
        assert!(info.limit_known, "corte deveria ter sido aprendido");
        assert!((info.limit_frac - 7800.0 / 8000.0).abs() < 0.015);
        assert!(info.shift_frac < info.limit_frac);
    }

    #[test]
    fn reduzida_forte_acima_do_corte_nao_vira_limite() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Acelerando em 3ª...
        let up: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 200.0, 3, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &up);
        // ...reduzida forte p/ 2ª com o pé no chão: a roda segura o motor em
        // 8100 por mais de 1 s (muito além do blackout). O giro entrou por
        // cima — nunca subiu até esse platô — então não é limitador.
        let over: Vec<Telemetry> = (0..16).map(|_| snap(8100.0, 2, 100.0)).collect();
        feed(&mut tr, t0, 500, &over);
        assert!(!tr.info().limit_known, "overrev de reduzida não é corte");
    }

    #[test]
    fn corte_errado_salvo_se_corrige_para_baixo() {
        let mut tr = RevLimitTracker::new();
        tr.cars.insert(
            "ord:42".into(),
            CarRevData {
                max_rpm: 8000.0,
                cut_rpm: Some(8160.0), // aprendizado ruim (ex.: overrev antigo)
                power: vec![0.0; BINS],
                drops: Vec::new(),
            },
        );
        let t0 = Instant::now();
        // Substituição exige platô longo (PLATEAU_HOLD_REPLACE) — segura 1 s.
        let samples = climb_then_hold(3, 5000.0, 7800.0, 20);
        feed(&mut tr, t0, 0, &samples);
        let cut = tr.cars.get("ord:42").unwrap().cut_rpm.unwrap();
        assert!(
            cut > 7650.0 && cut < 7900.0,
            "corte deveria cair p/ ~7800, ficou {cut}"
        );
    }

    #[test]
    fn rampa_de_reduzida_no_blackout_nao_conta_como_subida() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Acelerando em 4ª sem tirar o pé...
        let up: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 150.0, 4, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &up);
        // ...reduz 2 marchas: a embreagem varre o RPM p/ cima DENTRO do
        // blackout e a roda prende o motor acima do corte por >1 s. A rampa
        // não é subida do motor livre — não pode validar o platô.
        let mut down: Vec<Telemetry> = [6600.0, 7200.0, 7800.0, 8100.0]
            .iter()
            .map(|&r| snap(r, 2, 100.0))
            .collect();
        down.extend((0..20).map(|_| snap(8150.0, 2, 100.0)));
        feed(&mut tr, t0, 500, &down);
        assert!(
            !tr.info().limit_known,
            "overrev de reduzida com rampa não é limitador"
        );
    }

    #[test]
    fn plato_patinando_nao_substitui_corte_bom() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Aprende o corte real (7800) numa puxada limpa em 3ª.
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        assert!(tr.info().limit_known);
        // Reduz p/ 2ª e o TCS segura o giro em 7000 (87,5%) patinando: sobe
        // DEPOIS do blackout (8 amostras = 400 ms em 6000) e fica flat por
        // >1 s — sem o gate de slip isso substituiria o corte bom por 7000.
        let spin: Vec<Telemetry> = std::iter::repeat(6000.0)
            .take(8)
            .chain([6500.0, 6800.0])
            .chain(std::iter::repeat(7000.0).take(22))
            .map(|r| {
                let mut s = snap(r, 2, 100.0);
                s.slip = [1.8, 1.8, 2.4, 2.4];
                s
            })
            .collect();
        feed(&mut tr, t0, 2000, &spin);
        let cut = tr.cars.get("ord:42").unwrap().cut_rpm.unwrap();
        assert!(
            (cut - 7800.0).abs() < 100.0,
            "patinada não deveria mexer no corte, ficou {cut}"
        );
    }

    #[test]
    fn overshoot_de_downshift_e_ignorado() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Estabiliza em 3ª bem abaixo do corte (varia RPM p/ não formar platô).
        let cruise: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 150.0, 3, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &cruise);
        // Reduz p/ 2ª: RPM estoura acima do corte por ~300 ms (blackout cobre).
        let over: Vec<Telemetry> = (0..6).map(|_| snap(8100.0, 2, 100.0)).collect();
        feed(&mut tr, t0, 500, &over);
        assert!(!tr.info().limit_known, "overshoot não é limitador");
    }

    #[test]
    fn plato_aero_na_marcha_mais_alta_nao_vale() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Só andou em 5ª (marcha mais alta vista) subindo até 6800 (85% < 93%):
        // pode ser limite aerodinâmico, não o corte.
        let samples = climb_then_hold(5, 5000.0, 6800.0, 12);
        feed(&mut tr, t0, 0, &samples);
        assert!(!tr.info().limit_known);
    }

    #[test]
    fn plato_em_marcha_baixa_vale_mesmo_abaixo_da_confianca() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Passou pela 4ª antes (max_gear_seen = 4)...
        let shift_up: Vec<Telemetry> = (0..3).map(|i| snap(4000.0, 4 - i as u8, 30.0)).collect();
        feed(&mut tr, t0, 0, &shift_up);
        // ...então subir e travar em 2ª a 6900 (86%) é limitador de verdade.
        let samples = climb_then_hold(2, 5000.0, 6900.0, 12);
        feed(&mut tr, t0, 1000, &samples);
        let info = tr.info();
        assert!(info.limit_known);
        assert!((info.limit_frac - 6900.0 / 8000.0).abs() < 0.015);
    }

    #[test]
    fn upshift_a_todo_gas_mede_queda_de_rpm() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        // Sobe em 2ª de 5000 a 7000 (10 amostras) e engata 3ª caindo p/ 4900.
        let up: Vec<Telemetry> = (0..10)
            .map(|i| snap(5000.0 + i as f32 * 222.0, 2, 100.0))
            .collect();
        feed(&mut tr, t0, 0, &up);
        let third: Vec<Telemetry> = (0..12).map(|_| snap(4900.0, 3, 100.0)).collect();
        feed(&mut tr, t0, 500, &third);
        let e = tr.cars.get("ord:42").unwrap();
        assert_eq!(e.drops.len(), 1);
        assert!((e.drops[0] - 4900.0 / 6998.0).abs() < 0.02);
    }

    #[test]
    fn curva_de_potencia_com_queda_antecipar_troca() {
        // Parábola com pico em ~62,5% do giro: com razão de queda 0.7, o ponto
        // ótimo (P(rpm) = P(0.7·rpm)) fica em ~70% — bem antes do corte.
        let mut e = CarRevData::new(8000.0);
        e.cut_rpm = Some(7800.0);
        e.drops = vec![0.7, 0.7, 0.7];
        for i in 8..=62 {
            let f = (i as f32 + 0.5) / 64.0;
            let d = (f - 0.625) / 0.4;
            e.power[i] = (1000.0 * (1.0 - d * d)).max(1.0);
        }
        let info = compute_info(&e);
        assert!(info.limit_known);
        assert!(
            info.shift_frac > 0.64 && info.shift_frac < 0.78,
            "shift_frac = {}",
            info.shift_frac
        );
    }

    #[test]
    fn potencia_crescente_ate_o_corte_troca_no_corte() {
        // Potência só sobe até o limitador → troca logo antes do corte.
        let mut e = CarRevData::new(8000.0);
        e.cut_rpm = Some(7800.0);
        e.drops = vec![0.7, 0.7, 0.7];
        for i in 8..=62 {
            e.power[i] = 100.0 + i as f32 * 10.0;
        }
        let info = compute_info(&e);
        let cut_margin = (7800.0 - 117.0) / 8000.0;
        assert!(
            (info.shift_frac - cut_margin).abs() < 0.02,
            "shift_frac = {}",
            info.shift_frac
        );
    }

    #[test]
    fn sem_razao_de_queda_ignora_curva_de_potencia() {
        // Curva com pico cedo mas nenhuma troca observada: sem saber onde a
        // próxima marcha cai, não dá para antecipar — fica no corte.
        let mut e = CarRevData::new(8000.0);
        e.cut_rpm = Some(7800.0);
        for i in 8..=62 {
            let f = (i as f32 + 0.5) / 64.0;
            let d = (f - 0.5) / 0.35;
            e.power[i] = (900.0 * (1.0 - d * d)).max(1.0);
        }
        let info = compute_info(&e);
        let cut_margin = (7800.0 - 117.0) / 8000.0;
        assert!((info.shift_frac - cut_margin).abs() < 0.001);
    }

    #[test]
    fn sem_dados_usa_default() {
        let mut tr = RevLimitTracker::new();
        tr.update_at(&snap(3000.0, 2, 0.0), true, Instant::now());
        let info = tr.info();
        assert!(!info.limit_known);
        assert!((info.shift_frac - DEFAULT_SHIFT_FRAC).abs() < 1e-6);
        assert_eq!(info.limit_frac, 1.0);
    }

    #[test]
    fn upgrade_de_motor_invalida_aprendizado() {
        let mut tr = RevLimitTracker::new();
        let t0 = Instant::now();
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        assert!(tr.info().limit_known);
        // Mesmo carro, novo motor: max_rpm 9000 → recalibra.
        let mut s2 = snap(4000.0, 2, 0.0);
        s2.max_rpm = 9000.0;
        tr.update_at(&s2, true, t0 + Duration::from_secs(5));
        assert!(!tr.info().limit_known);
    }

    #[test]
    fn persiste_e_recarrega() {
        let dir = std::env::temp_dir().join(format!("revlimit-test-{}", std::process::id()));
        let path = dir.join("rev-limits.json");
        let _ = std::fs::remove_file(&path);

        let mut tr = RevLimitTracker::with_persistence(path.clone());
        let t0 = Instant::now();
        let samples = climb_then_hold(3, 5000.0, 7800.0, 10);
        feed(&mut tr, t0, 0, &samples);
        assert!(tr.info().limit_known);
        tr.save_now();

        let mut tr2 = RevLimitTracker::with_persistence(path.clone());
        tr2.update_at(&snap(1000.0, 1, 0.0), false, Instant::now());
        let info = tr2.info();
        assert!(info.limit_known, "corte deveria voltar do disco");
        assert!((info.limit_frac - 7800.0 / 8000.0).abs() < 0.015);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
