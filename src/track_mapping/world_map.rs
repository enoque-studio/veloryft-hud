//! Gravador do mapa de elevação do mundo — agrega a altura `y` da posição
//! `[x,y,z]` da telemetria por célula de grade enquanto o jogador dirige.
//! Espelha a validação de posição/teleporte do [`crate::telemetry::elevation::ElevationTracker`].
//!
//! Persistência: o estado em memória é semeado do arquivo no boot (`load_seed`) e
//! sobrescreve o arquivo nos flushes (`save`). Como a memória só cresce a partir do
//! que foi carregado, sobrescrever é idempotente e funde sessões — desde que apenas
//! um processo escreva o arquivo por vez (o overlay é o dono único).

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use veloryft_protocol::{HeightCell, WorldHeightmap, SURFACE_KINDS};

use crate::telemetry::model::Telemetry;

/// Classes de superfície — índices no histograma `HeightCell::surf` (alinhado com
/// `veloryft_protocol::SURFACE_KINDS`). O Forza não rotula a superfície no UDP; isto é
/// inferido de `surf_rumble`/`puddle` e é uma heurística calibrável.
pub mod surface {
    pub const UNKNOWN: usize = 0;
    pub const ASPHALT: usize = 1; // via pavimentada
    pub const DIRT: usize = 2; // terra / trilha de chão batido
    pub const GRASS: usize = 3; // grama / offroad acidentado
    pub const WATER: usize = 4; // vau / poça funda
}

/// Profundidade média de poça (0..~1) acima da qual a amostra vira Água. Tem
/// prioridade sobre a vibração (atravessar água molha qualquer superfície).
const PUDDLE_WET: f32 = 0.10;
/// Vibração média de superfície (0..~1) abaixo da qual é Asfalto (liso).
const RUMBLE_PAVED: f32 = 0.05;
/// Vibração abaixo da qual é Terra/Trilha; acima vira Grama/Offroad acidentado.
const RUMBLE_DIRT: f32 = 0.30;
const MAX_VALID_TEMP: f32 = 900.0;

#[inline]
fn quad_avg(q: [f32; 4]) -> f32 {
    (q[0] + q[1] + q[2] + q[3]) * 0.25
}

/// Classifica a superfície de uma amostra a partir dos sinais do pacote.
/// `UNKNOWN` quando não há sinal (ex.: pacote Sled, sem dados de roda).
fn classify_surface(t: &Telemetry) -> usize {
    let Some(rumble) = t.surf_rumble else {
        return surface::UNKNOWN;
    };
    if let Some(p) = t.puddle {
        if quad_avg(p) > PUDDLE_WET {
            return surface::WATER;
        }
    }
    let r = quad_avg(rumble);
    if !r.is_finite() {
        return surface::UNKNOWN;
    }
    if r < RUMBLE_PAVED {
        surface::ASPHALT
    } else if r < RUMBLE_DIRT {
        surface::DIRT
    } else {
        surface::GRASS
    }
}

fn tire_temp_mean(tires: Option<[f32; 4]>) -> f32 {
    let Some(temps) = tires else {
        return 0.0;
    };
    let mut sum = 0.0;
    let mut n = 0.0;
    for temp in temps {
        if temp.is_finite() && temp > 0.0 && temp <= MAX_VALID_TEMP {
            sum += temp;
            n += 1.0;
        }
    }
    if n == 0.0 {
        0.0
    } else {
        sum / n
    }
}

/// Lado da célula (m) padrão. Fino o bastante para a pista ter largura/detalhe
/// (mapeamento "só da pista", tipo um Google Maps das ruas). Ajustável por env.
pub const DEFAULT_CELL_M: f32 = 0.5;
/// Salto acima disso = teleporte (fast travel / respawn): descarta a amostra.
const MAX_JUMP: f32 = 40.0;
/// Movimento mínimo entre amostras (m): evita inflar `n` com o carro parado/idle.
/// Bem baixo (5 cm) para capturar o traçado quase no detalhe milimétrico.
const MIN_MOVE_M: f32 = 0.05;
/// Lacuna (m) desde o último ponto gravado acima da qual a fita quebra num novo
/// segmento. Acontece quando se atravessa território já mapeado (células puladas)
/// ou num micro-salto abaixo do teleporte — evita ligar pontos distantes com uma
/// reta falsa. Abaixo disso, segue no mesmo segmento.
const PATH_BREAK_GAP_M: f32 = 22.0;
/// Identificador do mundo — FH6 é mundo aberto único por enquanto.
pub const WORLD_ID: &str = "fh6";
/// Intervalo do autosave em disco.
const AUTOSAVE_SECS: u64 = 5;
/// A cada N ticks de autosave (~60 s), grava snapshot JSON completo no disco.
const COMPACT_EVERY_TICKS: u64 = 12;

/// Lado da célula vindo de `VELORYFT_WORLDMAP_CELL_M`, ou o padrão.
fn cell_size_from_env() -> f32 {
    std::env::var("VELORYFT_WORLDMAP_CELL_M")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_CELL_M)
}

fn client_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Acumulador de uma célula. Somas em f64 para precisão sobre muitas amostras.
#[derive(Clone, Debug)]
struct CellAcc {
    sum_y: f64,
    min_y: f32,
    max_y: f32,
    sum_spd: f64,
    sum_temp: f64,
    n: u32,
    /// Histograma de classes de superfície (ver [`surface`]).
    surf: [u32; SURFACE_KINDS],
}

impl Default for CellAcc {
    fn default() -> Self {
        Self {
            sum_y: 0.0,
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
            sum_spd: 0.0,
            sum_temp: 0.0,
            n: 0,
            surf: [0; SURFACE_KINDS],
        }
    }
}

impl CellAcc {
    fn observe(&mut self, y: f32, kind: usize, speed: f32, temp: f32) {
        self.sum_y += y as f64;
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
        self.sum_spd += speed as f64;
        self.sum_temp += temp as f64;
        self.n += 1;
        if let Some(slot) = self.surf.get_mut(kind) {
            *slot += 1;
        }
    }

    fn mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_y / self.n as f64) as f32
        }
    }

    fn spd_mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_spd / self.n as f64) as f32
        }
    }

    fn temp_mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_temp / self.n as f64) as f32
        }
    }
}

/// Estatísticas leves para a UI/comando.
#[derive(Serialize, Clone, Default, Debug)]
pub struct WorldMapStats {
    pub world_id: String,
    pub cells: u32,
    pub samples: u64,
    pub cols: u32,
    pub rows: u32,
}

/// Um ponto do traçado, na ordem em que foi dirigido.
#[derive(Clone, Copy, Debug)]
struct PathPoint {
    x: f32,
    y: f32,
    z: f32,
    spd: f32,
    temp: f32,
    surf: u8,
}

/// Traçado ordenado servido ao viewer para a fita de largura fixa. Cada ponto é
/// `[x, y, z, spd_kmh, temp_c, surf]`; os segmentos quebram em teleporte/parada.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct PathDto {
    pub world_id: String,
    pub points: usize,
    pub segments: Vec<Vec<[f32; 6]>>,
}

/// Um segmento com pontos novos/mudados no delta. `from` é o índice a partir do
/// qual `pts` começa (o viewer anexa); `from == 0` significa "substitua o segmento".
#[derive(Serialize, Clone, Debug)]
pub struct PathDeltaSeg {
    pub id: u64,
    pub from: usize,
    pub pts: Vec<[f32; 6]>,
}

/// Resposta incremental de `/path/delta`. `order` lista todos os ids atuais na
/// ordem do traçado (o viewer descarta os que sumiram e reordena); `changed` traz
/// só os segmentos com pontos novos. `full == true` quando o epoch divergiu
/// (reset/restart): o viewer limpa o cache antes de aplicar.
#[derive(Serialize, Clone, Debug)]
pub struct PathDeltaDto {
    pub world_id: String,
    pub epoch: u64,
    pub points: usize,
    pub loops_locked: u32,
    pub full: bool,
    pub order: Vec<u64>,
    pub changed: Vec<PathDeltaSeg>,
}

/// Delta de persistência incremental drenado a cada flush do autosave. `reset`
/// pede para truncar os logs (epoch divergiu); `cells`/`path` são o que anexar.
pub struct PersistDelta {
    pub reset: bool,
    pub cells: Vec<HeightCell>,
    /// Pontos novos do traçado: `(seg_id, [x, y, z, spd, temp, surf])`.
    pub path: Vec<(u64, [f32; 6])>,
}

pub struct WorldMapRecorder {
    cell_m: f32,
    cells: HashMap<(i32, i32), CellAcc>,
    last_xz: Option<(f32, f32)>,
    last_pos: Option<[f32; 3]>,
    recording: bool,
    dirty: bool,
    /// Traçado ordenado (segmentos) para a fita; independente do grid de células.
    path: Vec<Vec<PathPoint>>,
    /// Id estável por segmento (paralelo a `path`) + epoch (bump no reset). Usados
    /// pela API incremental `/path/delta`: o viewer só baixa pontos novos/mudados.
    seg_ids: Vec<u64>,
    next_seg_id: u64,
    epoch: u64,
    seg_open: bool,
    path_len: usize,
    last_path_xz: Option<(f32, f32)>,
    /// Células (na resolução de `cell_m`) já cobertas pela fita — dedupe espacial:
    /// revisitar a mesma célula não empilha um novo ponto. Reconstruído do disco no
    /// boot (independente do heightmap), para a fita se recuperar sozinha.
    path_cells: HashSet<(i32, i32)>,
    /// Cursores da persistência incremental (append-only). `pending_cells`: células
    /// novas ainda não anexadas ao log; `seg_saved[i]`: nº de pontos do segmento `i`
    /// já gravados no log; `persist_epoch`: epoch já refletido em disco (divergiu =
    /// reset → o autosave trunca os logs).
    pending_cells: Vec<(i32, i32)>,
    seg_saved: Vec<usize>,
    persist_epoch: u64,
}

impl Default for WorldMapRecorder {
    fn default() -> Self {
        Self::with_cell_size(cell_size_from_env())
    }
}

impl WorldMapRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cria com um lado de célula explícito (em metros).
    pub fn with_cell_size(cell_m: f32) -> Self {
        Self {
            cell_m,
            cells: HashMap::new(),
            last_xz: None,
            last_pos: None,
            recording: false,
            dirty: false,
            path: Vec::new(),
            seg_ids: Vec::new(),
            next_seg_id: 0,
            epoch: 0,
            seg_open: false,
            path_len: 0,
            last_path_xz: None,
            path_cells: HashSet::new(),
            pending_cells: Vec::new(),
            seg_saved: Vec::new(),
            persist_epoch: 0,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn set_recording(&mut self, on: bool) {
        self.recording = on;
        self.last_xz = None;
        self.break_segment(); // alternar gravação inicia um novo segmento do traçado
    }

    pub fn last_pos(&self) -> Option<[f32; 3]> {
        self.last_pos
    }

    pub fn cell_size_m(&self) -> f32 {
        self.cell_m
    }

    /// Mantido por compatibilidade com a live API. No modelo "mapear o mundo todo"
    /// não há mais travamento de voltas, então é sempre 0.
    pub fn loops_locked(&self) -> u32 {
        0
    }

    /// Antes "fixava" o traçado (travava regravação). Sem o modelo de dedupe por
    /// corredor isso não existe mais — agora só encerra o segmento aberto, para que
    /// o que vier depois comece numa linha nova.
    pub fn fix_map(&mut self) {
        self.break_segment();
        self.dirty = true;
    }

    #[inline]
    fn cell_index(&self, v: f32) -> i32 {
        (v / self.cell_m).floor() as i32
    }

    /// Integra um frame de telemetria quando `active` (dirigindo).
    /// Só grava células com `recording == true`; `last_pos` atualiza sempre que há posição.
    pub fn ingest(&mut self, t: &Telemetry, active: bool) {
        if !active || t.sled_only != 0 {
            self.last_xz = None;
            self.last_pos = None;
            self.break_segment();
            return;
        }
        let Some([x, y, z]) = t.pos else {
            self.last_pos = None;
            return;
        };
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return;
        }

        self.last_pos = Some([x, y, z]);

        if !self.recording {
            return;
        }

        if let Some((lx, lz)) = self.last_xz {
            let jump = ((x - lx).powi(2) + (z - lz).powi(2)).sqrt();
            if jump > MAX_JUMP {
                self.last_xz = Some((x, z)); // reancora sem gravar o salto
                self.break_segment();
                return;
            }
            if jump < MIN_MOVE_M {
                return; // movimento ínfimo: não reamostra a mesma célula
            }
        }

        let kind = classify_surface(t);
        let speed = t.speed.unwrap_or(0.0);
        let temp = tire_temp_mean(t.tires);
        let cell_key = (self.cell_index(x), self.cell_index(z));
        // Cobertura (heightmap): grava cada célula uma única vez (não remarca).
        if !self.cells.contains_key(&cell_key) {
            self.cells
                .entry(cell_key)
                .or_default()
                .observe(y, kind, speed, temp);
            self.pending_cells.push(cell_key); // fila da persistência incremental
        }
        self.last_xz = Some((x, z));

        // Fita (traçado ordenado): um ponto por célula recém-coberta, na ordem
        // dirigida. Sem dedupe por corredor nem trava — mapeia o mundo todo; só
        // revisitar a MESMA célula é ignorado, pra não empilhar geometria.
        if !self.path_cells.contains(&cell_key) {
            // Lacuna grande desde o último ponto (pulamos território já coberto, ou
            // micro-salto sub-teleporte) → começa um segmento novo, sem reta falsa.
            if let Some((px, pz)) = self.last_path_xz {
                if ((x - px).powi(2) + (z - pz).powi(2)).sqrt() > PATH_BREAK_GAP_M {
                    self.break_segment();
                }
            }
            self.record_path_point(PathPoint {
                x,
                y,
                z,
                spd: speed,
                temp,
                surf: kind as u8,
            });
            self.path_cells.insert(cell_key);
        }

        self.dirty = true;
    }

    /// Funde um heightmap (ex.: carregado do disco) no estado atual.
    pub fn merge_from(&mut self, hm: &WorldHeightmap) {
        for c in &hm.cells {
            self.merge_cell(c);
        }
        self.dirty = true;
    }

    fn record_path_point(&mut self, p: PathPoint) {
        self.last_path_xz = Some((p.x, p.z));
        self.push_path(p);
    }

    fn break_segment(&mut self) {
        self.seg_open = false;
        self.last_path_xz = None;
    }

    fn push_path(&mut self, p: PathPoint) {
        if !self.seg_open {
            self.path.push(Vec::new());
            self.seg_ids.push(self.next_seg_id);
            self.seg_saved.push(0);
            self.next_seg_id = self.next_seg_id.wrapping_add(1);
            self.seg_open = true;
        }
        if let Some(seg) = self.path.last_mut() {
            seg.push(p);
            self.path_len += 1;
        }
        // Sem teto: "mapear o mundo todo". O crescimento é limitado pela cobertura
        // do mundo (dedupe por célula em `path_cells`), não pelo tempo de jogo.
    }

    /// Nº de pontos do traçado — a UI usa para saber quando rebuscar `/path`.
    pub fn path_points(&self) -> usize {
        self.path_len
    }

    /// Serializa o traçado ordenado (segmentos com ≥ 2 pontos) para o viewer.
    pub fn to_path(&self) -> PathDto {
        let segments = self
            .path
            .iter()
            .filter(|s| s.len() >= 2)
            .map(|s| {
                s.iter()
                    .map(|p| [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32])
                    .collect()
            })
            .collect();
        PathDto {
            world_id: WORLD_ID.to_string(),
            points: self.path_len,
            segments,
        }
    }

    /// Delta do traçado para o viewer: dado o `client_epoch` e o que ele já tem
    /// (`have`: id → nº de pontos), devolve só os pontos novos/mudados. Em mapas
    /// com kms gravados isso troca um payload de MBs por alguns KB por atualização.
    pub fn to_path_delta(&self, client_epoch: u64, have: &HashMap<u64, usize>) -> PathDeltaDto {
        let full = client_epoch != self.epoch;
        let mut order = Vec::with_capacity(self.path.len());
        let mut changed = Vec::new();

        for (seg, &id) in self.path.iter().zip(self.seg_ids.iter()) {
            order.push(id);
            let len = seg.len();
            let known = if full {
                0
            } else {
                have.get(&id).copied().unwrap_or(0)
            };
            if !full && known == len {
                continue; // segmento inalterado — só entra em `order`
            }
            // Encolheu (trim/fix) → reenvia inteiro; senão anexa a partir de `known`.
            let from = if known < len { known } else { 0 };
            let pts = seg[from..]
                .iter()
                .map(|p| [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32])
                .collect();
            changed.push(PathDeltaSeg { id, from, pts });
        }

        PathDeltaDto {
            world_id: WORLD_ID.to_string(),
            epoch: self.epoch,
            points: self.path_len,
            loops_locked: self.loops_locked(),
            full,
            order,
            changed,
        }
    }

    /// Converte o grid esparso no DTO de rede.
    pub fn to_heightmap(&self) -> WorldHeightmap {
        let mut cells = Vec::with_capacity(self.cells.len());
        let (mut min_cx, mut min_cz) = (i32::MAX, i32::MAX);
        let (mut max_cx, mut max_cz) = (i32::MIN, i32::MIN);

        for (&(cx, cz), acc) in &self.cells {
            if acc.n == 0 {
                continue;
            }
            min_cx = min_cx.min(cx);
            min_cz = min_cz.min(cz);
            max_cx = max_cx.max(cx);
            max_cz = max_cz.max(cz);
            cells.push(cell_to_height(cx, cz, acc));
        }

        let (origin, cols, rows) = if cells.is_empty() {
            ([0.0, 0.0], 0, 0)
        } else {
            (
                [min_cx as f32 * self.cell_m, min_cz as f32 * self.cell_m],
                (max_cx - min_cx + 1) as u32,
                (max_cz - min_cz + 1) as u32,
            )
        };

        WorldHeightmap {
            client_version: client_version(),
            world_id: WORLD_ID.to_string(),
            cell_size_m: self.cell_m,
            origin,
            cols,
            rows,
            cells,
        }
    }

    pub fn stats(&self) -> WorldMapStats {
        let hm = self.to_heightmap();
        let samples = hm.cells.iter().map(|c| c.n as u64).sum();
        WorldMapStats {
            world_id: hm.world_id,
            cells: hm.cells.len() as u32,
            samples,
            cols: hm.cols,
            rows: hm.rows,
        }
    }

    pub fn reset(&mut self) {
        self.cells.clear();
        self.last_xz = None;
        self.last_pos = None;
        self.path.clear();
        self.seg_ids.clear();
        // next_seg_id segue monotônico (não reusa ids); epoch++ força full no viewer.
        self.epoch = self.epoch.wrapping_add(1);
        self.seg_open = false;
        self.path_len = 0;
        self.last_path_xz = None;
        self.path_cells.clear();
        // Persistência: limpa a fila e os cursores. O epoch divergente faz o autosave
        // truncar os logs no próximo flush (ver `drain_persist_delta`).
        self.pending_cells.clear();
        self.seg_saved.clear();
        self.dirty = true;
    }

    /// Retorna `true` e zera a flag se houve mudança desde o último flush.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Semeia o estado do disco no boot: snapshot JSON (heightmap + traçado) MAIS o
    /// replay dos logs incrementais `.ndjson` (o que foi gravado após o último
    /// snapshot — recuperação de crash). Tudo que vem do disco já conta como
    /// persistido: zera fila/cursores para o autosave só anexar o que vier depois.
    pub fn load_seed(&mut self, path: &Path) {
        if let Some(hm) = read_heightmap(path) {
            // Adota o lado de célula do arquivo se houver dados — evita misturar
            // grades de resolução diferente caso o env tenha mudado entre sessões.
            if !hm.cells.is_empty() && hm.cell_size_m.is_finite() && hm.cell_size_m > 0.0 {
                self.cell_m = hm.cell_size_m;
            }
            self.merge_from(&hm);
        }
        for c in read_cells_log(&cells_log_path(path)) {
            self.merge_cell(&c);
        }

        // O traçado (fita) vive em arquivos companheiros (snapshot + log).
        self.load_path_seed(&companion_path(path));
        self.load_path_log(&path_log_path(path));

        self.pending_cells.clear();
        self.seg_saved = self.path.iter().map(|s| s.len()).collect();
        self.persist_epoch = self.epoch;
        self.dirty = false;
    }

    /// Restaura o traçado ordenado do snapshot e reconstrói a cobertura
    /// (`path_cells`) — dedupe por célula, sem trava: dirigir por cima não duplica.
    pub fn load_path_seed(&mut self, path: &Path) {
        let Some(dto) = read_path(path) else {
            return;
        };
        if dto.segments.is_empty() {
            return;
        }
        self.path.clear();
        self.seg_ids.clear();
        self.path_cells.clear();
        self.path_len = 0;
        for seg in &dto.segments {
            self.push_loaded_segment(seg);
        }
        self.seg_open = false;
        self.last_path_xz = None;
        self.dirty = false;
    }

    /// Aplica o log incremental do traçado (pontos gravados após o snapshot). Pontos
    /// contíguos de mesmo `seg_id` viram um segmento; o id do log só agrupa — aqui
    /// reatribuímos ids frescos para não colidir com os do snapshot.
    fn load_path_log(&mut self, path: &Path) {
        let pts = read_path_log(path);
        if pts.is_empty() {
            return;
        }
        let mut cur_src: Option<u64> = None;
        let mut seg: Vec<[f32; 6]> = Vec::new();
        for (src_id, p) in pts {
            if cur_src != Some(src_id) {
                self.push_loaded_segment(&seg);
                seg = Vec::new();
                cur_src = Some(src_id);
            }
            seg.push(p);
        }
        self.push_loaded_segment(&seg);
        self.seg_open = false;
        self.last_path_xz = None;
    }

    /// Empilha um segmento carregado (id fresco), somando ao `path_len` e marcando a
    /// cobertura em `path_cells`. Ignora segmentos com menos de 2 pontos.
    fn push_loaded_segment(&mut self, seg: &[[f32; 6]]) {
        if seg.len() < 2 {
            return;
        }
        let pts: Vec<PathPoint> = seg
            .iter()
            .map(|a| PathPoint {
                x: a[0],
                y: a[1],
                z: a[2],
                spd: a[3],
                temp: a[4],
                surf: a[5] as u8,
            })
            .collect();
        for p in &pts {
            self.path_cells
                .insert((self.cell_index(p.x), self.cell_index(p.z)));
        }
        self.path_len += pts.len();
        self.path.push(pts);
        self.seg_ids.push(self.next_seg_id);
        self.next_seg_id = self.next_seg_id.wrapping_add(1);
    }

    /// Funde uma única célula salva no acumulador em memória.
    fn merge_cell(&mut self, c: &HeightCell) {
        if c.n == 0 {
            return;
        }
        let e = self.cells.entry((c.cx, c.cz)).or_default();
        // Reconstrói a soma a partir da média salva (exato se mean = sum/n).
        e.sum_y += c.y_mean as f64 * c.n as f64;
        e.sum_spd += c.spd_mean as f64 * c.n as f64;
        e.sum_temp += c.temp_mean as f64 * c.n as f64;
        e.min_y = e.min_y.min(c.y_min);
        e.max_y = e.max_y.max(c.y_max);
        e.n += c.n;
        for k in 0..SURFACE_KINDS {
            e.surf[k] = e.surf[k].saturating_add(c.surf[k]);
        }
    }

    /// Drena o delta de persistência desde o último flush: células e pontos novos.
    /// Se o epoch divergiu (reset), pede truncamento e devolve o estado inteiro como
    /// base nova. Chamado pelo autosave sob o lock; a serialização/escrita acontece
    /// fora do lock. `None` quando nada mudou.
    pub fn drain_persist_delta(&mut self) -> Option<PersistDelta> {
        if self.persist_epoch != self.epoch {
            self.persist_epoch = self.epoch;
            self.pending_cells.clear();
            let cells: Vec<HeightCell> = self
                .cells
                .iter()
                .filter(|(_, a)| a.n > 0)
                .map(|(&(cx, cz), a)| cell_to_height(cx, cz, a))
                .collect();
            let mut path = Vec::new();
            for (seg, &id) in self.path.iter().zip(self.seg_ids.iter()) {
                for p in seg {
                    path.push((id, [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32]));
                }
            }
            self.seg_saved = self.path.iter().map(|s| s.len()).collect();
            return Some(PersistDelta {
                reset: true,
                cells,
                path,
            });
        }

        let mut cells = Vec::new();
        for key in self.pending_cells.drain(..) {
            if let Some(acc) = self.cells.get(&key) {
                if acc.n > 0 {
                    cells.push(cell_to_height(key.0, key.1, acc));
                }
            }
        }

        if self.seg_saved.len() < self.path.len() {
            self.seg_saved.resize(self.path.len(), 0);
        }
        let mut path = Vec::new();
        for i in 0..self.path.len() {
            let saved = self.seg_saved[i];
            let len = self.path[i].len();
            if saved < len {
                let id = self.seg_ids[i];
                for p in &self.path[i][saved..] {
                    path.push((id, [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32]));
                }
                self.seg_saved[i] = len;
            }
        }

        if cells.is_empty() && path.is_empty() {
            None
        } else {
            Some(PersistDelta {
                reset: false,
                cells,
                path,
            })
        }
    }

    /// Compacta o disco: reescreve os snapshots JSON completos (heightmap + traçado)
    /// e trunca os logs incrementais. Chamado ao sair e no `/fix`. Encerra o segmento
    /// aberto para a próxima sessão começar numa linha nova.
    pub fn compact(&mut self, path: &Path) -> std::io::Result<()> {
        self.save(path)?;
        self.save_path(path)?;
        std::fs::write(cells_log_path(path), b"")?;
        std::fs::write(path_log_path(path), b"")?;
        self.pending_cells.clear();
        self.seg_saved = self.path.iter().map(|s| s.len()).collect();
        self.persist_epoch = self.epoch;
        self.break_segment();
        Ok(())
    }

    /// Sobrescreve o snapshot do heightmap (células) com o estado atual.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self.to_heightmap())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(path, &json)
    }

    /// Sobrescreve o snapshot do traçado no arquivo companheiro de `path`.
    pub fn save_path(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self.to_path())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(&companion_path(path), &json)
    }
}

/// Monta a célula de rede a partir do acumulador em memória.
fn cell_to_height(cx: i32, cz: i32, acc: &CellAcc) -> HeightCell {
    HeightCell {
        cx,
        cz,
        y_mean: acc.mean(),
        y_min: acc.min_y,
        y_max: acc.max_y,
        n: acc.n,
        spd_mean: acc.spd_mean(),
        temp_mean: acc.temp_mean(),
        surf: acc.surf,
    }
}

fn read_heightmap(path: &Path) -> Option<WorldHeightmap> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn read_path(path: &Path) -> Option<PathDto> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Log incremental do heightmap: `worldmap-fh6.json` → `worldmap-fh6.ndjson`.
fn cells_log_path(hm_path: &Path) -> PathBuf {
    hm_path.with_file_name(format!("worldmap-{WORLD_ID}.ndjson"))
}

/// Log incremental do traçado: ao lado, `worldpath-fh6.ndjson`.
fn path_log_path(hm_path: &Path) -> PathBuf {
    hm_path.with_file_name(format!("worldpath-{WORLD_ID}.ndjson"))
}

/// Lê um log NDJSON, ignorando linhas malformadas (ex.: última linha truncada por
/// um crash no meio da escrita). Arquivo ausente → vazio.
fn read_cells_log(path: &Path) -> Vec<HeightCell> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<HeightCell>(l).ok())
        .collect()
}

fn read_path_log(path: &Path) -> Vec<(u64, [f32; 6])> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<(u64, [f32; 6])>(l).ok())
        .collect()
}

/// Anexa linhas NDJSON ao arquivo (cria se preciso). O(novo) por flush — não
/// reescreve o mapa inteiro, então escala para o mundo completo do FH6.
fn append_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let mut buf = String::with_capacity(lines.len() * 64);
    for l in lines {
        buf.push_str(l);
        buf.push('\n');
    }
    f.write_all(buf.as_bytes())
}

/// Escreve atômico (grava num `.tmp` e renomeia): um kill no meio não deixa o
/// arquivo (ex.: o heightmap de ~9 MB) truncado/corrompido. `rename` substitui
/// o destino existente em Windows e Unix.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Arquivo companheiro do traçado, ao lado do heightmap:
/// `worldmap-fh6.json` → `worldpath-fh6.json`.
fn companion_path(hm_path: &Path) -> PathBuf {
    hm_path.with_file_name(format!("worldpath-{WORLD_ID}.json"))
}

/// Default persistence path: `%APPDATA%/forza-hud-native/worldmap-fh6.json`,
/// the same directory the overlay uses for `layout.txt` and `theme.txt`.
#[must_use]
pub fn default_path() -> PathBuf {
    crate::paths::config_dir().join(format!("worldmap-{WORLD_ID}.json"))
}

/// Sobe a thread de autosave incremental: a cada `AUTOSAVE_SECS` drena só o delta
/// (células + pontos novos) e ANEXA aos logs NDJSON — O(novo), não reescreve o
/// mapa inteiro, então aguenta o mundo completo do FH6. A compactação (snapshot
/// JSON + truncar logs) acontece ao sair (`compact`). SÓ UM processo (o overlay,
/// dono único) deve chamar isto. A serialização roda fora do lock.
pub fn spawn_autosave(rec: Arc<Mutex<WorldMapRecorder>>, path: PathBuf) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("worldmap-autosave".into())
        .spawn(move || {
            let mut tick: u64 = 0;
            loop {
                thread::sleep(Duration::from_secs(AUTOSAVE_SECS));
                tick += 1;
                let delta = {
                    let mut g = rec.lock().unwrap();
                    g.drain_persist_delta()
                };
                if let Some(delta) = delta {
                    // Reset (epoch divergiu): zera os logs antes de regravar a base nova.
                    if delta.reset {
                        let _ = std::fs::write(cells_log_path(&path), b"");
                        let _ = std::fs::write(path_log_path(&path), b"");
                    }

                    let cell_lines: Vec<String> = delta
                        .cells
                        .iter()
                        .filter_map(|c| serde_json::to_string(c).ok())
                        .collect();
                    let path_lines: Vec<String> = delta
                        .path
                        .iter()
                        .filter_map(|p| serde_json::to_string(p).ok())
                        .collect();
                    let _ = append_lines(&cells_log_path(&path), &cell_lines);
                    let _ = append_lines(&path_log_path(&path), &path_lines);
                }

                // Snapshot JSON completo periodicamente — o banco fica recuperável
                // mesmo se o HUD não fechar com Ctrl+Alt+Q.
                if tick % COMPACT_EVERY_TICKS == 0 {
                    let mut g = rec.lock().unwrap();
                    if !g.cells.is_empty() || g.path_points() > 0 {
                        let _ = g.compact(&path);
                    }
                }
            }
        })
        .expect("thread worldmap-autosave")
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

    /// Igual a `dash_pos`, mas com sinais de superfície (rumble e poça uniformes).
    fn dash_surf(x: f32, y: f32, z: f32, rumble: f32, puddle: f32) -> Telemetry {
        Telemetry {
            surf_rumble: Some([rumble; 4]),
            puddle: Some([puddle; 4]),
            ..dash_pos(x, y, z)
        }
    }

    /// Recorder determinístico (célula de 8 m) para os testes de indexação.
    fn rec() -> WorldMapRecorder {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        r
    }

    #[test]
    fn indexa_celula_e_grava_altura() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        let hm = r.to_heightmap();
        assert_eq!(hm.cells.len(), 1);
        let c = &hm.cells[0];
        assert_eq!((c.cx, c.cz), (0, 0));
        assert_eq!(c.y_mean, 100.0);
        assert_eq!(c.n, 1);
        assert_eq!(hm.cell_size_m, 8.0);
        assert_eq!(hm.world_id, WORLD_ID);
    }

    #[test]
    fn celulas_distintas_por_posicao() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 10.0, 0.0), true);
        r.ingest(&dash_pos(100.0, 20.0, 0.0), true); // ~100m → outra célula
                                                     // salto de 100m > MAX_JUMP: reancora sem gravar; então move +5m e grava
        r.ingest(&dash_pos(105.0, 21.0, 0.0), true);
        let hm = r.to_heightmap();
        // célula (0,0) da 1ª amostra e célula ~13 da 3ª
        assert_eq!(hm.cells.iter().map(|c| c.n).sum::<u32>(), 2);
        assert_eq!(hm.cells.len(), 2);
    }

    #[test]
    fn celula_existente_nao_remarcar() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(2.0, 110.0, 0.0), true); // mesma célula (cx=0), move 2m
        let hm = r.to_heightmap();
        assert_eq!(hm.cells.len(), 1);
        let c = &hm.cells[0];
        assert_eq!(c.n, 1);
        assert_eq!(c.y_mean, 100.0);
    }

    #[test]
    fn movimento_infimo_nao_reamostra() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(0.02, 999.0, 0.0), true); // < MIN_MOVE_M (5 cm): ignorado
        let hm = r.to_heightmap();
        assert_eq!(hm.cells[0].n, 1);
        assert_eq!(hm.cells[0].y_mean, 100.0);
    }

    #[test]
    fn teleporte_descartado() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(5000.0, 7.0, 5000.0), true); // fast travel: descarta
        let hm = r.to_heightmap();
        assert_eq!(hm.cells.len(), 1); // só a 1ª amostra
        assert_eq!(hm.cells[0].cx, 0);
    }

    #[test]
    fn menu_e_sled_nao_gravam() {
        let mut r = WorldMapRecorder::new();
        r.ingest(&dash_pos(0.0, 100.0, 0.0), false); // não-ativo
        let mut sled = dash_pos(10.0, 100.0, 0.0);
        sled.sled_only = 1;
        r.ingest(&sled, true);
        assert_eq!(r.to_heightmap().cells.len(), 0);
    }

    #[test]
    fn classifica_superficie_por_celula() {
        let mut r = rec();
        // Asfalto (rumble baixo), depois move e pega terra (rumble médio).
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), true);
        r.ingest(&dash_surf(2.0, 10.0, 0.0, 0.01, 0.0), true); // mesma célula — não remarca
        r.ingest(&dash_surf(100.0, 10.0, 0.0, 0.20, 0.0), true); // salto: reancora
        r.ingest(&dash_surf(105.0, 10.0, 0.0, 0.20, 0.0), true); // outra célula, terra
        let hm = r.to_heightmap();
        let asphalt = hm.cells.iter().find(|c| c.cx == 0).unwrap();
        assert_eq!(asphalt.surf[surface::ASPHALT], 1);
        assert_eq!(asphalt.dominant_surface(), surface::ASPHALT as u8);
        let dirt = hm.cells.iter().find(|c| c.cx != 0).unwrap();
        assert_eq!(dirt.surf[surface::DIRT], 1);
    }

    #[test]
    fn volta_circuito_continua_no_mesmo_segmento_nao_duplica() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        let n = 36usize;
        let radius = 55.0f32;
        let mut after_lap1 = 0usize;

        for lap in 0..2 {
            for i in 0..n {
                let ang = std::f32::consts::TAU * i as f32 / n as f32;
                let x = radius * ang.cos();
                let z = radius * ang.sin();
                r.ingest(&dash_surf(x, 10.0, z, 0.01, 0.0), true);
            }
            if lap == 0 {
                after_lap1 = r.to_path().points;
                assert!(
                    after_lap1 >= 20,
                    "1ª volta deve gravar o circuito: {after_lap1}"
                );
            }
        }
        assert_eq!(
            r.to_path().points,
            after_lap1,
            "2ª volta contínua no mesmo segmento não deve regravar"
        );
    }

    #[test]
    fn nova_barra_so_longe_da_ponta_do_segmento() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let base = r.to_path().points;

        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        // Paralela a 5 m — perto da ponta, não cria barra.
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 5.0, 0.01, 0.0), true);
        }
        assert_eq!(r.to_path().points, base);

        r.ingest(&dash_surf(0.0, 10.0, 5.0, 0.01, 0.0), false);
        // Paralela a 32 m — longe da faixa e das pontas.
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 32.0, 0.01, 0.0), true);
        }
        assert!(r.to_path().points > base, "nova barra separada deve gravar");
    }

    #[test]
    fn segunda_volta_com_desvio_lateral_nao_duplica() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..25 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let pts1 = r.to_path().points;

        // 2ª volta ~3 m ao lado (desvio típico entre voltas).
        for i in 0..25 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 3.0, 0.01, 0.0), true);
        }
        assert_eq!(
            r.to_path().points,
            pts1,
            "desvio lateral dentro do halo não deve duplicar o traçado"
        );
    }

    #[test]
    fn tracado_continuo_nao_fragmenta_e_nao_regrava_nova_passagem() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        // 1ª passagem: linha reta contínua.
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p1 = r.to_path();
        assert_eq!(p1.segments.len(), 1, "direção contínua = um único segmento");
        let pts1 = p1.points;
        assert!(pts1 >= 18, "deve gravar a maioria dos pontos: {pts1}");

        // Para (fecha o segmento) e refaz exatamente o mesmo caminho. A fita já
        // cobre essa estrada, então o carro deve andar sem engrossar o traçado.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false); // inativo -> break
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p2 = r.to_path();
        assert_eq!(p2.segments.len(), 1);
        assert_eq!(p2.points, pts1, "2ª passagem não deve engrossar a fita");
    }

    #[test]
    fn nao_regrava_retorno_no_mesmo_segmento() {
        // Caso da volta em "U" sem parar: a ida já cobriu a estrada, então a
        // volta não deve criar uma segunda fita por cima da primeira.
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..30 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let out = r.to_path().points;
        for i in (0..29).rev() {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let back = r.to_path().points;
        assert_eq!(
            back, out,
            "retorno sobre pista coberta não deve regravar: out={out} back={back}"
        );
    }

    #[test]
    fn passagem_lateral_dentro_da_faixa_nao_engrossa_mas_rua_paralela_grava() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let base = r.to_path().points;

        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 5.0, 0.01, 0.0), true);
        }
        assert_eq!(
            r.to_path().points,
            base,
            "outro lado da mesma pista não deve alargar"
        );

        r.ingest(&dash_surf(0.0, 10.0, 5.0, 0.01, 0.0), false);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 32.0, 0.01, 0.0), true);
        }
        let parallel = r.to_path();
        assert!(
            parallel.points > base,
            "rua paralela separada precisa continuar gravando"
        );
        assert_eq!(parallel.segments.len(), 2);
    }

    #[test]
    fn agrega_velocidade_media_por_celula() {
        let mut r = rec();
        let mut a = dash_pos(0.0, 10.0, 0.0);
        a.speed = Some(80.0);
        r.ingest(&a, true);
        r.ingest(&dash_pos(100.0, 10.0, 0.0), true); // salto: reancora
        let mut c = dash_pos(105.0, 10.0, 0.0);
        c.speed = Some(120.0);
        r.ingest(&c, true);
        let hm = r.to_heightmap();
        let fast = hm.cells.iter().find(|c| c.cx != 0).unwrap();
        assert_eq!(fast.n, 1);
        assert_eq!(fast.spd_mean, 120.0);
    }

    #[test]
    fn agrega_temperatura_media_por_celula() {
        let mut r = rec();
        let mut a = dash_pos(0.0, 10.0, 0.0);
        a.tires = Some([160.0, 170.0, 180.0, 190.0]);
        r.ingest(&a, true);
        r.ingest(&dash_pos(100.0, 10.0, 0.0), true);
        let mut c = dash_pos(105.0, 10.0, 0.0);
        c.tires = Some([200.0, 210.0, 220.0, 230.0]);
        r.ingest(&c, true);
        let hm = r.to_heightmap();
        let hot = hm.cells.iter().find(|c| c.cx != 0).unwrap();
        assert_eq!(hot.n, 1);
        assert_eq!(hot.temp_mean, 215.0);
    }

    #[test]
    fn agua_tem_prioridade_sobre_vibracao() {
        // Poça funda mesmo com rumble alto → classifica como Água.
        let mut r = rec();
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.9, 0.5), true);
        let hm = r.to_heightmap();
        assert_eq!(hm.cells[0].surf[surface::WATER], 1);
    }

    #[test]
    fn sem_sinal_de_superficie_e_desconhecido() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 10.0, 0.0), true); // surf_rumble None
        let hm = r.to_heightmap();
        assert_eq!(hm.cells[0].surf[surface::UNKNOWN], 1);
    }

    #[test]
    fn merge_funde_e_soma() {
        let mut a = WorldMapRecorder::new();
        a.set_recording(true);
        a.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        let hm = a.to_heightmap();

        let mut b = WorldMapRecorder::new();
        b.set_recording(true);
        b.ingest(&dash_pos(0.0, 200.0, 0.0), true); // mesma célula (0,0)
        b.merge_from(&hm);

        let out = b.to_heightmap();
        assert_eq!(out.cells.len(), 1);
        assert_eq!(out.cells[0].n, 2);
        assert_eq!(out.cells[0].y_mean, 150.0); // (200 + 100) / 2
    }

    #[test]
    fn round_trip_save_load() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(50.0, 120.0, 0.0), true);
        let original = r.to_heightmap();

        let dir = std::env::temp_dir().join(format!("vry-wm-{}", std::process::id()));
        let path = dir.join("worldmap-test.json");
        r.save(&path).unwrap();

        let mut loaded = WorldMapRecorder::new();
        loaded.load_seed(&path);
        let back = loaded.to_heightmap();

        assert_eq!(original.cells.len(), back.cells.len());
        assert_eq!(original.cols, back.cols);
        assert_eq!(original.rows, back.rows);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn take_dirty_alterna() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        assert!(!r.take_dirty()); // limpo no início
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        assert!(r.take_dirty());
        assert!(!r.take_dirty()); // já consumido
    }

    #[test]
    fn sem_gravacao_nao_cria_celula_mas_guarda_pos() {
        let mut r = WorldMapRecorder::new();
        r.ingest(&dash_pos(10.0, 50.0, 20.0), true);
        assert_eq!(r.to_heightmap().cells.len(), 0);
        assert_eq!(r.last_pos(), Some([10.0, 50.0, 20.0]));
        r.set_recording(true);
        r.ingest(&dash_pos(12.0, 51.0, 20.0), true);
        assert_eq!(r.to_heightmap().cells.len(), 1);
    }

    #[test]
    fn delta_full_depois_incremental() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..10 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }

        // 1ª busca: epoch bate (0==0) mas `have` vazio → tudo cheio (from=0).
        let d0 = r.to_path_delta(0, &HashMap::new());
        assert!(!d0.full);
        assert_eq!(d0.order.len(), 1);
        assert_eq!(d0.changed.len(), 1);
        assert_eq!(d0.changed[0].from, 0);
        let id = d0.order[0];
        let len0 = d0.changed[0].pts.len();
        assert!(len0 >= 5, "1ª busca deve trazer o segmento: {len0}");

        // Dirige mais; cliente já tem len0 pontos → delta só anexa o novo.
        for i in 10..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let mut have = HashMap::new();
        have.insert(id, len0);
        let d1 = r.to_path_delta(d0.epoch, &have);
        assert_eq!(d1.order, vec![id]);
        assert_eq!(d1.changed.len(), 1);
        assert_eq!(
            d1.changed[0].from, len0,
            "deve anexar a partir do que o cliente tinha"
        );
        assert!(d1.changed[0].pts.len() >= 5);

        // Sem nada novo desde d1 → segmento inalterado não reenvia pontos.
        let total = len0 + d1.changed[0].pts.len();
        have.insert(id, total);
        let d2 = r.to_path_delta(d1.epoch, &have);
        assert_eq!(d2.order, vec![id]);
        assert!(
            d2.changed.is_empty(),
            "segmento inalterado não deve reenviar"
        );
    }

    #[test]
    fn delta_novo_segmento_ganha_id_proprio() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..6 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let d0 = r.to_path_delta(0, &HashMap::new());
        let id0 = d0.order[0];
        let len0 = d0.changed[0].pts.len();

        // Quebra (inativo) e dirige uma rua paralela separada → novo segmento.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        for i in 0..6 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 60.0, 0.01, 0.0), true);
        }

        let mut have = HashMap::new();
        have.insert(id0, len0);
        let d1 = r.to_path_delta(d0.epoch, &have);
        assert_eq!(d1.order.len(), 2, "deve haver dois segmentos");
        assert_eq!(d1.order[0], id0);
        assert_ne!(d1.order[1], id0, "novo segmento tem id distinto");
        // Só o novo segmento muda; o antigo (inalterado) fica de fora do `changed`.
        assert_eq!(d1.changed.len(), 1);
        assert_eq!(d1.changed[0].id, d1.order[1]);
        assert_eq!(d1.changed[0].from, 0);
    }

    #[test]
    fn delta_reset_bumpa_epoch_e_forca_full() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..6 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let d0 = r.to_path_delta(0, &HashMap::new());
        let e0 = d0.epoch;

        r.reset();
        // Cliente ainda no epoch antigo → full=true e traçado vazio.
        let d1 = r.to_path_delta(e0, &HashMap::new());
        assert!(d1.full, "epoch divergente deve marcar full");
        assert_ne!(d1.epoch, e0, "reset deve bumpar o epoch");
        assert!(d1.order.is_empty());
        assert!(d1.changed.is_empty());
    }

    #[test]
    fn path_persiste_e_base_restaurada_nao_duplica() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let before = r.to_path().points;
        assert!(before >= 15, "linha base deve gravar: {before}");

        let dir = std::env::temp_dir().join(format!("vry-wp-{}", std::process::id()));
        let hm_path = dir.join("worldmap-test.json");
        r.save(&hm_path).unwrap();
        r.save_path(&hm_path).unwrap();

        // Novo recorder (restart simulado): load_seed restaura células + traçado.
        let mut loaded = WorldMapRecorder::with_cell_size(8.0);
        loaded.load_seed(&hm_path);
        assert_eq!(
            loaded.to_path().points,
            before,
            "o traçado deve sobreviver ao restart"
        );

        // Dirigir por cima da base restaurada não deve duplicar a fita.
        loaded.set_recording(true);
        for i in 0..20 {
            loaded.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        assert_eq!(
            loaded.to_path().points,
            before,
            "base restaurada é fixa: não re-grava por cima"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fita_continua_crescendo_em_territorio_novo_apos_break() {
        // Regressão do bug "a faixa neon não se mantém": antes, após uma pausa
        // (break), dirigir em território NOVO não engordava mais a fita. Agora deve.
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p1 = r.to_path().points;
        assert!(p1 >= 15, "1ª passagem deve gravar: {p1}");

        // Pausa (menu/respawn = inativo → break) e segue em FRENTE, em estrada nova.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        for i in 20..40 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p2 = r.to_path().points;
        assert!(
            p2 > p1,
            "a fita deve continuar crescendo em território novo: p1={p1} p2={p2}"
        );
    }

    #[test]
    fn persistencia_incremental_sobrevive_via_log() {
        // Recuperação de crash: SEM snapshot JSON, só o log incremental (.ndjson)
        // gravado pelo autosave deve reconstruir células e traçado no boot.
        let dir = std::env::temp_dir().join(format!("vry-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let hm_path = dir.join("worldmap-test.json");

        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let pts = r.to_path().points;
        let cells = r.stats().cells;
        assert!(pts >= 15 && cells >= 15, "base: pts={pts} cells={cells}");

        // Simula um tick do autosave: drena o delta e ANEXA aos logs (sem compactar).
        let delta = r.drain_persist_delta().expect("delta não-vazio");
        assert!(!delta.reset, "1º drain de uma sessão fresca é incremental");
        let cell_lines: Vec<String> = delta
            .cells
            .iter()
            .filter_map(|c| serde_json::to_string(c).ok())
            .collect();
        let path_lines: Vec<String> = delta
            .path
            .iter()
            .filter_map(|p| serde_json::to_string(p).ok())
            .collect();
        append_lines(&cells_log_path(&hm_path), &cell_lines).unwrap();
        append_lines(&path_log_path(&hm_path), &path_lines).unwrap();

        // Restart "sujo" (crash): só os logs existem, nenhum snapshot.
        let mut loaded = WorldMapRecorder::with_cell_size(8.0);
        loaded.load_seed(&hm_path);
        assert_eq!(loaded.to_path().points, pts, "traçado recuperado do log");
        assert_eq!(loaded.stats().cells, cells, "células recuperadas do log");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
