use std::collections::{HashMap, HashSet};
use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use std::io::Write as _;
use std::sync::atomic::AtomicI32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::sync::Mutex;

use crate::config::ServerConfig;
use crate::items::ItemEntity;
use crate::net::writing::build_packet;
use crate::player::Player;
use crate::worldgen::WorldGenerator;

#[derive(Clone)]
pub struct TpsTracker {
    history: VecDeque<Instant>,
    last_cpu_total: u64,
    last_cpu_time: Instant,
    cpu_percent: f64,
    tick_counter: u32,
}

impl TpsTracker {
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(18001),
            last_cpu_total: 0,
            last_cpu_time: Instant::now(),
            cpu_percent: 0.0,
            tick_counter: 0,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.history.push_back(now);
        if self.history.len() > 18000 {
            self.history.pop_front();
        }
        self.tick_counter += 1;
        if self.tick_counter >= 20 {
            self.tick_counter = 0;
            if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
                let parts: Vec<&str> = stat.split_whitespace().collect();
                if parts.len() >= 15 {
                    let utime: u64 = parts[13].parse().unwrap_or(0);
                    let stime: u64 = parts[14].parse().unwrap_or(0);
                    let cpu_total = utime + stime;
                    let cpu_delta = cpu_total.saturating_sub(self.last_cpu_total);
                    let wall_delta = now.duration_since(self.last_cpu_time).as_secs_f64();
                    if self.last_cpu_total > 0 && wall_delta > 0.0 {
                        self.cpu_percent = cpu_delta as f64 / 100.0 / wall_delta * 100.0;
                    }
                    self.last_cpu_total = cpu_total;
                    self.last_cpu_time = now;
                }
            }
        }
    }

    pub fn cpu(&self) -> f64 {
        self.cpu_percent
    }

    pub fn tps(&self, window: Duration) -> f64 {
        if self.history.len() < 2 {
            return 0.0;
        }
        let cutoff = Instant::now() - window;
        let count = self.history.iter().filter(|&&t| t > cutoff).count();
        if count == 0 {
            return 0.0;
        }
        count as f64 / window.as_secs_f64()
    }
}

pub struct State {
    pub players: Mutex<HashMap<i32, Player>>,
    /// Monde en stockage PAR-CHUNK : clé (cx, cz) -> tableau dense 16x16x256
    /// (idx = (y<<8)|(z<<4)|x, même layout que le buffer de génération).
    /// Un accès bloc = 1 lookup de hashmap + 1 index de tableau, et les
    /// snapshots/sauvegardes ne touchent que les chunks concernés au lieu
    /// de balayer toute la map globale.
    pub world: Mutex<WorldMap>,
    pub items: Mutex<HashMap<i32, ItemEntity>>,
    pub next_id: AtomicI32,
    pub tps: Mutex<TpsTracker>,
    pub config: ServerConfig,
    pub redstone_queue: Mutex<VecDeque<(i32, i32, i32)>>,
    pub redstone_delayed: Mutex<VecDeque<(u64, i32, i32, i32, u16)>>,
    pub tick_counter: AtomicU64,
    /// Générateur procédural (immutables : thread-safe par construction).
    pub generator: WorldGenerator,
    /// Chunks déjà générés et fusionnés dans `world` (session courante).
    pub generated_chunks: Mutex<HashSet<(i32, i32)>>,
    /// Éditions disque en attente (chunks sauvegardés pas encore régénérés) :
    /// appliquées PAR-DESSUS le terrain procédural lors de la génération.
    pub pending_edits: Mutex<HashMap<(i32, i32), HashMap<(i32, i32, i32), u16>>>,
}

pub type SharedState = Arc<State>;

/// Longueur d'un tableau de chunk dense (16*16*256).
pub const CHUNK_LEN: usize = 65536;

/// Données d'un chunk fusionné : blocs denses + biomes par colonne +
/// flag "contient des plaques de pression" (évite à la boucle de tick de
/// redstone de balayer toute la map 20 fois par seconde).
pub struct ChunkData {
    pub blocks: Box<[u16; CHUNK_LEN]>,
    /// Biome par colonne (index z*16+x, ids protocole 1.7.10).
    pub biomes: [u8; 256],
    pub has_plates: bool,
}

impl Default for ChunkData {
    fn default() -> Self {
        // Biomes "plaines" par défaut : identique à l'ancien comportement
        // du paquet pour les chunks créés hors génération.
        Self { blocks: Box::new([0u16; CHUNK_LEN]), biomes: [1u8; 256], has_plates: false }
    }
}

impl ChunkData {
    #[inline]
    pub fn get(&self, lx: usize, y: usize, lz: usize) -> u16 {
        self.blocks[(y << 8) | (lz << 4) | lx]
    }

    #[inline]
    pub fn set(&mut self, lx: usize, y: usize, lz: usize, v: u16) {
        self.blocks[(y << 8) | (lz << 4) | lx] = v;
    }
}

pub type WorldMap = HashMap<(i32, i32), ChunkData>;

const PLATE_IDS: [u16; 4] = [70, 72, 147, 148];

#[inline]
fn is_plate(stored: u16) -> bool {
    PLATE_IDS.contains(&(stored & 0xFFF))
}

/// Lecture d'un bloc. Un chunk absent de la map est entièrement air : le
/// terrain réel est généré explicitement dans la map via `ensure_chunk`.
#[inline]
pub fn get_block(world: &WorldMap, x: i32, y: i32, z: i32) -> u16 {
    if y < 0 || y >= 256 {
        return 0;
    }
    match world.get(&(x >> 4, z >> 4)) {
        Some(c) => c.get((x & 15) as usize, y as usize, (z & 15) as usize),
        None => 0,
    }
}

/// Écriture d'un bloc (crée le chunk dense si absent). Met à jour le flag
/// plaques de pression pour le tick redstone.
pub fn set_block(world: &mut WorldMap, x: i32, y: i32, z: i32, v: u16) {
    if y < 0 || y >= 256 {
        return;
    }
    let chunk = world.entry((x >> 4, z >> 4)).or_default();
    chunk.set((x & 15) as usize, y as usize, (z & 15) as usize, v);
    if is_plate(v) {
        chunk.has_plates = true;
    }
}

/// Variante à coordonnées groupées de `set_block` (remplace les anciens
/// `world.insert((x, y, z), v)` sur la map globale).
pub fn set_block_at(world: &mut WorldMap, pos: (i32, i32, i32), v: u16) {
    set_block(world, pos.0, pos.1, pos.2, v);
}

/// Hauteur du premier bloc non-air d'une colonne (None si vide).
pub fn surface_y(world: &WorldMap, x: i32, z: i32) -> Option<i32> {
    (0..256).rev().find(|&y| get_block(world, x, y, z) != 0)
}

/// Génère le terrain brut d'un chunk (pur, hors locks). Les éditions
/// sauvegardées ne sont PAS appliquées ici : elles le sont au merge, une
/// seule fois, ce qui permet de générer en parallèle sans risque.
fn generate_raw(state: &SharedState, cx: i32, cz: i32) -> (Box<[u16; CHUNK_LEN]>, [u8; 256]) {
    state.generator.generate_chunk(cx, cz).into_parts()
}

/// Fusionne un chunk brut dans la map monde (applique les éditions
/// sauvegardées, détecte les plaques). Thread-safe : le retrait des éditions
/// se fait SOUS le lock de double-check, donc un seul thread les applique —
/// impossible qu'un thread fusionne le terrain brut en perdant les éditions.
fn merge_chunk(
    state: &SharedState,
    cx: i32,
    cz: i32,
    mut data: Box<[u16; CHUNK_LEN]>,
    biomes: [u8; 256],
) {
    let mut world = state.world.lock().unwrap();
    let mut g = state.generated_chunks.lock().unwrap();
    if g.contains(&(cx, cz)) {
        // Un autre thread a généré le même chunk entre-temps : le sien
        // (identique car déterministe, mêmes éditions appliquées) a déjà
        // été fusionné.
        return;
    }
    let edits = state.pending_edits.lock().unwrap().remove(&(cx, cz));
    let mut has_plates = false;
    if let Some(edits) = &edits {
        for (&(x, y, z), &v) in edits {
            if y < 0 || y >= 256 { continue; }
            let idx = ((y as usize) << 8) | (((z & 15) as usize) << 4) | (x & 15) as usize;
            data[idx] = v;
        }
    }
    for &b in data.iter() {
        if is_plate(b) {
            has_plates = true;
            break;
        }
    }
    world.insert((cx, cz), ChunkData { blocks: data, biomes, has_plates });
    g.insert((cx, cz));
}

/// Garantit qu'un chunk est généré (terrain procédural + éditions
/// sauvegardées) et fusionné dans la map monde.
pub fn ensure_chunk(state: &SharedState, cx: i32, cz: i32) {
    {
        let g = state.generated_chunks.lock().unwrap();
        if g.contains(&(cx, cz)) {
            return;
        }
    }
    let data = generate_raw(state, cx, cz);
    merge_chunk(state, cx, cz, data.0, data.1);
}

/// Génère tous les chunks du carré (2*radius+1) autour de (cx, cz).
///
/// Les chunks manquants sont générés EN PARALLÈLE (la génération est pure :
/// même seed + mêmes coords => résultat identique), puis fusionnés en série
/// sous lock. Sur un join à rayon 3 (49 chunks), ça divise le temps de
/// génération par le nombre de cœurs disponibles.
pub fn ensure_area(state: &SharedState, cx: i32, cz: i32, radius: i32) {
    let missing: Vec<(i32, i32)> = {
        let g = state.generated_chunks.lock().unwrap();
        let mut v = Vec::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let c = (cx + dx, cz + dz);
                if !g.contains(&c) {
                    v.push(c);
                }
            }
        }
        v
    };
    if missing.is_empty() {
        return;
    }

    if missing.len() > 1 {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
        if threads > 1 && missing.len() >= 4 {
            type RawChunk = (Box<[u16; CHUNK_LEN]>, [u8; 256]);
            let results: Mutex<Vec<((i32, i32), RawChunk)>> = Mutex::new(Vec::with_capacity(missing.len()));
            let batch_size = missing.len().div_ceil(threads);
            std::thread::scope(|s| {
                for batch in missing.chunks(batch_size) {
                    let results = &results;
                    let generator = &state.generator;
                    s.spawn(move || {
                        let mut local = Vec::with_capacity(batch.len());
                        for &(ccx, ccz) in batch {
                            local.push(((ccx, ccz), generator.generate_chunk(ccx, ccz).into_parts()));
                        }
                        results.lock().unwrap().extend(local);
                    });
                }
            });
            for ((gcx, gcz), (data, biomes)) in results.into_inner().unwrap() {
                merge_chunk(state, gcx, gcz, data, biomes);
            }
            return;
        }
    }

    for (ccx, ccz) in missing {
        let data = generate_raw(state, ccx, ccz);
        merge_chunk(state, ccx, ccz, data.0, data.1);
    }
}

/// Sauvegarde complète : chunks fusionnés + éditions en attente.
///
/// L'encodage binaire se fait SOUS lock (memcpy rapide, pas de clone de la
/// map), puis l'IO disque se fait HORS lock pour ne pas bloquer le tick.
/// Retourne le nombre de chunks persistés.
pub fn persist_world(state: &SharedState) -> std::io::Result<usize> {
    let encoded: Vec<((i32, i32), Vec<u8>)> = {
        let world = state.world.lock().unwrap();
        world.iter().map(|(&(cx, cz), chunk)| ((cx, cz), crate::save::encode_chunk_data(&chunk.blocks))).collect()
    };
    let known: HashSet<(i32, i32)> = {
        let g = state.generated_chunks.lock().unwrap();
        let p = state.pending_edits.lock().unwrap();
        g.union(&p.keys().copied().collect()).copied().collect()
    };
    let n_chunks = encoded.len();
    crate::save::save_all_chunks(encoded, &known)?;
    let pending = state.pending_edits.lock().unwrap();
    crate::save::save_pending_edits(&pending)?;
    Ok(n_chunks)
}

/// Extrait un "snapshot" compact des blocs non-air situés dans le carré de
/// chunks de rayon `radius` autour de (cx, cz).
///
/// Avec le stockage par-chunk, on ne balaye QUE les (2r+1)^2 tableaux denses
/// concernés — pas la map globale. Utilisé pour les vérifications de spawn
/// (find_safe_spawn) ; les paquets de chunk lisent directement le tableau
/// du chunk via `build_chunk_packet`.
pub fn extract_view_snapshot(
    world: &WorldMap,
    cx: i32,
    cz: i32,
    radius: i32,
) -> HashMap<(i32, i32, i32), u16> {
    let mut m = HashMap::new();
    for ccx in (cx - radius)..=(cx + radius) {
        for ccz in (cz - radius)..=(cz + radius) {
            if let Some(chunk) = world.get(&(ccx, ccz)) {
                for (idx, &b) in chunk.blocks.iter().enumerate() {
                    if b != 0 {
                        let y = (idx >> 8) as i32;
                        let lz = ((idx >> 4) & 15) as i32;
                        let lx = (idx & 15) as i32;
                        m.insert((ccx * 16 + lx, y, ccz * 16 + lz), b);
                    }
                }
            }
        }
    }
    m
}

pub fn compute_heightmap(data: &[u16; CHUNK_LEN]) -> [[i32; 16]; 16] {
    compute_heightmap_and_sections(data).0
}

/// Calcule la heightmap ET le nombre de sections en un seul passage sur le
/// tableau dense du chunk (accès directs, zéro lookup de hashmap).
pub fn compute_heightmap_and_sections(data: &[u16; CHUNK_LEN]) -> ([[i32; 16]; 16], usize) {
    let mut heights = [[-1i32; 16]; 16];
    let mut max_y = 0i32;
    for lx in 0..16usize {
        for lz in 0..16usize {
            for y in (0..256).rev() {
                if data[(y << 8) | (lz << 4) | lx] != 0 {
                    heights[lx][lz] = y as i32;
                    max_y = max_y.max(y as i32);
                    break;
                }
            }
        }
    }
    let num_sections = (max_y / 16 + 1).clamp(1, 16) as usize;
    (heights, num_sections)
}

pub fn pack_nibbles(values: &[u8; 4096]) -> Vec<u8> {
    let mut result = Vec::with_capacity(2048);
    for i in 0..2048 {
        let byte = (values[2 * i + 1] << 4) | (values[2 * i] & 0x0F);
        result.push(byte);
    }
    result
}

pub fn generate_section(
    data: &[u16; CHUNK_LEN],
    heights: &[[i32; 16]; 16],
    section_y: i32,
) -> ([u8; 4096], [u8; 4096], [u8; 4096]) {
    let mut block_data = [0u8; 4096];
    let mut block_meta = [0u8; 4096];
    let mut sky_light = [0u8; 4096];
    let y_start = section_y * 16;
    for lx in 0..16usize {
        for lz in 0..16usize {
            for ly in 0..16usize {
                let wy = y_start as usize + ly;
                let idx = ly * 256 + lz * 16 + lx;
                let stored = data[(wy << 8) | (lz << 4) | lx];
                block_data[idx] = stored as u8;
                block_meta[idx] = ((stored >> 12) & 0x0F) as u8;
                sky_light[idx] = if wy as i32 >= heights[lx][lz] { 15 } else { 0 };
            }
        }
    }
    (block_data, block_meta, sky_light)
}

#[allow(dead_code)] // remplacée par compute_heightmap_and_sections (scan unique)
pub fn chunk_section_count(data: &[u16; CHUNK_LEN]) -> usize {
    compute_heightmap_and_sections(data).1
}

pub fn build_chunk_packet(chunk_x: i32, chunk_z: i32, world: &WorldMap) -> Vec<u8> {
    static EMPTY: std::sync::OnceLock<ChunkData> = std::sync::OnceLock::new();
    let empty = EMPTY.get_or_init(ChunkData::default);
    let chunk = world.get(&(chunk_x, chunk_z)).unwrap_or(empty);
    let data: &[u16; CHUNK_LEN] = &chunk.blocks;
    let biomes: &[u8; 256] = &chunk.biomes;

    let (heights, num_sections) = compute_heightmap_and_sections(data);

    // Le format 1.7.10 range les tableaux PAR TYPE à travers toutes les
    // sections (tous les block IDs de toutes les sections, PUIS toutes les
    // métadonnées, PUIS toute la block light, PUIS toute la sky light) - pas
    // section par section. Avec une seule section (couches 0-15, le cas le
    // plus courant) les deux ordres donnent le même résultat, ce qui masquait
    // le bug ; dès qu'une 2e section existe (quelque chose au-dessus de la
    // couche 15), l'ancien ordre (groupé par section) désynchronisait tout ce
    // qui suit -> plus rien au-dessus de la couche 15 ne s'affichait.
    let mut sections = Vec::with_capacity(num_sections);
    for s in 0..num_sections {
        sections.push(generate_section(data, &heights, s as i32));
    }

    let mut raw_data = Vec::with_capacity(num_sections * 4096 + num_sections * 2048 * 3 + 256);
    for (blocks, _, _) in &sections {
        raw_data.extend(blocks);
    }
    for (_, meta, _) in &sections {
        raw_data.extend(pack_nibbles(meta));
    }
    for _ in &sections {
        raw_data.resize(raw_data.len() + 2048, 0); // block light (non géré, toujours 0)
    }
    for (_, _, sky) in &sections {
        raw_data.extend(pack_nibbles(sky));
    }
    // Biomes réels par colonne (générés dans le buffer, cf. generate_chunk).
    raw_data.extend_from_slice(&biomes[..]);

    // Compression rapide : le niveau 6 par défaut coûte cher en CPU au moment
    // du join/chargement des chunks ; le niveau 1 est bien assez compressé
    // pour des données Minecraft et beaucoup plus rapide.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&raw_data).expect("compress chunk");
    let compressed_data = encoder.finish().expect("finish compression");

    let mut content = Vec::with_capacity(4 + 4 + 1 + 2 + 2 + 4 + compressed_data.len());
    content.extend(chunk_x.to_be_bytes());
    content.extend(chunk_z.to_be_bytes());
    content.push(1u8);
    let primary_bitmap = if num_sections >= 16 { 0xFFFFu16 } else { ((1u16 << num_sections) - 1) as u16 };
    content.extend(primary_bitmap.to_be_bytes());
    content.extend((0u16).to_be_bytes());
    content.extend((compressed_data.len() as i32).to_be_bytes());
    content.extend(compressed_data);

    build_packet(0x21, &mut content)
}
