//! Ravines : longues entailles étroites générées par régions.
//!
//! Chaque région de 12x12 chunks tire ses ravines d'un RNG dérivé du
//! seed et des coordonnées de région ; la géométrie est purement en
//! coordonnées monde, donc chaque chunk recalcule la part qui le traverse
//! sans jamais dépendre de l'ordre de génération.

use super::buffer::ChunkBuffer;
use super::climate::{Column, SEA_LEVEL};
use super::rng::{hash_coords, Rng};

const REGION_CHUNKS: i32 = 12;
const REGION_BLOCKS: i32 = REGION_CHUNKS * 16;
const MAX_LEN: f64 = 170.0;

struct Ravine {
    cx: f64,
    cz: f64,
    dx: f64,
    dz: f64,
    len: f64,
    half_w: f64,
    yc: f64,
    hh: f64,
    phase: f64,
}

fn region_ravines(seed: u64, rx: i32, rz: i32) -> Vec<Ravine> {
    let mut rng = Rng::new(hash_coords(seed ^ 0x5A17_1E5E, rx, rz, 0xA11CE));
    let mut out = Vec::new();
    let mut count = 0;
    if rng.chance(0.55) {
        count += 1;
    }
    if rng.chance(0.15) {
        count += 1;
    }
    for _ in 0..count {
        let angle = rng.next_f64() * std::f64::consts::TAU;
        out.push(Ravine {
            cx: (rx * REGION_BLOCKS + rng.range_i32(0, REGION_BLOCKS - 1)) as f64,
            cz: (rz * REGION_BLOCKS + rng.range_i32(0, REGION_BLOCKS - 1)) as f64,
            dx: angle.cos(),
            dz: angle.sin(),
            len: 70.0 + rng.next_f64() * 100.0,
            half_w: 2.2 + rng.next_f64() * 2.4,
            yc: 18.0 + rng.next_f64() * 24.0,
            hh: 7.0 + rng.next_f64() * 8.0,
            phase: rng.next_f64() * std::f64::consts::TAU,
        });
    }
    out
}

/// Creuse les ravines traversant ce chunk.
pub fn carve_ravines(buf: &mut ChunkBuffer, cols: &[Column], stride: usize, seed: u64) {
    let rx0 = div_floor(buf.cx, REGION_CHUNKS);
    let rz0 = div_floor(buf.cz, REGION_CHUNKS);
    let mut candidates = Vec::new();
    for rx in rx0 - 1..=rx0 + 1 {
        for rz in rz0 - 1..=rz0 + 1 {
            candidates.extend(region_ravines(seed, rx, rz));
        }
    }
    if candidates.is_empty() {
        return;
    }

    for lx in 0..16usize {
        for lz in 0..16usize {
            let col = cols[(lz + 1) * stride + (lx + 1)];
            let h = col.height;
            let underwater = h < SEA_LEVEL;
            let y_top = if underwater { h - 7 } else { h };
            if y_top < 6 {
                continue;
            }
            let px = (buf.cx * 16 + lx as i32) as f64;
            let pz = (buf.cz * 16 + lz as i32) as f64;

            for r in &candidates {
                // Projection sur le segment central.
                let rel_x = px - r.cx;
                let rel_z = pz - r.cz;
                let t = ((rel_x * r.dx + rel_z * r.dz) / r.len).clamp(0.0, 1.0);
                let qx = r.cx + r.dx * t * r.len;
                let qz = r.cz + r.dz * t * r.len;
                let dist = ((px - qx).powi(2) + (pz - qz).powi(2)).sqrt();
                let width = r.half_w
                    * (0.55 + 0.45 * (t * std::f64::consts::PI).sin())
                    * (1.0 + 0.30 * (t * 9.0 + r.phase).sin());
                if dist >= width {
                    continue;
                }

                let yc_t = r.yc + (t * 5.0 + r.phase).sin() * 5.0;
                let dd = dist / width;
                for y in 5..=(y_top.min(200)) {
                    let cur = buf.get(lx, y as usize, lz);
                    if cur == 0 || cur == super::terrain::WATER || (cur & 0xFFF) == super::terrain::BEDROCK {
                        continue;
                    }
                    let rel_y = (y as f64 - yc_t) / r.hh;
                    if dd * dd + rel_y * rel_y <= 1.0 {
                        buf.set(lx, y as usize, lz, 0);
                    }
                }
            }
        }
    }
}

#[inline]
fn div_floor(a: i32, b: i32) -> i32 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}
