//! Passe de surface : remplace les couches supérieures de pierre par les
//! matériaux du biome (herbe, sable, grès, gravier, argile...), pose la
//! neige et gèle l'eau.

use super::biomes::{params, Biome};
use super::buffer::{b, bm, ChunkBuffer};
use super::climate::{Column, SEA_LEVEL};
use super::rng::hash3;

pub const DIRT: u16 = 3;
pub const SAND: u16 = 12;
pub const GRAVEL: u16 = 13;
pub const CLAY: u16 = 82;
pub const SNOW_LAYER: u16 = bm(78, 0);
pub const ICE: u16 = b(79);
pub const STONE: u16 = 1;
pub const MYCELIUM: u16 = b(110);

#[inline]
fn col_jitter(x: i32, y: i32, z: i32, salt: u64) -> f64 {
    RngJitter(hash3(0x5A_F1, x, y, z, salt)).f64()
}

/// Petit lecteur de bits de hash : évite de construire un Rng complet
/// pour un simple jitter par colonne.
struct RngJitter(u64);
impl RngJitter {
    fn f64(&self) -> f64 {
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Applique les règles de surface sur tout le chunk.
pub fn apply_surface(buf: &mut ChunkBuffer, cols: &[Column], stride: usize) {
    for lx in 0..16usize {
        for lz in 0..16usize {
            let col = cols[(lz + 1) * stride + (lx + 1)];
            let h = col.height;
            if h <= 0 || h > 254 {
                continue;
            }
            let wx = buf.cx * 16 + lx as i32;
            let wz = buf.cz * 16 + lz as i32;

            if h < SEA_LEVEL {
                underwater_floor(buf, lx, lz, wx, wz, h, &col);
                continue;
            }

            let p = params(col.biome);
            let depth_jitter = (col_jitter(wx, 0, wz, 1) * 2.0) as i32;

            // Cas particuliers montagne : roche nue en altitude, éboulis
            // de gravier, neige au-delà de la ligne.
            if col.biome == Biome::Mountains {
                let snowy = col.temp < -0.15 || h > 106;
                if snowy {
                    fill_column(buf, lx, lz, h, STONE, STONE, 3);
                    buf.set(lx, (h + 1) as usize, lz, SNOW_LAYER);
                    continue;
                }
                if col.patch < -0.45 {
                    fill_column(buf, lx, lz, h, GRAVEL, GRAVEL, 3 + depth_jitter);
                    continue;
                }
                if h > 94 {
                    fill_column(buf, lx, lz, h, STONE, STONE, 2 + depth_jitter);
                    continue;
                }
            }

            let top = p.top;
            let filler = p.filler;
            let depth = p.filler_depth + depth_jitter;

            // Île champignon : sol de mycélium au lieu d'herbe.
            if col.biome == Biome::MushroomIsland {
                fill_column(buf, lx, lz, h, MYCELIUM, DIRT, depth.max(1));
                continue;
            }

            // Placards de gravier en surface sur les terres tempérées
            // (hors désert, champignons et biomes enneigés) : le canal
            // `patch` très négatif signale une zone caillouteuse.
            if col.biome != Biome::Desert
                && col.biome != Biome::MushroomIsland
                && col.biome != Biome::SnowyPlains
                && col.biome != Biome::SnowyTaiga
                && col.patch < -0.58
            {
                fill_column(buf, lx, lz, h, GRAVEL, GRAVEL, 2 + depth_jitter.max(0));
                continue;
            }

            // Sous le sable du désert : une assise de grès.
            if col.biome == Biome::Desert {
                fill_column(buf, lx, lz, h, SAND, SAND, depth);
                let sandstone_top = (h - depth).max(1);
                let ss_depth = 3 + ((col_jitter(wx, 1, wz, 2) * 2.0) as i32);
                for y in sandstone_top.saturating_sub(ss_depth)..=sandstone_top {
                    if y >= 1 && (buf.get(lx, y as usize, lz) & 0xFFF) == STONE {
                        buf.set(lx, y as usize, lz, p.deep);
                    }
                }
            } else {
                fill_column(buf, lx, lz, h, top, filler, depth.max(1));
            }

            // Couche de neige posée au sol.
            if p.snow_cover && buf.get(lx, (h + 1) as usize, lz) == 0 {
                buf.set(lx, (h + 1) as usize, lz, SNOW_LAYER);
            }
        }
    }

    // Gel des surfaces d'eau (une seconde passe pour parcourir la glace
    // après que toutes les colonnes ont leur hauteur finale).
    for lx in 0..16usize {
        for lz in 0..16usize {
            let col = cols[(lz + 1) * stride + (lx + 1)];
            if !params(col.biome).freeze_water {
                continue;
            }
            if col.height < SEA_LEVEL - 1 {
                let cur = buf.get(lx, SEA_LEVEL as usize, lz) & 0xFFF;
                if cur == 9 {
                    buf.set(lx, SEA_LEVEL as usize, lz, ICE);
                }
            }
        }
    }
}

fn fill_column(buf: &mut ChunkBuffer, lx: usize, lz: usize, h: i32, top: u16, filler: u16, depth: i32) {
    buf.set(lx, h as usize, lz, top);
    for y in (h - depth).max(1)..h {
        buf.set(lx, y as usize, lz, filler);
    }
}

/// Fond sous-marin : sable/gravier selon le canal `patch`, argile dans
/// les zones calmes (rivières, marais, hauts-fonds), dirt sinon.
fn underwater_floor(
    buf: &mut ChunkBuffer,
    lx: usize,
    lz: usize,
    wx: i32,
    wz: i32,
    h: i32,
    col: &Column,
) {
    let shallow = h >= SEA_LEVEL - 4;
    let clay_zone = (col.biome == Biome::River || col.biome == Biome::Swamp || shallow)
        && col.clay > 0.38;

    let (top, filler) = if clay_zone {
        (CLAY, DIRT)
    } else if col.patch > 0.25 {
        (SAND, SAND)
    } else if col.patch < -0.25 {
        (GRAVEL, GRAVEL)
    } else {
        (DIRT, DIRT)
    };

    let depth = 2 + (col_jitter(wx, 2, wz, 3) * 2.0) as i32;
    fill_column(buf, lx, lz, h, top, filler, depth);
}
