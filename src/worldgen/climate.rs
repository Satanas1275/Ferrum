//! Climat et sélection des biomes.
//!
//! Chaque colonne du monde est décrite par des canaux de bruit continus
//! (continentalité, érosion, crêtes, température, humidité, rivière...).
//! La hauteur du terrain est une fonction DIRECTE de ces canaux — jamais
//! d'une table de biomes — ce qui garantit une continuité parfaite entre
//! chunks et des transitions douces entre biomes.

use super::biomes::Biome;
use super::noise::{cosine_lerp, smooth_step, Fbm, SimplexFbm, ValueFbm};
use super::rng::Rng;

pub const SEA_LEVEL: i32 = 62;

/// Fréquences (en cycles/bloc) des canaux : le choix de ces valeurs fixe
/// l'échelle visuelle du monde (continents ~2-3 km, collines ~100 m...).
const F_CONTINENTAL: f64 = 1.0 / 1500.0;
const F_EROSION: f64 = 1.0 / 430.0;
const F_RIDGE: f64 = 1.0 / 520.0;
const F_HILLS: f64 = 1.0 / 115.0;
const F_DETAIL: f64 = 1.0 / 38.0;
const F_TEMP: f64 = 1.0 / 1350.0;
const F_HUMID: f64 = 1.0 / 1050.0;
const F_RIVER: f64 = 1.0 / 900.0;
const F_PATCH: f64 = 1.0 / 70.0;
const F_CLAY: f64 = 1.0 / 45.0;

/// Tous les champs de bruit du monde, construits une fois par génération
/// de chunk à partir du seed. Aucun état mutable partagé.
pub struct Fields {
    pub continental: Fbm,
    pub erosion: Fbm,
    pub ridge: Fbm,
    pub hills: Fbm,
    pub detail: Fbm,
    pub temp: Fbm,
    pub humid: Fbm,
    pub river: Fbm,
    /// Texture de surface (sable/gravier/argile) : base douce — value noise.
    pub patch: ValueFbm,
    /// Canal d'argile : base organique irrégulière — simplex.
    pub clay: SimplexFbm,
}

impl Fields {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0x5EED_1A7A_C0DE);
        Self {
            continental: Fbm::new(&mut rng, 4, F_CONTINENTAL, 0.52),
            erosion: Fbm::new(&mut rng, 3, F_EROSION, 0.5),
            ridge: Fbm::new(&mut rng, 3, F_RIDGE, 0.5),
            hills: Fbm::new(&mut rng, 4, F_HILLS, 0.5),
            detail: Fbm::new(&mut rng, 3, F_DETAIL, 0.5),
            temp: Fbm::new(&mut rng, 2, F_TEMP, 0.55),
            humid: Fbm::new(&mut rng, 2, F_HUMID, 0.55),
            river: Fbm::new(&mut rng, 3, F_RIVER, 0.5),
            patch: ValueFbm::new(&mut rng, 2, F_PATCH, 0.5),
            clay: SimplexFbm::new(&mut rng, 2, F_CLAY, 0.5),
        }
    }
}

/// Résultat complet de l'échantillonnage d'une colonne monde.
#[derive(Clone, Copy)]
pub struct Column {
    pub x: i32,
    pub z: i32,
    /// Hauteur finale du terrain (dernier bloc solide).
    pub height: i32,
    /// Hauteur avant creusement des rivières (pour les plages).
    pub base_height: i32,
    pub biome: Biome,
    /// Température effective (altitude froide incluse), ~[-1, 1].
    pub temp: f64,
    /// Humidité, ~[-1, 1].
    pub humid: f64,
    /// Continentalité brute, ~[-1, 1] (négatif = océan).
    pub cont: f64,
    /// Force du courant central de rivière [0, 1].
    pub river_core: f64,
    /// Canal de variation locale (matériaux de fond marin, falaises...).
    pub patch: f64,
    /// Canal d'argile (fonds de rivières/marais).
    pub clay: f64,
    /// Masque montagnes [0, 1].
    pub mountain: f64,
}

impl Column {
    pub fn is_land(self) -> bool {
        self.height >= SEA_LEVEL - 1
    }

    pub fn is_water_column(self) -> bool {
        self.height < SEA_LEVEL
    }
}

/// Profil altimétrique du socle en fonction de la continentalité.
/// Points de contrôle : (-1 = abysse, +1 = hinterland élevé).
const CONT_ANCHORS: [(f64, f64); 9] = [
    (-1.00, 20.0),
    (-0.55, 26.0),
    (-0.32, 35.0),
    (-0.20, 43.0),
    (-0.10, 53.0),
    (0.00, 63.0),
    (0.30, 68.0),
    (0.65, 74.0),
    (1.00, 82.0),
];

fn land_base(cont: f64) -> f64 {
    let anchors = &CONT_ANCHORS;
    if cont <= anchors[0].0 {
        return anchors[0].1;
    }
    for w in anchors.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if cont <= x1 {
            let t = (cont - x0) / (x1 - x0);
            return cosine_lerp(y0, y1, t);
        }
    }
    anchors[anchors.len() - 1].1
}

/// Échantillonne tous les canaux d'une colonne monde. Fonction pure :
/// deux appels avec les mêmes coordonnées donnent strictement le même
/// résultat, où que soit le chunk.
pub fn sample_column(f: &Fields, x: i32, z: i32) -> Column {
    let fx = x as f64;
    let fz = z as f64;

    let cont = f.continental.sample2(fx, fz);
    let erosion = f.erosion.sample2(fx, fz);
    let ridge_n = f.ridge.sample2(fx, fz);
    let hills = f.hills.sample2(fx, fz);
    let detail = f.detail.sample2(fx, fz);
    let patch = f.patch.sample2(fx * 1.7, fz * 1.7);
    let clay = f.clay.sample2(fx, fz);

    // Relief : socle continental + collines (amplitude pilotée par
    // l'érosion) + chaînes montagneuses (crêtes nettes, masquées hors des
    // terres hautes peu érodées).
    let hill_amp = 2.5 + 16.5 * smooth_step(0.45, -0.45, erosion);
    let mountain_mask = smooth_step(0.05, 0.35, cont) * smooth_step(0.30, -0.30, erosion);
    let ridge_v = (1.0 - ridge_n.abs()).max(0.0);
    let mountain_h = ridge_v * ridge_v * ridge_v * 60.0 * mountain_mask;

    let mut height = land_base(cont) + hills * hill_amp + detail * 3.0 + mountain_h;

    // Rivières : bande étroite autour du zéro du champ `river`, avec
    // léger domaine warp pour des méandres organiques.
    let warp_x = f.detail.sample2(fx * 0.11 + 91.7, fz * 0.11 - 33.3) * 55.0;
    let warp_z = f.detail.sample2(fx * 0.11 - 71.1, fz * 0.11 + 12.9) * 55.0;
    let river_n = f.river.sample2(fx + warp_x, fz + warp_z);
    let dist = river_n.abs();
    let river_core = 1.0 - smooth_step(0.018, 0.054, dist);
    let river_valley = 1.0 - smooth_step(0.03, 0.17, dist);

    let base_height = height;
    let land_fade = smooth_step(-0.08, 0.03, cont);
    let target = (SEA_LEVEL - 3) as f64;
    if height > target {
        let k = (river_core * land_fade).powf(1.25);
        height = height + (target - height) * k;
        // Berges légèrement abaissées pour un lit large et naturel.
        height -= river_valley * land_fade * 3.0 * smooth_step(target + 2.0, target + 8.0, base_height);
    }

    let mut temp = f.temp.sample2(fx, fz);
    let humid = f.humid.sample2(fx, fz);

    let height_i = height.round().clamp(6.0, 250.0) as i32;
    // Refroidissement d'altitude : les hauts sommets dépassent la ligne
    // de neige même sous climat tempéré.
    let altitude_cool = ((height_i - 86).max(0)) as f64 * 0.011;
    temp -= altitude_cool;

    let mountain = mountain_mask * ridge_v;
    let biome = select_biome(x, z, height_i, cont, temp, humid, river_core, mountain);

    Column {
        x,
        z,
        height: height_i,
        base_height: base_height.round().clamp(6.0, 250.0) as i32,
        biome,
        temp,
        humid,
        cont,
        river_core,
        patch,
        clay,
        mountain,
    }
}

/// Petite empreinte de hasard déterministe par colonne, pour les biomes
/// rares (îles champignons) qui ne doivent ni dépendre de l'ordre de
/// génération ni créer de frontières artificielles dures.
fn hash_chance(x: i32, z: i32, salt: u64) -> f64 {
    use super::rng::hash3;
    (hash3(0xD0BB_10AA, x, z, 0, salt) >> 11) as f64 / (1u64 << 53) as f64
}

fn select_biome(
    x: i32,
    z: i32,
    height: i32,
    cont: f64,
    temp: f64,
    humid: f64,
    river_core: f64,
    mountain: f64,
) -> Biome {
    use Biome::*;

    // Masse d'eau principale.
    if height < SEA_LEVEL - 1 {
        if cont < -0.05 {
            return if height < 39 { DeepOcean } else { Ocean };
        }
        // Dépression intérieure sous le niveau de la mer : lac/rivière.
        return River;
    }

    // Cours d'eau et berges.
    if river_core > 0.55 && height <= SEA_LEVEL + 1 {
        return River;
    }

    let beach_ok = height <= SEA_LEVEL + 1 && temp > -0.40 && mountain < 0.45;
    if beach_ok {
        // Rare île champignon juste au bord de l'eau (climat doux).
        if temp > 0.05 && hash_chance(x, z, 0x1A57) < 0.0015 {
            return MushroomIsland;
        }
        return Beach;
    }

    if mountain > 0.45 && height > 90 {
        return Mountains;
    }

    if temp < -0.42 {
        return if humid > 0.05 { SnowyTaiga } else { SnowyPlains };
    }

    if temp > 0.38 {
        if humid < -0.18 {
            Desert
        } else if humid > 0.32 {
            Jungle
        } else if humid < 0.12 {
            // Savane : chaud et sec mais pas désert.
            Savanna
        } else {
            Plains
        }
    } else if temp > -0.02 {
        if humid > 0.36 && height < SEA_LEVEL + 5 {
            Swamp
        } else if humid > 0.08 {
            Forest
        } else {
            Plains
        }
    } else if humid > 0.10 {
        Taiga
    } else {
        Plains
    }
}
