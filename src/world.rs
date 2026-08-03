use std::collections::HashMap;
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
    pub world: Mutex<HashMap<(i32, i32, i32), u16>>,
    pub items: Mutex<HashMap<i32, ItemEntity>>,
    pub next_id: AtomicI32,
    pub tps: Mutex<TpsTracker>,
    pub config: ServerConfig,
    pub redstone_queue: Mutex<VecDeque<(i32, i32, i32)>>,
    pub redstone_delayed: Mutex<VecDeque<(u64, i32, i32, i32, u16)>>,
    pub tick_counter: AtomicU64,
}

pub type SharedState = Arc<State>;

pub fn get_block(world: &HashMap<(i32, i32, i32), u16>, x: i32, y: i32, z: i32) -> u16 {
    if let Some(&b) = world.get(&(x, y, z)) {
        b
    } else if y <= 15 {
        3
    } else {
        0
    }
}

/// Extrait un "snapshot" compact des blocs stockés explicitement (b != 0)
/// situés dans le carré de chunks de rayon `radius` autour de (cx, cz).
///
/// Contrairement à `world.clone()` (copie de TOUT le monde à chaque join ou
/// changement de chunk), on ne copie ici que les blocs réellement présents
/// dans la zone vue — ce qui est tout ce dont `build_chunk_packet` et
/// `find_safe_spawn` ont besoin (`get_block` synthétise le sol plat y <= 15).
pub fn extract_view_snapshot(
    world: &HashMap<(i32, i32, i32), u16>,
    cx: i32,
    cz: i32,
    radius: i32,
) -> HashMap<(i32, i32, i32), u16> {
    let min_cx = cx - radius;
    let max_cx = cx + radius;
    let min_cz = cz - radius;
    let max_cz = cz + radius;
    let mut m = HashMap::new();
    for (&(x, y, z), &b) in world.iter() {
        if b == 0 { continue; }
        let bcx = x >> 4;
        let bcz = z >> 4;
        if bcx >= min_cx && bcx <= max_cx && bcz >= min_cz && bcz <= max_cz && y >= 0 && y <= 255 {
            m.insert((x, y, z), b);
        }
    }
    m
}

pub fn compute_heightmap(world: &HashMap<(i32, i32, i32), u16>, chunk_x: i32, chunk_z: i32) -> [[i32; 16]; 16] {
    compute_heightmap_and_sections(world, chunk_x, chunk_z).0
}

/// Calcule la heightmap ET le nombre de sections en un seul passage.
///
/// Avant ce fix, `build_chunk_packet` appelait `compute_heightmap` (65536
/// lookups dans la hashmap du monde) PUIS `chunk_section_count` qui refaisait
/// exactement le même balayage colonne par colonne (encore 65536 lookups) rien
/// que pour connaître la hauteur max. Pour un simple join avec un rayon de vue
/// de 3 chunks (49 chunks), ça fait ~6.4M lookups de hashmap rien que pour ces
/// deux scans redondants, avant même de générer le contenu des sections -> gros
/// contributeur au "downloading terrain" qui traîne en longueur.
pub fn compute_heightmap_and_sections(
    world: &HashMap<(i32, i32, i32), u16>,
    chunk_x: i32,
    chunk_z: i32,
) -> ([[i32; 16]; 16], usize) {
    let mut heights = [[-1i32; 16]; 16];
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    let mut max_y = 0i32;
    for lx in 0..16 {
        for lz in 0..16 {
            let wx = base_x + lx as i32;
            let wz = base_z + lz as i32;
            for y in (0..256).rev() {
                if get_block(world, wx, y, wz) != 0 {
                    heights[lx][lz] = y;
                    max_y = max_y.max(y);
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
    world: &HashMap<(i32, i32, i32), u16>,
    heights: &[[i32; 16]; 16],
    chunk_x: i32,
    chunk_z: i32,
    section_y: i32,
) -> ([u8; 4096], [u8; 4096], [u8; 4096]) {
    let mut block_data = [0u8; 4096];
    let mut block_meta = [0u8; 4096];
    let mut sky_light = [0u8; 4096];
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    let y_start = section_y * 16;
    for lx in 0..16 {
        for lz in 0..16 {
            for ly in 0..16 {
                let wx = base_x + lx as i32;
                let wz = base_z + lz as i32;
                let wy = y_start + ly as i32;
                let idx = ly as usize * 256 + lz * 16 + lx;
                let stored = get_block(world, wx, wy, wz);
                block_data[idx] = stored as u8;
                block_meta[idx] = ((stored >> 12) & 0x0F) as u8;
                sky_light[idx] = if wy >= heights[lx][lz] { 15 } else { 0 };
            }
        }
    }
    (block_data, block_meta, sky_light)
}

#[allow(dead_code)] // remplacée par compute_heightmap_and_sections (scan unique)
pub fn chunk_section_count(world: &HashMap<(i32, i32, i32), u16>, chunk_x: i32, chunk_z: i32) -> usize {
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    let mut max_y = 0i32;
    for lx in 0..16 {
        for lz in 0..16 {
            let wx = base_x + lx;
            let wz = base_z + lz;
            for y in (0..256).rev() {
                if get_block(world, wx, y, wz) != 0 {
                    max_y = max_y.max(y);
                    break;
                }
            }
        }
    }
    (max_y / 16 + 1).clamp(1, 16) as usize
}

pub fn build_chunk_packet(chunk_x: i32, chunk_z: i32, world: &HashMap<(i32, i32, i32), u16>) -> Vec<u8> {
    let (heights, num_sections) = compute_heightmap_and_sections(world, chunk_x, chunk_z);

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
        sections.push(generate_section(world, &heights, chunk_x, chunk_z, s as i32));
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
    raw_data.resize(raw_data.len() + 256, 1);

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
