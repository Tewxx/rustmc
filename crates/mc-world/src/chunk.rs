//! 16×256×16 chunk column with 1.8 storage: 16 sections of `id << 4 | meta`, plus light.

pub const SECTION_VOLUME: usize = 16 * 16 * 16;
pub const COLUMN_VOLUME: usize = 16 * 16 * 256;

#[derive(Clone)]
pub struct Section {
    /// Indexed `y << 8 | z << 4 | x`, like `ExtendedBlockStorage`.
    pub blocks: Box<[u16; SECTION_VOLUME]>,
}

impl Section {
    fn empty() -> Self {
        Self { blocks: Box::new([0; SECTION_VOLUME]) }
    }
}

#[derive(Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub sections: [Option<Section>; 16],
    /// Sky and block light (0–15), indexed `y << 8 | z << 4 | x`.
    pub sky_light: Box<[u8; COLUMN_VOLUME]>,
    pub block_light: Box<[u8; COLUMN_VOLUME]>,
    /// True once light came from the save or was computed; false means it needs a pass.
    pub light_valid: bool,
}

#[inline]
pub fn column_index(x: usize, y: i32, z: usize) -> usize {
    ((y as usize) << 8) | (z << 4) | x
}

impl Chunk {
    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            sections: Default::default(),
            sky_light: vec![0; COLUMN_VOLUME].into_boxed_slice().try_into().unwrap(),
            block_light: vec![0; COLUMN_VOLUME].into_boxed_slice().try_into().unwrap(),
            light_valid: false,
        }
    }

    /// Local coords. Returns `(id, meta)`; air outside 0..256.
    pub fn get(&self, x: usize, y: i32, z: usize) -> (u16, u8) {
        if !(0..256).contains(&y) {
            return (0, 0);
        }
        match &self.sections[(y >> 4) as usize] {
            Some(s) => {
                let v = s.blocks[((y as usize & 15) << 8) | (z << 4) | x];
                (v >> 4, (v & 15) as u8)
            }
            None => (0, 0),
        }
    }

    pub fn set(&mut self, x: usize, y: i32, z: usize, id: u16, meta: u8) {
        if !(0..256).contains(&y) {
            return;
        }
        let s = self.sections[(y >> 4) as usize].get_or_insert_with(Section::empty);
        s.blocks[((y as usize & 15) << 8) | (z << 4) | x] = (id << 4) | (meta as u16 & 15);
        self.light_valid = false;
    }
}
