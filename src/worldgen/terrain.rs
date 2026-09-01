//! Remplissage de base du terrain et creusement des grottes.

use super::buffer::{b, ChunkBuffer};
use super::climate::{Column, SEA_LEVEL};
use super::noise::Fbm;
use super::rng::{hash3, Rng};

pub const BEDROCK: u16 = 7;
pub const STONE: u16 = 1;
pub const WATER: u16 = b(9);
pub const LAVA: u16 = b(11);

/// Champs 3D des grottes (construits une fois par chunk).
pub struct CaveFields {
    pub tunnel_a: Fbm,
    pub tunnel_b: Fbm,
    pub room: Fbm,
}

impl CaveFields {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0xCAFE_5EED_BABE);
        Self {
            tunnel_a: Fbm::new(&mut rng, 2, 1.0 / 85.0, 0.5),
            tunnel_b: Fbm::new(&mut rng, 2, 1.0 / 85.0, 0.5),
            room: Fbm::new(&mut rng, 2, 1.0 / 135.0, 0.5),
        }
    }
}

const CAVE_GRID_STEP: usize = 4;

/// Remplit le socle : bedrock en bas, pierre jusqu'à la hauteur de
/// colonne, eau jusqu'au niveau de la mer au-dessus du sol sous-marin.
pub fn base_fill(buf: &mut ChunkBuffer, cols: &[Column], stride: usize) {
    for lx in 0..16usize {
        for lz in 0..16usize {
            let col = cols[(lz + 1) * stride + (lx + 1)];
            let h = col.height;
            let wx = buf.cx * 16 + lx as i32;
            let wz = buf.cz * 16 + lz as i32;
            // Bedrock : couche 0 pleine, couches 1-3 rugueuses mais
            // déterministes.
            for y in 0..=h.min(255) {
                let v = if y == 0 {
                    BEDROCK
                } else if y <= 3 {
                    let r = Rng::new(hash3(0xBE_D0_C7, wx, y as i32, wz, 77)).next_f64();
                    if r < (4 - y) as f64 / 5.0 {
                        BEDROCK
                    } else {
                        STONE
                    }
                } else {
                    STONE
                };
                buf.set(lx, y as usize, lz, v);
            }
            if h < SEA_LEVEL {
                for y in (h + 1)..=SEA_LEVEL {
                    buf.set(lx, y as usize, lz, WATER);
                }
            }
        }
    }
}

/// Grille grossière de densité de grottes interpolée trilinéairement :
/// principe du champ de densité classique, coût divisé par ~64 par
/// rapport à un échantillonnage par bloc.
struct CaveGrid {
    nx: usize,
    ny: usize,
    nz: usize,
    y0: i32,
    a: Vec<f64>,
    b: Vec<f64>,
    room: Vec<f64>,
}

fn sample_grid(f: &CaveFields, cols: &[Column], stride: usize) -> CaveGrid {
    let y0 = 4i32;
    let max_h = cols.iter().map(|c| c.height).max().unwrap_or(SEA_LEVEL);
    let y1 = (max_h + 8).clamp(48, 118);
    let nx = 16 / CAVE_GRID_STEP + 1;
    let nz = nx;
    let ny = ((y1 - y0) as usize / CAVE_GRID_STEP) + 2;
    let len = nx * ny * nz;
    let mut a = vec![0.0; len];
    let mut tb = vec![0.0; len];
    let mut room = vec![0.0; len];

    for gx in 0..nx {
        for gz in 0..nz {
            let lx = (gx * CAVE_GRID_STEP).min(15);
            let lz = (gz * CAVE_GRID_STEP).min(15);
            let col = cols[(lz + 1) * stride + (lx + 1)];
            let wx = col.x as f64;
            let wz = col.z as f64;
            for gy in 0..ny {
                let y = y0 + (gy * CAVE_GRID_STEP) as i32;
                let i = (gy * nz + gz) * nx + gx;
                // Écrasement vertical : galeries plus larges que hautes.
                a[i] = f.tunnel_a.sample3(wx, y as f64, wz, 1.65);
                tb[i] = f.tunnel_b.sample3(wx, y as f64, wz, 1.65);
                room[i] = f.room.sample3(wx, y as f64, wz, 1.3);
            }
        }
    }

    CaveGrid { nx, ny, nz, y0, a, b: tb, room }
}

#[inline]
fn trilerp(g: &CaveGrid, field: &[f64], x: f64, y: f64, z: f64) -> f64 {
    let fx = (x / CAVE_GRID_STEP as f64).clamp(0.0, (g.nx - 1) as f64);
    let fy = ((y - g.y0 as f64) / CAVE_GRID_STEP as f64).clamp(0.0, (g.ny - 1) as f64);
    let fz = (z / CAVE_GRID_STEP as f64).clamp(0.0, (g.nz - 1) as f64);

    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let z0 = fz.floor() as usize;
    let x1 = (x0 + 1).min(g.nx - 1);
    let y1 = (y0 + 1).min(g.ny - 1);
    let z1 = (z0 + 1).min(g.nz - 1);
    let tx = fx - x0 as f64;
    let ty = fy - y0 as f64;
    let tz = fz - z0 as f64;

    #[inline]
    fn l(a: f64, b: f64, t: f64) -> f64 {
        a + (b - a) * t
    }
    let at = |xx: usize, yy: usize, zz: usize| field[(yy * g.nz + zz) * g.nx + xx];
    let c00 = l(at(x0, y0, z0), at(x1, y0, z0), tx);
    let c10 = l(at(x0, y1, z0), at(x1, y1, z0), tx);
    let c01 = l(at(x0, y0, z1), at(x1, y0, z1), tx);
    let c11 = l(at(x0, y1, z1), at(x1, y1, z1), tx);
    let c0 = l(c00, c10, ty);
    let c1 = l(c01, c11, ty);
    l(c0, c1, tz)
}

/// Creuse les grottes : intersection de deux surfaces de bruit 3D
/// (galeries tubulaires connectées) + salles profondes. Ne touche ni
/// l'eau ni le bedrock ; protège le plancher océanique.
pub fn carve_caves(buf: &mut ChunkBuffer, cols: &[Column], stride: usize, caves: &CaveFields) {
    let grid = sample_grid(caves, cols, stride);

    const TUNNEL_T: f64 = 0.058;
    const ROOM_T: f64 = 0.60;

    for lx in 0..16usize {
        for lz in 0..16usize {
            let col = cols[(lz + 1) * stride + (lx + 1)];
            let h = col.height;
            let underwater = h < SEA_LEVEL;
            // Sous l'eau : garder un plancher épais, ne pas drainer l'océan.
            let y_top = if underwater { h - 7 } else { h };
            if y_top < 5 {
                continue;
            }
            for y in 5..=(y_top.min(117)) {
                let cur = buf.get(lx, y as usize, lz);
                if cur == 0 || cur == WATER || (cur & 0xFFF) == BEDROCK {
                    continue;
                }
                let ta = trilerp(&grid, &grid.a, lx as f64, y as f64, lz as f64);
                let carved = if ta.abs() <= TUNNEL_T {
                    let tbn = trilerp(&grid, &grid.b, lx as f64, y as f64, lz as f64);
                    tbn.abs() <= TUNNEL_T
                } else if y < 36 {
                    trilerp(&grid, &grid.room, lx as f64, y as f64, lz as f64) >= ROOM_T
                } else {
                    false
                };
                if carved {
                    buf.set(lx, y as usize, lz, 0);
                }
            }
            // Lacs de lave profonds : tout vide creusé sous le niveau de
            // lave se remplit.
            for y in 5..=10 {
                if buf.get(lx, y as usize, lz) == 0 {
                    buf.set(lx, y as usize, lz, LAVA);
                }
            }
        }
    }
}
