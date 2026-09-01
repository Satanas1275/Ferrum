//! Biomes de l'Overworld et leurs paramètres de génération.
//!
//! La table est data-driven : ajuster un biome (densité d'arbres, blocs de
//! surface, végétation...) se fait ici sans toucher au générateur.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Biome {
    DeepOcean,
    Ocean,
    Beach,
    River,
    Plains,
    Forest,
    Taiga,
    SnowyPlains,
    SnowyTaiga,
    Desert,
    Jungle,
    Swamp,
    Mountains,
    MushroomIsland,
    Savanna,
}

impl Biome {
    /// ID de biome côté protocole 1.7.10.
    pub fn id(self) -> i32 {
        match self {
            Biome::DeepOcean => 24,
            Biome::Ocean => 0,
            Biome::Beach => 16,
            Biome::River => 7,
            Biome::Plains => 1,
            Biome::Forest => 4,
            Biome::Taiga => 5,
            Biome::SnowyPlains => 12,
            Biome::SnowyTaiga => 30,
            Biome::Desert => 2,
            Biome::Jungle => 21,
            Biome::Swamp => 6,
            Biome::Mountains => 3,
            Biome::MushroomIsland => 14,
            Biome::Savanna => 35,
        }
    }
}

/// Style d'arbres : nombre de tentatives par chunk, probabilité de réussite
/// et poids relatifs par espèce.
#[derive(Clone, Copy)]
pub struct TreeSpec {
    pub attempts: u32,
    pub probability: f64,
    pub oak: f64,
    pub birch: f64,
    pub spruce: f64,
    pub jungle_small: f64,
    pub jungle_big: f64,
    pub swamp_oak: f64,
    pub acacia: f64,
}

impl TreeSpec {
    pub const NONE: TreeSpec = TreeSpec {
        attempts: 0,
        probability: 0.0,
        oak: 0.0,
        birch: 0.0,
        spruce: 0.0,
        jungle_small: 0.0,
        jungle_big: 0.0,
        swamp_oak: 0.0,
        acacia: 0.0,
    };
}

/// Paramètres de génération d'un biome.
#[derive(Clone, Copy)]
pub struct BiomeParams {
    pub name: &'static str,
    /// Bloc de surface (herbe, sable...).
    pub top: u16,
    /// Bloc sous la surface (dirt, sable...).
    pub filler: u16,
    /// Bloc profond sous le remplissage (grès pour le désert).
    pub deep: u16,
    /// Profondeur moyenne du remplissage.
    pub filler_depth: i32,
    pub trees: TreeSpec,
    pub tall_grass: f64,
    pub ferns: f64,
    pub flowers: f64,
    pub mushrooms: f64,
    pub dead_bush: f64,
    pub cactus: f64,
    pub sugar_cane: f64,
    pub lily_pad: f64,
    pub pumpkins: f64,
    pub melons: f64,
    /// Couche de neige posée sur le sol.
    pub snow_cover: bool,
    /// Surface d'eau gelée (glace).
    pub freeze_water: bool,
    /// Autorise les émeraudes.
    pub emerald: bool,
    /// Autorise les étangs d'eau de surface.
    pub ponds: bool,
}

const fn p(name: &'static str) -> BiomeParams {
    BiomeParams {
        name,
        top: 2,
        filler: 3,
        deep: 1,
        filler_depth: 3,
        trees: TreeSpec::NONE,
        tall_grass: 0.0,
        ferns: 0.0,
        flowers: 0.0,
        mushrooms: 0.0,
        dead_bush: 0.0,
        cactus: 0.0,
        sugar_cane: 0.0,
        lily_pad: 0.0,
        pumpkins: 0.0,
        melons: 0.0,
        snow_cover: false,
        freeze_water: false,
        emerald: false,
        ponds: false,
    }
}

const fn sandy(mut b: BiomeParams) -> BiomeParams {
    b.top = 12;
    b.filler = 12;
    b
}

const fn frozen(mut b: BiomeParams) -> BiomeParams {
    b.snow_cover = true;
    b.freeze_water = true;
    b
}

const fn spruce(b: BiomeParams, attempts: u32, probability: f64) -> BiomeParams {
    BiomeParams { trees: TreeSpec { spruce: 1.0, attempts, probability, ..TreeSpec::NONE }, ..b }
}

// Table centrale des biomes : toute la personnalité visuelle du monde se
// règle sur ces constantes.
pub static PARAMS: [BiomeParams; 15] = [
    // 0 DeepOcean
    BiomeParams { filler_depth: 4, ..sandy(p("deep_ocean")) },
    // 1 Ocean
    BiomeParams { filler_depth: 3, ..sandy(p("ocean")) },
    // 2 Beach
    sandy(p("beach")),
    // 3 River
    BiomeParams { filler_depth: 2, ..sandy(p("river")) },
    // 4 Plains
    BiomeParams {
        trees: TreeSpec { oak: 1.0, attempts: 1, probability: 0.12, ..TreeSpec::NONE },
        tall_grass: 0.10,
        flowers: 0.018,
        pumpkins: 0.0015,
        sugar_cane: 0.06,
        ponds: true,
        ..p("plains")
    },
    // 5 Forest
    BiomeParams {
        trees: TreeSpec { oak: 0.62, birch: 0.38, attempts: 6, probability: 0.75, ..TreeSpec::NONE },
        tall_grass: 0.06,
        flowers: 0.012,
        mushrooms: 0.002,
        pumpkins: 0.0015,
        sugar_cane: 0.05,
        ponds: true,
        ..p("forest")
    },
    // 6 Taiga
    spruce(
        BiomeParams { tall_grass: 0.04, mushrooms: 0.001, sugar_cane: 0.03, ..p("taiga") },
        5,
        0.68,
    ),
    // 7 SnowyPlains
    frozen(BiomeParams {
        trees: TreeSpec { spruce: 1.0, attempts: 1, probability: 0.22, ..TreeSpec::NONE },
        tall_grass: 0.008,
        ..p("snowy_plains")
    }),
    // 8 SnowyTaiga
    frozen(spruce(BiomeParams { tall_grass: 0.02, ..p("snowy_taiga") }, 5, 0.55)),
    // 9 Desert
    BiomeParams {
        dead_bush: 0.012,
        cactus: 0.009,
        filler_depth: 4,
        ..sandy(BiomeParams { deep: 24, ..p("desert") })
    },
    // 10 Jungle
    BiomeParams {
        trees: TreeSpec {
            jungle_small: 0.72,
            jungle_big: 0.28,
            attempts: 7,
            probability: 0.85,
            ..TreeSpec::NONE
        },
        tall_grass: 0.05,
        ferns: 0.03,
        melons: 0.004,
        sugar_cane: 0.06,
        ..p("jungle")
    },
    // 11 Swamp
    BiomeParams {
        trees: TreeSpec { swamp_oak: 1.0, attempts: 3, probability: 0.6, ..TreeSpec::NONE },
        tall_grass: 0.08,
        mushrooms: 0.007,
        lily_pad: 0.06,
        sugar_cane: 0.08,
        ..p("swamp")
    },
    // 12 Mountains
    BiomeParams {
        trees: TreeSpec { spruce: 1.0, attempts: 1, probability: 0.28, ..TreeSpec::NONE },
        tall_grass: 0.03,
        flowers: 0.004,
        emerald: true,
        ..p("mountains")
    },
    // 13 MushroomIsland
    BiomeParams {
        mushrooms: 0.012,
        ..p("mushroom_island")
    },
    // 14 Savanna
    BiomeParams {
        trees: TreeSpec { acacia: 1.0, attempts: 2, probability: 0.55, ..TreeSpec::NONE },
        tall_grass: 0.20,
        ..p("savanna")
    },
];

pub const BIOME_COUNT: usize = 15;

/// Tous les biomes, dans l'ordre de la table PARAMS.
pub const ALL: [Biome; BIOME_COUNT] = [
    Biome::DeepOcean,
    Biome::Ocean,
    Biome::Beach,
    Biome::River,
    Biome::Plains,
    Biome::Forest,
    Biome::Taiga,
    Biome::SnowyPlains,
    Biome::SnowyTaiga,
    Biome::Desert,
    Biome::Jungle,
    Biome::Swamp,
    Biome::Mountains,
    Biome::MushroomIsland,
    Biome::Savanna,
];

pub fn index(biome: Biome) -> usize {
    ALL.iter().position(|&b| b == biome).unwrap_or(0)
}

/// Recherche d'un biome par nom (insensible à la casse, espaces ou
/// underscores acceptés : "snowy plains" == "snowy_plains").
pub fn by_name(name: &str) -> Option<Biome> {
    let norm = name.to_lowercase().replace(' ', "_");
    ALL.iter().copied().find(|&b| params(b).name == norm)
}

/// Nom affichable ("snowy_plains" -> "Snowy Plains").
pub fn display_name(biome: Biome) -> String {
    params(biome).name.split('_').map(|w| {
        let mut c = w.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_lookup_by_name() {
        assert_eq!(by_name("desert"), Some(Biome::Desert));
        assert_eq!(by_name("Desert"), Some(Biome::Desert));
        assert_eq!(by_name("snowy plains"), Some(Biome::SnowyPlains));
        assert_eq!(by_name("snowy_plains"), Some(Biome::SnowyPlains));
        assert_eq!(by_name("mushroom_island"), Some(Biome::MushroomIsland));
        assert_eq!(by_name("savanna"), Some(Biome::Savanna));
        assert_eq!(by_name("nope"), None);
    }

    #[test]
    fn biome_ids_unique_and_valid() {
        let mut seen = std::collections::HashSet::new();
        for &b in &ALL {
            assert!(seen.insert(b.id()), "id dupliqué pour {b:?}");
        }
        assert_eq!(Biome::Savanna.id(), 35);
        assert_eq!(Biome::MushroomIsland.id(), 14);
        assert_eq!(display_name(Biome::SnowyPlains), "Snowy Plains");
    }
}

pub fn params(biome: Biome) -> &'static BiomeParams {
    &PARAMS[index(biome)]
}

/// Raccourci : spécification d'arbres d'un biome.
pub fn tree_spec(biome: Biome) -> TreeSpec {
    params(biome).trees
}
