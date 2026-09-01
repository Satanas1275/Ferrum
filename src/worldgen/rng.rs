//! Générateur pseudo-aléatoire déterministe pour la génération de monde.
//!
//! Aucun état global : chaque flux de randomité est dérivé du seed du monde
//! et des coordonnées concernées, ce qui garantit qu'un chunk produit
//! exactement le même contenu quel que soit l'ordre ou le thread de génération.

/// PRNG basé sur SplitMix64 (rapide, qualité statistique suffisante pour
/// la génération de terrain, sortie reproductible).
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// f64 uniforme dans [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn next_f32(&mut self) -> f32 {
        self.next_f64() as f32
    }

    /// Entier uniforme dans [min, max] inclus.
    pub fn range_i32(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        min + (self.next_u64() % ((max - min + 1) as u64)) as i32
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p
    }
}

/// Hash déterministe de coordonnées + sel -> graine de RNG.
///
/// C'est le mécanisme central qui rend la génération indépendante de
/// l'ordre : deux chunks différents produisent des flux totalement
/// indépendants, et le même chunk retombe toujours sur la même graine.
pub fn hash_coords(seed: u64, x: i32, z: i32, salt: u64) -> u64 {
    hash_combine(seed, (x as i64) as u64, salt).wrapping_add(hash_combine(0x9E3779B9, (z as i64) as u64, salt << 1))
}

pub fn hash3(seed: u64, x: i32, y: i32, z: i32, salt: u64) -> u64 {
    hash_coords(seed ^ hash_combine(salt, y as u64, 0x51ED2701), x, z, salt)
}

fn hash_combine(a: u64, b: u64, c: u64) -> u64 {
    let mut h = a ^ b.wrapping_mul(0xFF51AFD7ED558CCD);
    h ^= c.wrapping_mul(0xC4CEB9FE1A85EC53);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51AFD7ED558CCD);
    h ^= h >> 29;
    h = h.wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^ (h >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn hash_coords_stable() {
        assert_eq!(hash_coords(7, 10, -3, 5), hash_coords(7, 10, -3, 5));
        assert_ne!(hash_coords(7, 10, -3, 5), hash_coords(7, 10, -2, 5));
        assert_ne!(hash_coords(7, 10, -3, 5), hash_coords(8, 10, -3, 5));
    }
}
