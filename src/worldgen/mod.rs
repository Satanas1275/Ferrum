//! Génération procédurale de l'Overworld (style 1.7.10, implémentation
//! propre à Ferrum).
//!
//! Pipeline par chunk :
//!
//! ```text
//! seed -> champs de bruit (climat)
//!      -> échantillonnage des colonnes (18x18 étendu)
//!      -> socle (bedrock / pierre / eau)
//!      -> surface (biomes, plages, neige, glace)
//!      -> grottes (champ de densité 3D)
//!      -> ravines
//!      -> minerais
//!      -> features (arbres, végétation, étangs)
//!      -> buffer prêt à fusionner dans la map monde
//! ```
//!
//! Garanties : tout est fonction pure du (seed, cx, cz). Aucun état
//! global mutable, aucun RNG partagé — la génération est donc
//! déterministe, parallèle et indépendante de l'ordre.

pub mod biomes;
pub mod buffer;
pub mod carvers;
pub mod climate;
pub mod features;
pub mod liquids;
pub mod noise;
pub mod ores;
pub mod rng;
pub mod surface;
pub mod terrain;

use buffer::ChunkBuffer;
use climate::{sample_column, Column, Fields};
use terrain::CaveFields;

const COLS_STRIDE: usize = 18;

pub struct WorldGenerator {
    pub seed: u64,
    fields: Fields,
    caves: CaveFields,
}

impl WorldGenerator {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            fields: Fields::new(seed),
            caves: CaveFields::new(seed),
        }
    }

    /// Génère le chunk (cx, cz) complet. Pur et thread-safe.
    pub fn generate_chunk(&self, cx: i32, cz: i32) -> ChunkBuffer {
        let mut cols = Vec::with_capacity(COLS_STRIDE * COLS_STRIDE);
        for lz in -1..17i32 {
            for lx in -1..17i32 {
                cols.push(sample_column(&self.fields, cx * 16 + lx, cz * 16 + lz));
            }
        }

        let mut buf = ChunkBuffer::new(cx, cz);
        // Biomes réels par colonne (grille intérieure 16x16) : le client
        // les utilise pour l'herbe colorée, la pluie et F3.
        for lz in 0..16usize {
            for lx in 0..16usize {
                let col = cols[(lz + 1) * COLS_STRIDE + (lx + 1)];
                buf.biomes[lz * 16 + lx] = col.biome.id() as u8;
            }
        }
        terrain::base_fill(&mut buf, &cols, COLS_STRIDE);
        surface::apply_surface(&mut buf, &cols, COLS_STRIDE);
        terrain::carve_caves(&mut buf, &cols, COLS_STRIDE, &self.caves);
        carvers::carve_ravines(&mut buf, &cols, COLS_STRIDE, self.seed);
        ores::generate_ores(&mut buf, &self.fields, self.seed);
        features::decorate(&mut buf, &cols, COLS_STRIDE, &self.fields, self.seed);
        buf
    }

    /// Aperçu d'une colonne sans générer de chunk (recherche de spawn).
    pub fn preview_column(&self, x: i32, z: i32) -> Column {
        sample_column(&self.fields, x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn blocks_of(generator: &WorldGenerator, cx: i32, cz: i32) -> Vec<(i32, i32, i32, u16)> {
        let buf = generator.generate_chunk(cx, cz);
        let mut out = Vec::new();
        for (idx, v) in buf.iter_nonzero() {
            let y = (idx >> 8) as i32;
            let lz = ((idx >> 4) & 15) as i32;
            let lx = (idx & 15) as i32;
            out.push((cx * 16 + lx, y, cz * 16 + lz, v));
        }
        out.sort_unstable();
        out
    }

    #[test]
    fn deterministic_same_seed() {
        let a = WorldGenerator::new(12345);
        let b = WorldGenerator::new(12345);
        assert_eq!(blocks_of(&a, 3, 7), blocks_of(&b, 3, 7));
        assert_eq!(blocks_of(&a, -5, -9), blocks_of(&b, -5, -9));
    }

    #[test]
    fn different_seeds_differ() {
        let a = WorldGenerator::new(1);
        let b = WorldGenerator::new(2);
        assert_ne!(blocks_of(&a, 0, 0), blocks_of(&b, 0, 0));
    }

    #[test]
    fn generation_order_independent() {
        let generator = WorldGenerator::new(777);
        let coords = [(0, 0), (1, 0), (-1, -1)];
        // Ordre A -> B -> C
        let mut forward = Vec::new();
        for &(cx, cz) in &coords {
            forward.push(blocks_of(&generator, cx, cz));
        }
        // Ordre C -> B -> A
        let mut shuffled = Vec::new();
        for &(cx, cz) in coords.iter().rev() {
            shuffled.push(blocks_of(&generator, cx, cz));
        }
        assert_eq!(forward[0], shuffled[2]);
        assert_eq!(forward[1], shuffled[1]);
        assert_eq!(forward[2], shuffled[0]);
    }

    #[test]
    fn parallel_matches_sequential() {
        let coords: Vec<(i32, i32)> = (-4..4).flat_map(|x| (-4..4).map(move |z| (x * 7, z * 11))).collect();
        let sequential: Vec<Vec<(i32, i32, i32, u16)>> = {
            let generator = WorldGenerator::new(2026);
            coords.iter().map(|&(cx, cz)| blocks_of(&generator, cx, cz)).collect()
        };
        let parallel: Vec<Vec<(i32, i32, i32, u16)>> = {
            let gens: Vec<_> = (0..4)
                .map(|_| std::sync::Arc::new(WorldGenerator::new(2026)))
                .collect();
            let coords = std::sync::Arc::new(coords);
            let chunks_per_thread = coords.chunks(coords.len().div_ceil(4));
            let handles: Vec<_> = gens
                .into_iter()
                .zip(chunks_per_thread)
                .map(|(generator, chunk)| {
                    let chunk: Vec<(i32, i32)> = chunk.to_vec();
                    std::thread::spawn(move || {
                        chunk.iter().map(|&(cx, cz)| blocks_of(&generator, cx, cz)).collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).flatten().collect()
        };
        assert_eq!(sequential, parallel);
    }

    #[test]
    fn chunk_content_sane() {
        let generator = WorldGenerator::new(555);
        for &(cx, cz) in &[(0, 0), (-3, 5), (12, -7)] {
            let buf = generator.generate_chunk(cx, cz);
            for lx in 0..16usize {
                for lz in 0..16usize {
                    assert_eq!(buf.get(lx, 0, lz) & 0xFFF, 7, "bedrock manquante ({cx},{cz})");
                }
            }
        }
    }

    fn top_solid(buf: &ChunkBuffer, lx: usize, lz: i32) -> i32 {
        for y in (0..256).rev() {
            if buf.get(lx, y, lz as usize) != 0 {
                return y as i32;
            }
        }
        0
    }

    #[test]
    fn surface_heights_sane() {
        // La surface doit rester dans des bornes plausibles partout
        // (pas de colonne vide, pas de sommet collé au plafond du monde).
        let generator = WorldGenerator::new(999);
        for cx in -2..2i32 {
            for cz in -2..2i32 {
                let buf = generator.generate_chunk(cx, cz);
                for lx in (0..16usize).step_by(4) {
                    for lz in (0..16usize).step_by(4) {
                        let h = top_solid(&buf, lx, lz as i32);
                        assert!((1..=250).contains(&h), "hauteur aberrante {h} ({cx},{cz})");
                    }
                }
            }
        }
    }

    #[test]
    fn world_has_variety() {
        // Sur une zone étendue : océans ET terres, plusieurs biomes,
        // arbres, minerais, grottes.
        let generator = WorldGenerator::new(31337);
        let mut biomes_seen = HashSet::new();
        let mut trees = 0u64;
        let mut ores = 0u64;
        let mut cave_air = 0u64;
        let mut water = 0u64;
        for cx in -8..8i32 {
            for cz in -8..8i32 {
                let col = generator.preview_column(cx * 16 + 8, cz * 16 + 8);
                biomes_seen.insert(col.biome);
                let buf = generator.generate_chunk(cx, cz);
                // Comptage direct via accès dense.
                for y in 0..256usize {
                    for lz in 0..16usize {
                        for lx in 0..16usize {
                            let id = buf.get(lx, y, lz) & 0xFFF;
                            match id {
                                17 | 18 => trees += 1,
                                14 | 15 | 16 | 21 | 56 | 73 | 129 => ores += 1,
                                9 => water += 1,
                                _ => {}
                            }
                        }
                    }
                }
                // Air sous la surface = grottes.
                for y in 20..50usize {
                    for lz in 0..16usize {
                        for lx in 0..16usize {
                            if buf.get(lx, y, lz) == 0 {
                                cave_air += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(biomes_seen.len() >= 4, "trop peu de biomes : {biomes_seen:?}");
        assert!(trees > 100, "pas assez d'arbres : {trees}");
        assert!(ores > 200, "pas assez de minerais : {ores}");
        assert!(water > 100_000, "pas assez d'eau : {water}");
        assert!(cave_air > 500, "pas de grottes détectées : {cave_air}");
    }

    #[test]
    fn no_vegetation_on_water() {
        // Régression : les étangs sont placés AVANT la végétation, donc
        // aucune herbe/fleur ne doit se retrouver au-dessus ou dans l'eau
        // (seul le nénuphar du marais est autorisé sur l'eau).
        let generator = WorldGenerator::new(4242);
        for cx in -6..6i32 {
            for cz in -6..6i32 {
                let buf = generator.generate_chunk(cx, cz);
                for y in 1..255usize {
                    for lz in 0..16usize {
                        for lx in 0..16usize {
                            if buf.get(lx, y, lz) & 0xFFF == 9 {
                                let above = buf.get(lx, y + 1, lz) & 0xFFF;
                                assert!(
                                    !matches!(above, 31 | 32 | 37 | 38 | 39 | 40 | 81 | 83 | 86 | 103),
                                    "végétation ({above}) sur l'eau ({},{y},{})",
                                    cx * 16 + lx as i32,
                                    cz * 16 + lz as i32
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn biome_array_matches_columns() {
        // Le tableau de biomes transmis au client doit refléter le climat.
        let generator = WorldGenerator::new(8080);
        let buf = generator.generate_chunk(2, -3);
        for lz in 0..16usize {
            for lx in 0..16usize {
                let col = generator.preview_column(2 * 16 + lx as i32, -3 * 16 + lz as i32);
                assert_eq!(buf.biomes[lz * 16 + lx], col.biome.id() as u8);
            }
        }
    }

    #[test]
    fn new_biomes_trees_and_liquids_present() {
        use super::biomes::Biome;
        let generator = WorldGenerator::new(90210);
        // Les biomes s'étendent sur de grandes échelles : la savane (rare)
        // se cherche sur une large zone, à coût nul (preview).
        let mut savanna = false;
        for cx in -64..64i32 {
            for cz in -64..64i32 {
                if generator.preview_column(cx * 16 + 8, cz * 16 + 8).biome == Biome::Savanna {
                    savanna = true;
                    break;
                }
            }
            if savanna {
                break;
            }
        }
        assert!(savanna, "savane introuvable sur cette vaste zone");

        // Sources de lave, acacia et chêne noir sur une zone générée.
        // On balaye toute la hauteur : la lave de surface des sommets et
        // les canopées des arbres vivent bien au-dessus de la couche 60.
        let mut lava = false;
        let mut dark_leaf = false;
        let mut acacia_log = false;
        for cx in -16..16i32 {
            for cz in -16..16i32 {
                let buf = generator.generate_chunk(cx, cz);
                for y in 5..170usize {
                    for lz in 0..16usize {
                        for lx in 0..16usize {
                            match buf.get(lx, y, lz) & 0xFFF {
                                11 => lava = true,
                                161 => dark_leaf = true,
                                162 => acacia_log = true,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        assert!(lava, "pas de lave (sources) détectée");
        assert!(acacia_log, "acacia jamais rendu");
        assert!(dark_leaf, "chêne noir jamais rendu");
    }

    #[test]
    #[ignore] // bench : cargo test --release -- --ignored --nocapture
    fn benchmark_generation() {
        let generator = WorldGenerator::new(20260821);
        let n = 25;
        let start = std::time::Instant::now();
        for i in 0..n {
            let cx = (i % 5) as i32 - 2;
            let cz = (i / 5) as i32 - 2;
            let _ = generator.generate_chunk(cx, cz);
        }
        let elapsed = start.elapsed();
        println!("{n} chunks générés en {elapsed:?} ({:.2} ms/chunk)", elapsed.as_secs_f64() * 1000.0 / n as f64);
    }
}
