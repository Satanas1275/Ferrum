//! Bruit de Perlin classique (2D/3D) + bruit fractal (FBM).
//!
//! Implémentation propre à Ferrum : table de permutation dérivée du seed
//! via le RNG déterministe, gradients fixes, interpolation quintique.
//! Toutes les sorties sont centrées autour de 0 et bornées approximativement
//! dans [-1, 1].

use super::rng::Rng;

#[derive(Clone)]
pub struct Perlin {
    perm: [u8; 512],
}

// Gradients unitaires (norme 1) : garantit |noise2| <= sqrt(2)/2 * 1.42 < 1.01.
const GRAD2_Y: [f64; 8] = [0.0, std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2, 1.0, -1.0, 0.0, std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2];
const GRAD2_X: [f64; 8] = [1.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2, 0.0, 0.0, -1.0, -std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2];

// 12 arêtes du cube : mêmes directions que l'algorithme original de Perlin.
const GRAD3: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, -1.0], [0.0, -1.0, -1.0],
];

impl Perlin {
    pub fn new(rng: &mut Rng) -> Self {
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        // Mélange de Fisher-Yates piloté par le seed du monde.
        for i in (1..256).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Self { perm }
    }

    #[inline]
    fn fade(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    pub fn noise2(&self, x: f64, y: f64) -> f64 {
        let xi = x.floor() as i32;
        let yi = y.floor() as i32;
        let xf = x - xi as f64;
        let yf = y - yi as f64;

        let u = Self::fade(xf);
        let v = Self::fade(yf);

        let p = &self.perm;
        let h2 = |a: i32, b: i32| -> usize {
            p[((p[(a & 255) as usize] as usize).wrapping_add((b & 255) as usize)) & 255] as usize
        };
        let aa = h2(xi, yi) % 8;
        let ba = h2(xi + 1, yi) % 8;
        let ab = h2(xi, yi + 1) % 8;
        let bb = h2(xi + 1, yi + 1) % 8;

        let d1 = GRAD2_X[aa] * xf + GRAD2_Y[aa] * yf;
        let d2 = GRAD2_X[ba] * (xf - 1.0) + GRAD2_Y[ba] * yf;
        let d3 = GRAD2_X[ab] * xf + GRAD2_Y[ab] * (yf - 1.0);
        let d4 = GRAD2_X[bb] * (xf - 1.0) + GRAD2_Y[bb] * (yf - 1.0);

        lerp(lerp(d1, d2, u), lerp(d3, d4, u), v) * 1.42
    }

    pub fn noise3(&self, x: f64, y: f64, z: f64) -> f64 {
        let xi = x.floor() as i32;
        let yi = y.floor() as i32;
        let zi = z.floor() as i32;
        let xf = x - xi as f64;
        let yf = y - yi as f64;
        let zf = z - zi as f64;

        let u = Self::fade(xf);
        let v = Self::fade(yf);
        let w = Self::fade(zf);

        let p = &self.perm;
        let g = |hx: i32, hy: i32, hz: i32| -> usize {
            let a = (p[(hx & 255) as usize] as usize).wrapping_add((hy & 255) as usize) & 255;
            let b = (p[a] as usize).wrapping_add((hz & 255) as usize) & 255;
            p[b] as usize % 12
        };

        let aaa = g(xi, yi, zi);
        let baa = g(xi + 1, yi, zi);
        let aba = g(xi, yi + 1, zi);
        let bba = g(xi + 1, yi + 1, zi);
        let aab = g(xi, yi, zi + 1);
        let bab = g(xi + 1, yi, zi + 1);
        let abb = g(xi, yi + 1, zi + 1);
        let bbb = g(xi + 1, yi + 1, zi + 1);

        let dot3 = |gi: usize, dx: f64, dy: f64, dz: f64| -> f64 {
            GRAD3[gi][0] * dx + GRAD3[gi][1] * dy + GRAD3[gi][2] * dz
        };

        let x1 = lerp(dot3(aaa, xf, yf, zf), dot3(baa, xf - 1.0, yf, zf), u);
        let x2 = lerp(dot3(aba, xf, yf - 1.0, zf), dot3(bba, xf - 1.0, yf - 1.0, zf), u);
        let y1 = lerp(x1, x2, v);
        let x3 = lerp(dot3(aab, xf, yf, zf - 1.0), dot3(bab, xf - 1.0, yf, zf - 1.0), u);
        let x4 = lerp(dot3(abb, xf, yf - 1.0, zf - 1.0), dot3(bbb, xf - 1.0, yf - 1.0, zf - 1.0), u);
        let y2 = lerp(x3, x4, v);

        lerp(y1, y2, w) * 1.1
    }
}

/// Somme d'octaves de bruit de Perlin (fractal Brownian motion).
#[derive(Clone)]
pub struct Fbm {
    octaves: Vec<Perlin>,
    base_freq: f64,
    persistence: f64,
    norm: f64,
}

impl Fbm {
    pub fn new(rng: &mut Rng, octaves: usize, base_freq: f64, persistence: f64) -> Self {
        let mut layers = Vec::with_capacity(octaves);
        for _ in 0..octaves {
            layers.push(Perlin::new(rng));
        }
        let mut norm = 0.0;
        let mut amp = 1.0;
        for _ in 0..octaves {
            norm += amp;
            amp *= persistence;
        }
        Self { octaves: layers, base_freq, persistence, norm }
    }

    /// Échantillon 2D normalisé (~[-1, 1]).
    pub fn sample2(&self, x: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut freq = self.base_freq;
        for o in &self.octaves {
            sum += o.noise2(x * freq, z * freq) * amp;
            amp *= self.persistence;
            freq *= 2.0;
        }
        sum / self.norm
    }

    /// Échantillon 3D normalisé (~[-1, 1]). `vscale` écrase verticalement
    /// les structures (> 1 => features plus larges que hautes).
    pub fn sample3(&self, x: f64, y: f64, z: f64, vscale: f64) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut freq = self.base_freq;
        for o in &self.octaves {
            sum += o.noise3(x * freq, y * freq * vscale, z * freq) * amp;
            amp *= self.persistence;
            freq *= 2.0;
        }
        sum / self.norm
    }
}

/// Somme d'octaves de bruit de valeur (value noise) : même structure que
/// `Fbm` mais base plus douce, utilisée pour les textures organiques.
#[derive(Clone)]
pub struct ValueFbm {
    octaves: Vec<ValueNoise>,
    base_freq: f64,
    persistence: f64,
    norm: f64,
}

impl ValueFbm {
    pub fn new(rng: &mut Rng, octaves: usize, base_freq: f64, persistence: f64) -> Self {
        let mut layers = Vec::with_capacity(octaves);
        for _ in 0..octaves {
            layers.push(ValueNoise::new(rng));
        }
        let mut norm = 0.0;
        let mut amp = 1.0;
        for _ in 0..octaves {
            norm += amp;
            amp *= persistence;
        }
        Self { octaves: layers, base_freq, persistence, norm }
    }

    /// Échantillon 2D normalisé (~[-1, 1]).
    pub fn sample2(&self, x: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut freq = self.base_freq;
        for o in &self.octaves {
            sum += o.noise2(x * freq, z * freq) * amp;
            amp *= self.persistence;
            freq *= 2.0;
        }
        sum / self.norm
    }
}

/// Somme d'octaves de bruit simplex 2D : motif organique triangulé, un peu
/// plus irrégulier que le perlin mais très économique par octave.
#[derive(Clone)]
pub struct SimplexFbm {
    octaves: Vec<Simplex2D>,
    base_freq: f64,
    persistence: f64,
    norm: f64,
}

impl SimplexFbm {
    pub fn new(rng: &mut Rng, octaves: usize, base_freq: f64, persistence: f64) -> Self {
        let mut layers = Vec::with_capacity(octaves);
        for _ in 0..octaves {
            layers.push(Simplex2D::new(rng));
        }
        let mut norm = 0.0;
        let mut amp = 1.0;
        for _ in 0..octaves {
            norm += amp;
            amp *= persistence;
        }
        Self { octaves: layers, base_freq, persistence, norm }
    }

    /// Échantillon 2D normalisé (~[-1, 1]).
    pub fn sample2(&self, x: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut freq = self.base_freq;
        for o in &self.octaves {
            sum += o.noise2(x * freq, z * freq) * amp;
            amp *= self.persistence;
            freq *= 2.0;
        }
        sum / self.norm
    }
}

#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Interpolation cosinus entre deux valeurs (plus douce que linéaire).
#[inline]
pub fn cosine_lerp(a: f64, b: f64, t: f64) -> f64 {
    let t = (1.0 - (t.clamp(0.0, 1.0)) * std::f64::consts::PI).cos() * 0.5 + 0.5;
    a + (b - a) * t
}

/// Smoothstep supportant des bornes inversées (a > b donne une décroissance).
#[inline]
pub fn smooth_step(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bruit de valeur (value noise) : valeurs pseudo-aléatoires sur une
/// grille entière, interpolées de façon lisse (quintique + cosinus). Plus
/// doux et moins "cristallin" que le Perlin : utilisé pour des variations
/// très organiques.
#[derive(Clone)]
pub struct ValueNoise {
    perm: [u8; 512],
}

impl ValueNoise {
    pub fn new(rng: &mut Rng) -> Self {
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for i in (1..256).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Self { perm }
    }

    /// Paire déterministe de valeurs dans [-1, 1] à partir de coordonnées
    /// de grille. Entièrement en arithmétique saturante/wrapping : aucun
    /// dépassement ni dépendance à l'ordre.
    #[inline]
    fn hashv(&self, ix: i32, iy: i32) -> f64 {
        let ixu = ix as u32;
        let iyu = iy as u32;
        let h = (self.perm[(ixu as usize) & 255] as u64)
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add((iyu as u64).wrapping_mul(0xBF58476D1CE4E5B9))
            .wrapping_add(0x6A09E667F3BCC909);
        let mut z = h;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z ^= z >> 31;
        (z & 0xFFFF) as f64 / 65535.0_f64 * 2.0 - 1.0
    }

    pub fn noise2(&self, x: f64, y: f64) -> f64 {
        let xi = x.floor() as i32;
        let yi = y.floor() as i32;
        let xf = x - xi as f64;
        let yf = y - yi as f64;
        let u = Self::fade(xf);
        let v = Self::fade(yf);

        let g = |ix: i32, iy: i32| -> f64 { self.hashv(ix, iy) };
        let a = g(xi, yi);
        let b = g(xi + 1, yi);
        let c = g(xi, yi + 1);
        let d = g(xi + 1, yi + 1);

        lerp(lerp(a, b, u), lerp(c, d, u), v)
    }

    fn fade(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }
}

/// Bruit Simplex 2D : décomposition en triangles plutôt qu'en carrés, ce
/// qui produit un motif plus organique et un coût plus faible par octave
/// que le Perlin. Sorties bornées approximativement dans [-1, 1].
#[derive(Clone)]
pub struct Simplex2D {
    perm: [u8; 512],
}

impl Simplex2D {
    pub fn new(rng: &mut Rng) -> Self {
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for i in (1..256).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Self { perm }
    }

    // Gradients 2D unitaires (8 directions).
    const GRAD: [(f64, f64); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
        (-std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2),
        (std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2),
        (-std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2),
    ];

    pub fn noise2(&self, xin: f64, yin: f64) -> f64 {
        const F2: f64 = 0.3660254037844386; // (sqrt(3)-1)/2
        const G2: f64 = 0.21132486540518713; // (3-sqrt(3))/6

        let s = (xin + yin) * F2;
        let i = (xin + s).floor() as i32;
        let j = (yin + s).floor() as i32;
        let t = (i + j) as f64 * G2;
        let x0 = xin - (i as f64 - t);
        let y0 = yin - (j as f64 - t);

        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };

        let x1 = x0 - i1 as f64 + G2;
        let y1 = y0 - j1 as f64 + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;

        let ii = (i as usize) & 255;
        let jj = (j as usize) & 255;

        // Gradient d'un coin de la grille, hachage canonique de Perlin
        // (deux sous-niveaux de permutation) : garanti IDENTIQUE entre
        // triangles adjacents -> continuité.
        let gidx = |x: i32, y: i32| -> usize {
            let xb = (ii.wrapping_add(x as usize)) & 255;
            let yb = (jj.wrapping_add(y as usize)) & 255;
            let p1 = self.perm[yb];
            self.perm[(xb).wrapping_add(p1 as usize) & 255] as usize & 7
        };

        let contribution = |x: f64, y: f64, gi: usize| -> f64 {
            let t2 = 0.5 - x * x - y * y;
            if t2 < 0.0 {
                return 0.0;
            }
            let (gx, gy) = Self::GRAD[gi];
            let t4 = t2 * t2;
            t4 * t4 * (gx * x + gy * y)
        };

        let n0 = contribution(x0, y0, gidx(0, 0));
        let n1 = contribution(x1, y1, gidx(i1, j1));
        let n2 = contribution(x2, y2, gidx(1, 1));

        // Normalisation empirique (gradients unitaires) : borne, comme le
        // Perlin, dans ~[-1, 1].
        70.0 * (n0 + n1 + n2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_deterministic_and_bounded() {
        let mut rng = Rng::new(99);
        let p1 = Perlin::new(&mut rng);
        let mut rng = Rng::new(99);
        let p2 = Perlin::new(&mut rng);
        for i in 0..500 {
            let x = i as f64 * 0.173;
            let z = i as f64 * -0.071;
            let a = p1.noise2(x, z);
            let b = p2.noise2(x, z);
            assert_eq!(a, b);
            assert!(a >= -1.01 && a <= 1.01);
        }
    }

    #[test]
    fn fbm_continuous_across_samples() {
        let mut rng = Rng::new(5);
        let f = Fbm::new(&mut rng, 4, 1.0 / 100.0, 0.5);
        let mut prev = f.sample2(0.0, 0.0);
        for i in 1..2000 {
            let v = f.sample2(i as f64 * 0.5, 17.3);
            assert!((v - prev).abs() < 0.35, "discontinuité à {i}: {prev} -> {v}");
            prev = v;
        }
    }

    #[test]
    fn value_noise_deterministic_and_bounded() {
        let mut a = Rng::new(11);
        let v1 = ValueNoise::new(&mut a);
        let mut b = Rng::new(11);
        let v2 = ValueNoise::new(&mut b);
        for i in 0..600 {
            let x = i as f64 * 0.31;
            let y = i as f64 * -0.13;
            let n1 = v1.noise2(x, y);
            let n2 = v2.noise2(x, y);
            assert_eq!(n1, n2);
            assert!(n1 >= -1.01 && n1 <= 1.01, "value noise out of bounds: {n1}");
        }
    }

    #[test]
    fn simplex_deterministic_and_bounded() {        let mut a = Rng::new(23);
        let s1 = Simplex2D::new(&mut a);
        let mut b = Rng::new(23);
        let s2 = Simplex2D::new(&mut b);
        for i in 0..600 {
            let x = i as f64 * 0.17;
            let y = i as f64 * 0.29;
            let n1 = s1.noise2(x, y);
            let n2 = s2.noise2(x, y);
            assert_eq!(n1, n2);
            assert!(n1 >= -1.01 && n1 <= 1.01, "simplex out of bounds: {n1}");
        }
        // Continuité : sur un pas TRÈS fin (les vraies discontinuités
        // sauteraient d'un ordre de grandeur). Le simplex brut a une pente
        // locale plus forte que le perlin lissé, donc on ne teste pas sur
        // des pas larges.
        let mut prev = s1.noise2(0.5, -0.5);
        for i in 1..20000 {
            let v = s1.noise2(i as f64 * 0.01 + 0.5, i as f64 * 0.007 - 0.5);
            assert!((v - prev).abs() < 0.2, "simplex discontinu: {prev} -> {v}");
            prev = v;
        }
    }

    #[test]
    fn value_fbm_deterministic_and_bounded() {
        let mut a = Rng::new(7);
        let v1 = ValueFbm::new(&mut a, 3, 1.0 / 40.0, 0.5);
        let mut b = Rng::new(7);
        let v2 = ValueFbm::new(&mut b, 3, 1.0 / 40.0, 0.5);
        for i in 0..400 {
            let x = i as f64 * 0.27;
            let z = i as f64 * 0.11;
            let n1 = v1.sample2(x, z);
            let n2 = v2.sample2(x, z);
            assert_eq!(n1, n2);
            assert!(n1 >= -1.01 && n1 <= 1.01, "value fbm out of bounds: {n1}");
        }
    }

    #[test]
    fn simplex_fbm_deterministic_and_bounded() {
        let mut a = Rng::new(13);
        let s1 = SimplexFbm::new(&mut a, 3, 1.0 / 40.0, 0.5);
        let mut b = Rng::new(13);
        let s2 = SimplexFbm::new(&mut b, 3, 1.0 / 40.0, 0.5);
        for i in 0..400 {
            let x = i as f64 * 0.19;
            let z = i as f64 * -0.07;
            let n1 = s1.sample2(x, z);
            let n2 = s2.sample2(x, z);
            assert_eq!(n1, n2);
            assert!(n1 >= -1.01 && n1 <= 1.01, "simplex fbm out of bounds: {n1}");
        }
    }
}
