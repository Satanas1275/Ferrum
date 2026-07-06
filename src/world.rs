use std::collections::HashMap;
use std::io::Write as _;
use std::sync::atomic::AtomicI32;
use std::sync::Arc;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use tokio::sync::Mutex;

use crate::config::ServerConfig;
use crate::items::ItemEntity;
use crate::net::writing::build_packet;
use crate::player::Player;

pub struct State {
    pub players: Mutex<HashMap<i32, Player>>,
    pub world: Mutex<HashMap<(i32, i32, i32), u16>>,
    pub items: Mutex<HashMap<i32, ItemEntity>>,
    pub next_id: AtomicI32,
    pub config: ServerConfig,
}

pub type SharedState = Arc<State>;

pub fn get_block(world: &HashMap<(i32, i32, i32), u16>, x: i32, y: i32, z: i32) -> u16 {
    if let Some(&b) = world.get(&(x, y, z)) {
        b
    } else if y <= 15 {
        3
    } else {
        0
    }
}

pub fn compute_heightmap(world: &HashMap<(i32, i32, i32), u16>, chunk_x: i32, chunk_z: i32) -> [[i32; 16]; 16] {
    let mut heights = [[-1i32; 16]; 16];
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    for lx in 0..16 {
        for lz in 0..16 {
            let wx = base_x + lx as i32;
            let wz = base_z + lz as i32;
            for y in (0..256).rev() {
                if get_block(world, wx, y, wz) != 0 {
                    heights[lx][lz] = y;
                    break;
                }
            }
        }
    }
    heights
}

pub fn pack_nibbles(values: &[u8; 4096]) -> Vec<u8> {
    let mut result = Vec::with_capacity(2048);
    for i in 0..2048 {
        let byte = (values[2 * i + 1] << 4) | (values[2 * i] & 0x0F);
        result.push(byte);
    }
    result
}

pub fn generate_section(
    world: &HashMap<(i32, i32, i32), u16>,
    heights: &[[i32; 16]; 16],
    chunk_x: i32,
    chunk_z: i32,
    section_y: i32,
) -> Vec<u8> {
    let mut block_data = [0u8; 4096];
    let mut sky_light = [0u8; 4096];
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    let y_start = section_y * 16;
    for lx in 0..16 {
        for lz in 0..16 {
            for ly in 0..16 {
                let wx = base_x + lx as i32;
                let wz = base_z + lz as i32;
                let wy = y_start + ly as i32;
                let idx = ly as usize * 256 + lz * 16 + lx;
                block_data[idx] = get_block(world, wx, wy, wz) as u8;
                sky_light[idx] = if wy >= heights[lx][lz] { 15 } else { 0 };
            }
        }
    }
    let mut data = Vec::new();
    data.extend(block_data);
    data.extend(vec![0u8; 2048]);
    data.extend(vec![0u8; 2048]);
    data.extend(pack_nibbles(&sky_light));
    data
}

pub fn chunk_section_count(world: &HashMap<(i32, i32, i32), u16>, chunk_x: i32, chunk_z: i32) -> usize {
    let base_x = chunk_x * 16;
    let base_z = chunk_z * 16;
    let mut max_y = 0i32;
    for lx in 0..16 {
        for lz in 0..16 {
            let wx = base_x + lx;
            let wz = base_z + lz;
            for y in (0..256).rev() {
                if get_block(world, wx, y, wz) != 0 {
                    max_y = max_y.max(y);
                    break;
                }
            }
        }
    }
    (max_y / 16 + 1).clamp(1, 16) as usize
}

pub fn build_chunk_packet(chunk_x: i32, chunk_z: i32, world: &HashMap<(i32, i32, i32), u16>) -> Vec<u8> {
    let num_sections = chunk_section_count(world, chunk_x, chunk_z);
    let heights = compute_heightmap(world, chunk_x, chunk_z);
    let mut raw_data = Vec::new();
    for s in 0..num_sections {
        raw_data.extend(generate_section(world, &heights, chunk_x, chunk_z, s as i32));
    }
    raw_data.extend(vec![1u8; 256]);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&raw_data).expect("compress chunk");
    let compressed_data = encoder.finish().expect("finish compression");

    let mut content = Vec::new();
    content.extend(chunk_x.to_be_bytes());
    content.extend(chunk_z.to_be_bytes());
    content.push(1u8);
    let primary_bitmap = ((1u16 << num_sections) - 1) as u16;
    content.extend(primary_bitmap.to_be_bytes());
    content.extend((0u16).to_be_bytes());
    content.extend((compressed_data.len() as i32).to_be_bytes());
    content.extend(compressed_data);

    build_packet(0x21, &mut content)
}
