//! Décoration du monde : arbres, végétation basse et étangs.
//!
//! Toutes les features sont décidées par chunk-SOURCE (3x3 autour du
//! chunk courant) : un arbre planté près d'une bordure est rendu pour sa
//! part par chaque chunk qu'il touche, à partir de décisions identiques
//! (RNG dérivé du seed + coordonnées monde de l'arbre). Aucune dépendance
//! à l'ordre de génération, aucune duplication.

use super::biomes::{params, Biome, TreeSpec, tree_spec};
use super::buffer::{b, bm, ChunkBuffer};
use super::climate::{sample_column, Column, Fields, SEA_LEVEL};
use super::rng::{hash3, hash_coords, Rng};

const SALT_TREE: u64 = 0x7EE_1A7E;
const SALT_SHAPE: u64 = 0x5EA7_E110;
const SALT_VEG: u64 = 0x7E6E_7A7;
const SALT_POND: u64 = 0x90AD_55;

pub const WATER: u16 = b(9);

pub fn decorate(buf: &mut ChunkBuffer, cols: &[Column], stride: usize, fields: &Fields, seed: u64) {
    // Ordre important : les étangs d'abord (ils creusent/inondent le
    // terrain), puis les arbres (tree_site_ok voit le terrain final et
    // refuse un sol creusé ou sablonneux), enfin la végétation basse
    // (elle lit le buffer réel : rien ne pousse au-dessus de l'eau).
    for scx in buf.cx - 1..=buf.cx + 1 {
        for scz in buf.cz - 1..=buf.cz + 1 {
            place_ponds_from_source(buf, fields, seed, scx, scz);
        }
    }
    for scx in buf.cx - 1..=buf.cx + 1 {
        for scz in buf.cz - 1..=buf.cz + 1 {
            place_trees_from_source(buf, fields, seed, scx, scz);
        }
    }
    place_vegetation(buf, cols, stride, seed);
    // Sources d'eau/lave et cascades, en tout dernier (elles voient le
    // terrain décoré final).
    super::liquids::decorate_liquids(buf, fields, seed);
}

// ---------------------------------------------------------------------
// Arbres
// ---------------------------------------------------------------------

fn place_trees_from_source(buf: &mut ChunkBuffer, fields: &Fields, seed: u64, scx: i32, scz: i32) {
    let mut rng = Rng::new(hash_coords(seed ^ SALT_TREE, scx, scz, 0x51));
    // Grille 4x4 de cellules : une tentative d'arbre par cellule,
    // position jitterée -> espacement naturel sans chevauchements massifs.
    for cell in 0..16u32 {
        let tx = scx * 16 + ((cell % 4) as i32) * 4 + rng.range_i32(0, 3);
        let tz = scz * 16 + ((cell / 4) as i32) * 4 + rng.range_i32(0, 3);
        let col = sample_column(fields, tx, tz);
        let spec = tree_spec(col.biome);
        if spec.attempts == 0 {
            continue;
        }
        let p_cell = (spec.attempts as f64 * spec.probability / 16.0).min(1.0);
        if !rng.chance(p_cell) {
            continue;
        }
        if !tree_site_ok(buf, col) {
            continue;
        }
        let kind = pick_kind(&mut rng, spec);
        build_tree(buf, kind, tx, col.height, tz, seed);
    }
}

fn tree_site_ok(buf: &ChunkBuffer, col: Column) -> bool {
    match col.biome {
        Biome::Ocean | Biome::DeepOcean | Biome::River | Biome::Beach => return false,
        _ => {}
    }
    if col.height <= SEA_LEVEL || col.height > 200 {
        return false;
    }
    // Si la colonne est dans ce chunk, vérifier que le sol existe encore
    // (une grotte a pu creuser l'entrée exactement là, ou un étang
    // inonder / sabler la zone).
    if let Some(cur) = buf.get_world(col.x, col.height, col.z) {
        let id = cur & 0xFFF;
        if id == 0 || id == 9 || id == 11 || id == 12 {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy)]
enum TreeKind {
    Oak,
    LargeOak,
    Birch,
    Spruce,
    Pine,
    JungleSmall,
    JungleBig,
    SwampOak,
    DarkOak,
    Acacia,
}

fn pick_kind(rng: &mut Rng, spec: TreeSpec) -> TreeKind {
    let total = spec.oak
        + spec.birch
        + spec.spruce
        + spec.jungle_small
        + spec.jungle_big
        + spec.swamp_oak
        + spec.acacia;
    let mut roll = rng.next_f64() * total.max(f64::EPSILON);
    if roll < spec.oak {
        return if rng.chance(0.15) {
            TreeKind::LargeOak
        } else {
            TreeKind::Oak
        };
    }
    roll -= spec.oak;
    if roll < spec.birch {
        return TreeKind::Birch;
    }
    roll -= spec.birch;
    if roll < spec.spruce {
        return if rng.chance(0.35) {
            TreeKind::Pine
        } else {
            TreeKind::Spruce
        };
    }
    roll -= spec.spruce;
    if roll < spec.jungle_small {
        return TreeKind::JungleSmall;
    }
    roll -= spec.jungle_small;
    if roll < spec.jungle_big {
        return TreeKind::JungleBig;
    }
    roll -= spec.jungle_big;
    if roll < spec.swamp_oak {
        return if rng.chance(0.30) {
            TreeKind::DarkOak
        } else {
            TreeKind::SwampOak
        };
    }
    TreeKind::Acacia
}

/// Décision de "taille de coin" déterministe par coordonnées absolues :
/// identique quel que soit le chunk qui rend la feuille.
#[inline]
fn trim(x: i32, y: i32, z: i32, p: f64) -> bool {
    let h = hash3(SALT_SHAPE, x, y, z, 0xC0DE);
    ((h >> 11) as f64) / ((1u64 << 53) as f64) < p
}

fn build_tree(buf: &mut ChunkBuffer, kind: TreeKind, x: i32, ground: i32, z: i32, seed: u64) {
    let mut rng = Rng::new(hash_coords(seed ^ SALT_SHAPE, x, z, 0x22));
    match kind {
        TreeKind::Oak => oak_like(buf, x, ground, z, bm(17, 0), bm(18, 0), 4 + rng.range_i32(0, 2)),
        TreeKind::LargeOak => large_oak(buf, x, ground, z, 6 + rng.range_i32(0, 3), seed),
        TreeKind::Birch => oak_like(buf, x, ground, z, bm(17, 2), bm(18, 2), 5 + rng.range_i32(0, 2)),
        TreeKind::Spruce => spruce(buf, x, ground, z, 6 + rng.range_i32(0, 3)),
        TreeKind::Pine => pine(buf, x, ground, z, 7 + rng.range_i32(0, 3)),
        TreeKind::JungleSmall => jungle_small(buf, x, ground, z, 5 + rng.range_i32(0, 2)),
        TreeKind::JungleBig => {
            let th = 8 + rng.range_i32(0, 4);
            jungle_big(buf, x, ground, z, th);
        }
        TreeKind::SwampOak => swamp_oak(buf, x, ground, z, 5 + rng.range_i32(0, 1)),
        TreeKind::DarkOak => dark_oak(buf, x, ground, z, 6),
        TreeKind::Acacia => acacia(buf, x, ground, z, 4 + rng.range_i32(0, 2)),
    }
}

#[inline]
fn put_leaf(buf: &mut ChunkBuffer, x: i32, y: i32, z: i32, leaf: u16) {
    if let Some(cur) = buf.get_world(x, y, z) {
        if cur != 0 {
            return; // ne remplace ni le tronc ni le terrain
        }
    } else {
        return; // hors de ce chunk
    }
    buf.set_world(x, y, z, leaf);
}

fn leaf_with_trim(buf: &mut ChunkBuffer, x: i32, y: i32, z: i32, leaf: u16, is_corner: bool) {
    if is_corner && trim(x, y, z, 0.5) {
        return;
    }
    put_leaf(buf, x, y, z, leaf);
}

/// Chêne / bouleau : tronc droit + canopée sphérique classique.
fn oak_like(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, log: u16, leaf: u16, th: i32) {
    let top = gy + th;
    for dy in [-2i32, -1] {
        let y = top + dy;
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                let corner = dx.abs() == 2 && dz.abs() == 2;
                leaf_with_trim(buf, x + dx, y, z + dz, leaf, corner);
            }
        }
    }
    for dx in -1..=1i32 {
        for dz in -1..=1i32 {
            put_leaf(buf, x + dx, top, z + dz, leaf);
        }
    }
    for (dx, dz) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
        put_leaf(buf, x + dx, top + 1, z + dz, leaf);
    }
    put_leaf(buf, x, top + 1, z, leaf);

    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
}

/// Épicéa : cône à étages alternés.
fn spruce(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32) {
    let log = bm(17, 1);
    let leaf = bm(18, 1);
    let top = gy + th;

    put_leaf(buf, x, top + 1, z, leaf);
    for (dx, dz) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
        put_leaf(buf, x + dx, top, z + dz, leaf);
    }
    put_leaf(buf, x, top, z, leaf);

    let mut layer = top - 1;
    let mut radius = 1i32;
    while layer > gy + 2 {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let corner = dx.abs() == radius && dz.abs() == radius;
                leaf_with_trim(buf, x + dx, layer, z + dz, leaf, corner && radius >= 2);
            }
        }
        layer -= 2;
        radius = if radius == 2 { 1 } else { 2 };
    }

    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
}

/// Petit jungle : canopée ronde dense.
fn jungle_small(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32) {
    let log = bm(17, 3);
    let leaf = bm(18, 3);
    let top = gy + th;
    for dy in [-1i32, 0] {
        let y = top + dy;
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                let corner = dx.abs() == 2 && dz.abs() == 2;
                leaf_with_trim(buf, x + dx, y, z + dz, leaf, corner);
            }
        }
    }
    for (dx, dz) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
        put_leaf(buf, x + dx, top + 1, z + dz, leaf);
    }
    put_leaf(buf, x, top + 1, z, leaf);
    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
}

/// Grand jungle : tronc haut + large canopée + lianes sur le tronc.
fn jungle_big(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32) {
    jungle_small(buf, x, gy, z, th);
    let log = bm(17, 3);
    let leaf = bm(18, 3);
    let top = gy + th;
    // Étage intermédiaire pour donner de la masse.
    let mid = gy + th * 2 / 3;
    for dx in -1..=1i32 {
        for dz in -1..=1i32 {
            put_leaf(buf, x + dx, mid + 1, z + dz, leaf);
        }
    }
    // Canopée élargie au sommet.
    for dx in -3..=3i32 {
        for dz in -3..=3i32 {
            let d2 = (dx * dx + dz * dz) as f64;
            if d2 <= 10.0 {
                put_leaf(buf, x + dx, top, z + dz, leaf);
            }
        }
    }
    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
    vines_on_trunk(buf, x, gy + 2, z, top - 1, 6);
}

/// Chêne de marais : tronc court, canopée large et plate, lianes.
fn swamp_oak(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32) {
    let log = bm(17, 0);
    let leaf = bm(18, 0);
    let top = gy + th;
    for dy in [-1i32, 0] {
        let y = top + dy;
        for dx in -3..=3i32 {
            for dz in -3..=3i32 {
                let d2 = (dx * dx + dz * dz) as f64;
                if d2 <= 9.5 {
                    let corner = dx.abs() == 3 && dz.abs() == 3;
                    leaf_with_trim(buf, x + dx, y, z + dz, leaf, corner);
                }
            }
        }
    }
    for (dx, dz) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
        put_leaf(buf, x + dx, top + 1, z + dz, leaf);
    }
    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
    // Lianes pendantes sous la canopée.
    for (dx, dz) in [(-3i32, 0), (3, 0), (0, -3), (0, 3), (-2, -2), (2, 2), (-2, 2), (2, -2)] {
        let len = if trim(x + dx, top, z + dz, 0.6) { 3 } else { 2 };
        for k in 2..=len {
            put_leaf(buf, x + dx, top - k, z + dz, bm(106, 0));
        }
    }
}

/// Lianes accrochées aux flancs du tronc (jungle).
fn vines_on_trunk(buf: &mut ChunkBuffer, x: i32, y_from: i32, z: i32, y_to: i32, count: u32) {
    // Directions : (dx, dz, bit d'attachement vers le tronc).
    const SIDES: [(i32, i32, u16); 4] = [
        (1, 0, 2),   // liane à l'est -> attachée ouest
        (-1, 0, 8),  // à l'ouest -> est
        (0, 1, 4),   // au sud -> nord
        (0, -1, 1),  // au nord -> sud
    ];
    let span = (y_to - y_from).max(1);
    for i in 0..count {
        let t = i as i32;
        let side = SIDES[(t % 4) as usize];
        let y = y_from + (t * 7 % span);
        put_leaf(buf, x + side.0, y, z + side.1, bm(106, side.2 as u8));
    }
}

/// Grand chêne : gros tronc à deux blocs, branche maîtresse déportée,
/// canopée dense sur les deux couronnes.
fn large_oak(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32, seed: u64) {
    let log = bm(17, 0);
    let leaf = bm(18, 0);
    // Direction de la branche principale, déterministe par position.
    let mut rng = Rng::new(hash_coords(seed ^ SALT_SHAPE, x ^ 0x5F, z, 0x77));
    let dirs: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let (ddx, ddz) = dirs[rng.range_i32(0, 3) as usize];
    let split = gy + th / 2;
    let canopy_y = gy + th;

    // Tronc épais (2 blocs) de la base jusqu'au split.
    for y in (gy + 1)..=split {
        buf.set_world(x, y, z, log);
        buf.set_world(x + ddx, y, z + ddz, log);
    }
    // Branche horizontale puis remontée.
    let elbow = split + 2;
    let bx = x + ddx * 2;
    let bz = z + ddz * 2;
    for y in (split + 1)..=elbow {
        buf.set_world(x, y, z, log);
    }
    buf.set_world(bx, elbow, bz, log);
    buf.set_world(bx, elbow + 1, bz, log);
    for y in (elbow + 2)..=canopy_y {
        buf.set_world(bx, y, bz, log);
    }

    // Canopée sur la couronne principale (au bout de la branche).
    for dy in [-1i32, 0] {
        let y = canopy_y + dy;
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                let corner = dx.abs() == 2 && dz.abs() == 2;
                leaf_with_trim(buf, bx + dx, y, bz + dz, leaf, corner);
            }
        }
    }
    for (dx, dz) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
        put_leaf(buf, bx + dx, canopy_y + 1, bz + dz, leaf);
    }
    put_leaf(buf, bx, canopy_y + 1, bz, leaf);
    // Couronne secondaire au sommet du tronc vertical.
    for y in (split + 1)..=(elbow + 1) {
        for dx in -1..=1i32 {
            for dz in -1..=1i32 {
                if (dx != 0 || dz != 0) && y == elbow {
                    put_leaf(buf, x + dx, y, z + dz, leaf);
                }
            }
        }
    }
}

/// Pin : tronc long et raide, aiguilles clairsemées en étages.
fn pine(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32) {
    let log = bm(17, 1);
    let leaf = bm(18, 1);
    let top = gy + th;
    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
    // Étages d'aiguilles à intervalle régulier, de plus en plus petits.
    let mut layer = top - 1;
    let mut radius = 1i32;
    let mut step = 0;
    while layer > gy + 1 {
        if step % 2 == 0 {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if dx.abs() == radius || dz.abs() == radius {
                        let corner = dx.abs() == radius && dz.abs() == radius;
                        leaf_with_trim(buf, x + dx, layer, z + dz, leaf, corner);
                    }
                }
            }
        }
        layer -= 2;
        radius = if layer > gy + 6 { 2 } else { 1 };
        step += 1;
    }
    put_leaf(buf, x, top + 1, z, leaf);
}

/// Chêne noir : tronc très court, large canopée sombre sur 2 niveaux.
fn dark_oak(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, thi: i32) {
    let log = bm(162, 0);
    let leaf = bm(161, 0);
    // 2x2 couronne de troncs bas.
    for y in (gy + 1)..=(gy + thi) {
        buf.set_world(x, y, z, log);
        buf.set_world(x + 1, y, z, log);
        buf.set_world(x, y, z + 1, log);
        buf.set_world(x + 1, y, z + 1, log);
    }
    let top = gy + thi;
    for dy in [-1i32, 0] {
        let y = top + dy;
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                let d2 = (dx * dx + dz * dz) as f64;
                if d2 <= 5.5 {
                    let corner = dx.abs() == 2 && dz.abs() == 2;
                    leaf_with_trim(buf, x + 1 + dx, y, z + 1 + dz, leaf, corner);
                }
            }
        }
    }
    put_leaf(buf, x + 1, top + 1, z + 1, leaf);
}

/// Acacia : tronc fin, canopée plate en coupole sur un tronc légèrement
/// déporté en haut.
fn acacia(buf: &mut ChunkBuffer, x: i32, gy: i32, z: i32, th: i32) {
    let log = bm(162, 0);
    let leaf = bm(161, 0);
    let top = gy + th;
    let lean = if (x + z) % 2 == 0 { 1 } else { 0 };
    let cx = x + lean;
    let cz = z + (1 - lean);
    for y in (gy + 1)..=top {
        buf.set_world(x, y, z, log);
    }
    // Tête déportée.
    buf.set_world(cx, top, cz, log);
    for y in (top-1)..=(top) {
        for dx in -2..=2i32 {
            for dz in -2..=2i32 {
                let d2 = (dx * dx + dz * dz) as f64;
                if d2 <= 3.5 {
                    let corner = dx.abs() == 2 && dz.abs() == 2;
                    leaf_with_trim(buf, cx + dx, y, cz + dz, leaf, corner);
                }
            }
        }
    }
    for (dx, dz) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
        put_leaf(buf, cx + dx, top + 1, cz + dz, leaf);
    }
    put_leaf(buf, cx, top + 1, cz, leaf);
}

// ---------------------------------------------------------------------
// Végétation basse
// ---------------------------------------------------------------------

fn place_vegetation(buf: &mut ChunkBuffer, cols: &[Column], stride: usize, seed: u64) {
    for lx in 0..16usize {
        for lz in 0..16usize {
            let col = cols[(lz + 1) * stride + (lx + 1)];
            let wx = buf.cx * 16 + lx as i32;
            let wz = buf.cz * 16 + lz as i32;
            let mut rng = Rng::new(hash3(seed ^ SALT_VEG, wx, 0, wz, 0x33));

            // Nénuphars sur l'eau des marais.
            if col.biome == Biome::Swamp && col.height < SEA_LEVEL {
                let cur = buf.get(lx, SEA_LEVEL as usize, lz);
                if cur == WATER && rng.chance(params(col.biome).lily_pad) {
                    buf.set(lx, SEA_LEVEL as usize, lz, bm(111, 0));
                }
                continue;
            }

            if col.height <= SEA_LEVEL || col.height > 200 {
                continue;
            }
            let p = params(col.biome);
            if p.snow_cover {
                continue;
            }
            let h = col.height as usize;
            let surface = buf.get(lx, h, lz) & 0xFFF;
            let above = buf.get(lx, h + 1, lz);
            if above != 0 {
                continue;
            }
            let grassy = surface == 2 || surface == 3;
            let sandy = surface == 12;

            if sandy && col.biome == Biome::Desert {
                if rng.chance(p.cactus) {
                    let ch = 1 + rng.range_i32(0, 2);
                    for k in 1..=ch {
                        if buf.get(lx, h + k as usize, lz) == 0 {
                            buf.set(lx, h + k as usize, lz, bm(81, 0));
                        }
                    }
                } else if rng.chance(p.dead_bush) {
                    buf.set(lx, h + 1, lz, bm(32, 0));
                }
                continue;
            }

            if !grassy {
                continue;
            }

            // Canne à sucre : bord immédiat d'un plan d'eau.
            if rng.chance(p.sugar_cane) && has_adjacent_water(cols, stride, lx, lz) {
                let ch = 2 + rng.range_i32(0, 1);
                for k in 1..=ch {
                    if buf.get(lx, h + k as usize, lz) == 0 {
                        buf.set(lx, h + k as usize, lz, bm(83, 0));
                    }
                }
                continue;
            }

            if rng.chance(p.tall_grass) {
                buf.set(lx, h + 1, lz, bm(31, 1));
                continue;
            }
            if rng.chance(p.ferns) {
                buf.set(lx, h + 1, lz, bm(31, 2));
                continue;
            }
            if rng.chance(p.flowers) {
                let flower = if rng.chance(0.5) { b(37) } else { b(38) };
                buf.set(lx, h + 1, lz, flower);
                continue;
            }
            if rng.chance(p.mushrooms) {
                let shroom = if rng.chance(0.5) { b(39) } else { b(40) };
                buf.set(lx, h + 1, lz, shroom);
                continue;
            }
            if rng.chance(p.pumpkins) {
                buf.set(lx, h + 1, lz, bm(86, 0));
                continue;
            }
            if rng.chance(p.melons) {
                buf.set(lx, h + 1, lz, bm(103, 0));
                continue;
            }
        }
    }
}

/// Vrai si une colonne voisine (grille étendue ±1) est sous le niveau de
/// la mer — condition approximative mais robuste pour la canne à sucre.
fn has_adjacent_water(cols: &[Column], stride: usize, lx: usize, lz: usize) -> bool {
    for dx in -1i32..=1 {
        for dz in -1i32..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let nx = lx as i32 + dx;
            let nz = lz as i32 + dz;
            if !(0..16).contains(&nx) || !(0..16).contains(&nz) {
                continue;
            }
            let c = cols[((nz + 1) as usize) * stride + ((nx + 1) as usize)];
            if c.height < SEA_LEVEL {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------
// Étangs
// ---------------------------------------------------------------------

fn place_ponds_from_source(buf: &mut ChunkBuffer, fields: &Fields, seed: u64, scx: i32, scz: i32) {
    let mut rng = Rng::new(hash_coords(seed ^ SALT_POND, scx, scz, 0xB0A));
    if !rng.chance(0.03) {
        return;
    }
    let px = scx * 16 + rng.range_i32(2, 13);
    let pz = scz * 16 + rng.range_i32(2, 13);
    let center = sample_column(fields, px, pz);
    if !params(center.biome).ponds {
        return;
    }
    let rx = 2.2 + rng.next_f64() * 1.8;
    let rz = 2.2 + rng.next_f64() * 1.8;

    // Validation : zone plate, loin de l'eau libre, biome accueillant.
    let mut min_h = i32::MAX;
    let mut max_h = i32::MIN;
    for dx in -4..=4i32 {
        for dz in -4..=4i32 {
            let c = sample_column(fields, px + dx, pz + dz);
            if c.height <= SEA_LEVEL + 1 {
                return;
            }
            match c.biome {
                Biome::River | Biome::Beach | Biome::Ocean | Biome::DeepOcean | Biome::Mountains => return,
                _ => {}
            }
            min_h = min_h.min(c.height);
            max_h = max_h.max(c.height);
        }
    }
    if max_h - min_h > 3 {
        return;
    }
    let wl = min_h;
    let clay_bottom = center.clay > 0.25;

    for dx in -4..=4i32 {
        for dz in -4..=4i32 {
            let d = (dx as f64 / rx).powi(2) + (dz as f64 / rz).powi(2);
            if d > 1.35 {
                continue;
            }
            let c = sample_column(fields, px + dx, pz + dz);
            let hh = c.height;
            if d <= 1.0 {
                let depth = if d < 0.45 { 2 } else { 1 };
                // Fond borné : jamais plus de 2 blocs sous le plan d'eau,
                // et toujours au moins 1 bloc d'eau (floor < wl).
                let floor = ((hh - depth).max(wl - 2)).min(wl - 1);
                buf.set_world(px + dx, floor, pz + dz, if clay_bottom { b(82) } else { b(3) });
                // Eau jusqu'au niveau UNIFORME wl : la surface du plan
                // d'eau est plane même en pente.
                for y in (floor + 1)..=wl {
                    buf.set_world(px + dx, y, pz + dz, WATER);
                }
                // Creuse l'air au-dessus du plan d'eau (berge creusée).
                for y in (wl + 1)..=hh {
                    buf.set_world(px + dx, y, pz + dz, 0);
                }
            } else {
                // Bourrelet sableux autour du plan d'eau.
                buf.set_world(px + dx, hh, pz + dz, b(12));
            }
        }
    }
}
