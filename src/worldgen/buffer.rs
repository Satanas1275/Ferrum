//! Buffer de chunk 16x16x256 utilisé pendant la génération.
//!
//! La génération travaille dans ce tableau dense (cache-local) puis le
//! résultat est transféré dans la map monde ; on n'alloue qu'un seul
//! buffer par chunk généré.

pub const CHUNK_SIZE: usize = 16;
pub const WORLD_HEIGHT: usize = 256;
pub const BUFFER_LEN: usize = CHUNK_SIZE * CHUNK_SIZE * WORLD_HEIGHT;

/// Buffer dense indexé en (y, z, x), attaché à un chunk monde précis.
pub struct ChunkBuffer {
    pub cx: i32,
    pub cz: i32,
    blocks: Box<[u16; BUFFER_LEN]>,
    /// Biome par colonne (index z*16+x, ids protocole 1.7.10) : transmis
    /// tel quel au paquet de chunk.
    pub biomes: [u8; CHUNK_SIZE * CHUNK_SIZE],
}

impl ChunkBuffer {
    pub fn new(cx: i32, cz: i32) -> Self {
        Self { cx, cz, blocks: Box::new([0u16; BUFFER_LEN]), biomes: [0u8; CHUNK_SIZE * CHUNK_SIZE] }
    }

    #[inline]
    pub fn idx(x: usize, y: usize, z: usize) -> usize {
        (y << 8) | (z << 4) | x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u16 {
        self.blocks[Self::idx(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, v: u16) {
        self.blocks[Self::idx(x, y, z)] = v;
    }

    /// Écriture en coordonnées monde : n'écrit que si le bloc tombe dans
    /// CE chunk (les features des chunks voisins débordent et sont
    /// ignorées ici — chaque chunk concerné écrit sa propre part).
    #[inline]
    pub fn set_world(&mut self, wx: i32, wy: i32, wz: i32, v: u16) {
        if wx >> 4 != self.cx || wz >> 4 != self.cz {
            return;
        }
        let lx = (wx & 15) as usize;
        let lz = (wz & 15) as usize;
        if wy < 0 || wy >= WORLD_HEIGHT as i32 {
            return;
        }
        self.blocks[Self::idx(lx, wy as usize, lz)] = v;
    }

    /// Lecture en coordonnées monde si le bloc est dans ce chunk.
    #[inline]
    pub fn get_world(&self, wx: i32, wy: i32, wz: i32) -> Option<u16> {
        if wx >> 4 != self.cx || wz >> 4 != self.cz || wy < 0 || wy >= WORLD_HEIGHT as i32 {
            return None;
        }
        Some(self.blocks[Self::idx((wx & 15) as usize, wy as usize, (wz & 15) as usize)])
    }

    pub fn iter_nonzero(&self) -> impl Iterator<Item = (usize, u16)> + '_ {
        self.blocks
            .iter()
            .enumerate()
            .filter(|&(_, b)| *b != 0)
            .map(|(i, b)| (i, *b))
    }

    /// Consomme le buffer : (blocs, biomes par colonne).
    pub fn into_parts(self) -> (Box<[u16; BUFFER_LEN]>, [u8; CHUNK_SIZE * CHUNK_SIZE]) {
        (self.blocks, self.biomes)
    }
}

/// Encode un id de bloc + métadonnée au format de stockage Ferrum
/// (id sur 12 bits bas, meta sur 4 bits hauts).
#[inline]
pub const fn bm(id: u16, meta: u8) -> u16 {
    id | ((meta as u16) << 12)
}

/// Id seul (meta 0).
#[inline]
pub const fn b(id: u16) -> u16 {
    id
}
