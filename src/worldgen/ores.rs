//! Génération des minerais.
//!
//! Chaque chunk-source (3x3 autour du chunk courant) tire ses filons
//! d'un RNG dérivé du seed et de ses coordonnées ; seuls les blocs qui
//! tombent dans le chunk courant sont écrits. Résultat : filons continus
//! entre chunks, sans duplication, indépendants de l'ordre.

use super::buffer::{bm, ChunkBuffer};
use super::climate::Fields;
use super::biomes::Biome;
use super::rng::{hash_coords, Rng};

pub const STONE: u16 = 1;

struct OreDef {
    id: u16,
    tries: u32,
    size: i32,
    ymin: i32,
    ymax: i32,
}

const ORES: [OreDef; 6] = [
    OreDef { id: 16, tries: 14, size: 12, ymin: 5, ymax: 98 },   // coal
    OreDef { id: 15, tries: 10, size: 6, ymin: 5, ymax: 62 },    // iron
    OreDef { id: 14, tries: 2, size: 6, ymin: 5, ymax: 31 },     // gold
    OreDef { id: 73, tries: 6, size: 6, ymin: 5, ymax: 15 },     // redstone
    OreDef { id: 21, tries: 1, size: 5, ymin: 4, ymax: 30 },     // lapis
    OreDef { id: 56, tries: 1, size: 6, ymin: 5, ymax: 14 },     // diamond
];

/// Pose les filons de tous les chunks-sources pouvant atteindre ce chunk.
pub fn generate_ores(buf: &mut ChunkBuffer, fields: &Fields, seed: u64) {
    for scx in buf.cx - 1..=buf.cx + 1 {
        for scz in buf.cz - 1..=buf.cz + 1 {
            let mut rng = Rng::new(hash_coords(seed ^ 0x0BE5_1A7E, scx, scz, 0x0F1E));
            for ore in &ORES {
                for _ in 0..ore.tries {
                    let mut px = scx * 16 + rng.range_i32(0, 15);
                    let mut py = rng.range_i32(ore.ymin, ore.ymax);
                    let mut pz = scz * 16 + rng.range_i32(0, 15);
                    for _ in 0..ore.size {
                        place_ore(buf, px, py, pz, bm(ore.id, 0));
                        px += rng.range_i32(-1, 1);
                        py += rng.range_i32(-1, 1);
                        pz += rng.range_i32(-1, 1);
                        py = py.clamp(1, 250);
                    }
                }
            }
            // Émeraude : blocs isolés, montagnes uniquement.
            for _ in 0..4 {
                let px = scx * 16 + rng.range_i32(0, 15);
                let pz = scz * 16 + rng.range_i32(0, 15);
                let py = rng.range_i32(6, 32);
                if super::climate::sample_column(fields, px, pz).biome == Biome::Mountains {
                    place_ore(buf, px, py, pz, bm(129, 0));
                }
            }
        }
    }
}

#[inline]
fn place_ore(buf: &mut ChunkBuffer, wx: i32, wy: i32, wz: i32, ore: u16) {
    if let Some(cur) = buf.get_world(wx, wy, wz) {
        if cur == STONE {
            buf.set_world(wx, wy, wz, ore);
        }
    }
}
