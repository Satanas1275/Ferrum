//! Sources d'eau et de lave, et cascades.
//!
//! Décidées par chunk-source (3x3 autour du chunk courant), comme les
//! autres features : RNG dérivé du seed + coordonnées, positions en
//! coordonnées monde, et seule la part tombant dans chaque chunk est
//! écrite. Aucune dépendance à l'ordre de génération.

use super::biomes::Biome;
use super::buffer::{b, ChunkBuffer};
use super::climate::{sample_column, Fields, SEA_LEVEL};
use super::rng::{hash_coords, Rng};

const SALT_SPRING: u64 = 0x5A4_1F60;
const SALT_FALL: u64 = 0xCA5C_AD3A;

const WATER: u16 = b(9);
const LAVA: u16 = b(11);
const STONE: u16 = 1;

/// Pose les sources d'eau/lave et les cascades touchant ce chunk.
pub fn decorate_liquids(buf: &mut ChunkBuffer, fields: &Fields, seed: u64) {
    for scx in buf.cx - 1..=buf.cx + 1 {
        for scz in buf.cz - 1..=buf.cz + 1 {
            place_springs(buf, fields, seed, scx, scz);
            place_waterfall(buf, fields, seed, scx, scz);
        }
    }
}

/// Sources liquides : une source d'eau isolée sur les terres tempérées,
/// ou une source de lave ponctuelle sur les pentes rocheuses.
fn place_springs(buf: &mut ChunkBuffer, fields: &Fields, seed: u64, scx: i32, scz: i32) {
    let mut rng = Rng::new(hash_coords(seed ^ SALT_SPRING, scx, scz, 0xB0B1));
    // Une grille d'essais clairsemée : 9 tentatives par source chunk.
    for _ in 0..9u32 {
        if !rng.chance(0.03) {
            continue;
        }
        let tx = scx * 16 + rng.range_i32(2, 13);
        let tz = scz * 16 + rng.range_i32(2, 13);
        let col = sample_column(fields, tx, tz);
        if col.height <= SEA_LEVEL || col.height > 200 {
            continue;
        }
        let wl = col.height;
        // Ne plaque que là où le sol du chunk courant existe encore.
        if let Some(cur) = buf.get_world(tx, wl, tz) {
            let id = cur & 0xFFF;
            if id == 0 || id == 9 || id == 11 {
                continue;
            }
            let lava = col.temp < -0.5 || col.biome == Biome::Mountains;
            // Source d'eau : surtout sur les terres tempérées.
            if !lava && col.temp > -0.35 {
                buf.set_world(tx, wl + 1, tz, WATER);
            } else if lava && rng.chance(0.5) {
                // Petite flaque de lave en surface sur sol rocheux.
                buf.set_world(tx, wl + 1, tz, LAVA);
            }
        }
    }
}

/// Cascade : à un bord de falaise (dénivelé brusque entre deux colonnes
/// voisines), on pose un bloc d'eau au sommet pour matérialiser l'écoulement.
fn place_waterfall(buf: &mut ChunkBuffer, fields: &Fields, seed: u64, scx: i32, scz: i32) {
    let mut rng = Rng::new(hash_coords(seed ^ SALT_FALL, scx, scz, 0xD1FF));
    let tx = scx * 16 + rng.range_i32(1, 14);
    let tz = scz * 16 + rng.range_i32(1, 14);
    if !rng.chance(0.012) {
        return;
    }
    let col = sample_column(fields, tx, tz);
    // Un des quatre voisins est nettement plus bas -> bord de falaise.
    let both = [(-1i32, 0), (1, 0), (0, -1), (0, 1)];
    let mut best_drop = 0;
    for (dx, dz) in both {
        let n = sample_column(fields, tx + dx, tz + dz);
        best_drop = best_drop.max(col.height - n.height);
    }
    if best_drop < 4 {
        return;
    }
    if col.height <= SEA_LEVEL || col.height > 190 {
        return;
    }
    if let Some(cur) = buf.get_world(tx, col.height, tz) {
        let id = cur & 0xFFF;
        if id == 0 || id == 9 || id == 11 || id == 12 || id == 13 {
            return;
        }
        buf.set_world(tx, col.height + 1, tz, WATER);
        buf.set_world(tx, col.height, tz, WATER);
    }
}
