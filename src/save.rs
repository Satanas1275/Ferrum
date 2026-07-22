use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::player::Player;

const WORLD_DIR: &str = "world";
const CHUNKS_DIR: &str = "world/chunks";
const PLAYERS_DIR: &str = "world/players";

fn ensure_dirs() {
    let _ = fs::create_dir_all(WORLD_DIR);
    let _ = fs::create_dir_all(CHUNKS_DIR);
    let _ = fs::create_dir_all(PLAYERS_DIR);
}

fn chunk_path(cx: i32, cz: i32) -> String {
    format!("{}/{cx}_{cz}.dat", CHUNKS_DIR)
}

fn player_path(uuid: &str) -> String {
    format!("{}/{}.dat", PLAYERS_DIR, uuid)
}

pub fn save_chunk(cx: i32, cz: i32, blocks: &HashMap<(i32, i32, i32), u16>) -> std::io::Result<()> {
    ensure_dirs();
    let base_x = cx * 16;
    let base_z = cz * 16;

    let mut section_count = 0u8;
    for sy in 0u8..16 {
        let y_start = (sy as i32) * 16;
        let mut has_blocks = false;
        for ly in 0..16 {
            for lz in 0..16 {
                for lx in 0..16 {
                    let wx = base_x + lx;
                    let wz = base_z + lz;
                    let wy = y_start + ly;
                    if blocks.get(&(wx, wy, wz)).copied().unwrap_or(0) != 0 {
                        has_blocks = true;
                        break;
                    }
                }
                if has_blocks { break; }
            }
            if has_blocks { break; }
        }
        if has_blocks {
            section_count = sy + 1;
        }
    }

    let mut data = Vec::new();
    data.write_all(&[section_count])?;

    for sy in 0..section_count as usize {
        let y_start = (sy as i32) * 16;
        for ly in 0..16 {
            for lz in 0..16 {
                for lx in 0..16 {
                    let wx = base_x + lx;
                    let wz = base_z + lz;
                    let wy = y_start + ly;
                    let block = blocks.get(&(wx, wy, wz)).copied().unwrap_or(0);
                    data.write_all(&block.to_le_bytes())?;
                }
            }
        }
    }

    fs::write(chunk_path(cx, cz), data)?;
    Ok(())
}

pub fn load_chunk(cx: i32, cz: i32) -> std::io::Result<HashMap<(i32, i32, i32), u16>> {
    let path = chunk_path(cx, cz);
    let mut file = fs::File::open(&path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;

    let mut blocks = HashMap::new();
    if buf.is_empty() {
        return Ok(blocks);
    }

    let mut idx = 0;
    let section_count = buf[idx] as usize;
    idx += 1;

    let base_x = cx * 16;
    let base_z = cz * 16;

    for sy in 0..section_count {
        let y_start = (sy as i32) * 16;
        for ly in 0..16 {
            for lz in 0..16 {
                for lx in 0..16 {
                    if idx + 2 > buf.len() {
                        return Ok(blocks);
                    }
                    let block = u16::from_le_bytes([buf[idx], buf[idx + 1]]);
                    idx += 2;
                    if block != 0 {
                        let wx = base_x + lx;
                        let wz = base_z + lz;
                        let wy = y_start + ly;
                        blocks.insert((wx, wy, wz), block);
                    }
                }
            }
        }
    }

    Ok(blocks)
}

pub fn save_all_chunks(blocks: &HashMap<(i32, i32, i32), u16>) -> std::io::Result<()> {
    ensure_dirs();
    let mut chunks: HashMap<(i32, i32), Vec<(i32, i32, i32, u16)>> = HashMap::new();
    for (&(x, y, z), &block) in blocks {
        if block == 0 { continue; }
        let cx = x >> 4;
        let cz = z >> 4;
        chunks.entry((cx, cz)).or_default().push((x, y, z, block));
    }

    for ((cx, cz), entries) in &chunks {
        let mut chunk_blocks = HashMap::new();
        for &(x, y, z, block) in entries {
            chunk_blocks.insert((x, y, z), block);
        }
        save_chunk(*cx, *cz, &chunk_blocks)?;
    }

    let mut saved_chunks: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(CHUNKS_DIR) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                saved_chunks.push(name.to_string());
            }
        }
    }

    let active_keys: std::collections::HashSet<String> = chunks.keys()
        .map(|(cx, cz)| format!("{}_{}.dat", cx, cz))
        .collect();

    for name in &saved_chunks {
        if !active_keys.contains(name) {
            let _ = fs::remove_file(format!("{}/{}", CHUNKS_DIR, name));
        }
    }

    Ok(())
}

pub fn load_all_chunks() -> std::io::Result<HashMap<(i32, i32, i32), u16>> {
    ensure_dirs();
    let mut all_blocks = HashMap::new();

    if let Ok(entries) = fs::read_dir(CHUNKS_DIR) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(stripped) = name_str.strip_suffix(".dat") {
                let parts: Vec<&str> = stripped.split('_').collect();
                if parts.len() == 2 {
                    if let (Ok(cx), Ok(cz)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                        if let Ok(chunk_blocks) = load_chunk(cx, cz) {
                            all_blocks.extend(chunk_blocks);
                        }
                    }
                }
            }
        }
    }

    Ok(all_blocks)
}

pub fn save_player(player: &Player) -> std::io::Result<()> {
    ensure_dirs();
    let mut data = Vec::new();

    data.write_all(&player.x.to_le_bytes())?;
    data.write_all(&player.y.to_le_bytes())?;
    data.write_all(&player.z.to_le_bytes())?;
    data.write_all(&player.yaw.to_le_bytes())?;
    data.write_all(&player.pitch.to_le_bytes())?;
    data.write_all(&[player.gamemode])?;
    data.write_all(&player.health.to_le_bytes())?;

    for &item in &player.inventory {
        data.write_all(&item.to_le_bytes())?;
    }
    for &count in &player.counts {
        data.write_all(&[count])?;
    }

    data.write_all(&(player.selected_slot as u8).to_le_bytes())?;
    data.write_all(&player.cursor_item.to_le_bytes())?;
    data.write_all(&[player.cursor_count])?;

    fs::write(player_path(&player.uuid), data)?;
    Ok(())
}

pub struct PlayerData {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub gamemode: u8,
    pub health: f32,
    pub inventory: [i16; 45],
    pub counts: [u8; 45],
    pub selected_slot: usize,
    pub cursor_item: i16,
    pub cursor_count: u8,
}

pub fn load_player(uuid: &str) -> Option<PlayerData> {
    let path = player_path(uuid);
    let mut file = fs::File::open(&path).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;

    if buf.len() < 8 + 8 + 8 + 4 + 4 + 1 + 4 + 45 * 2 + 45 + 1 + 2 + 1 {
        return None;
    }

    let mut idx = 0;
    let read_f64 = |buf: &[u8], idx: &mut usize| -> f64 {
        let val = f64::from_le_bytes(buf[*idx..*idx + 8].try_into().unwrap());
        *idx += 8;
        val
    };
    let read_f32 = |buf: &[u8], idx: &mut usize| -> f32 {
        let val = f32::from_le_bytes(buf[*idx..*idx + 4].try_into().unwrap());
        *idx += 4;
        val
    };

    let x = read_f64(&buf, &mut idx);
    let y = read_f64(&buf, &mut idx);
    let z = read_f64(&buf, &mut idx);
    let yaw = read_f32(&buf, &mut idx);
    let pitch = read_f32(&buf, &mut idx);
    let gamemode = buf[idx]; idx += 1;
    let health = read_f32(&buf, &mut idx);

    let mut inventory = [-1i16; 45];
    for i in 0..45 {
        inventory[i] = i16::from_le_bytes([buf[idx], buf[idx + 1]]);
        idx += 2;
    }

    let mut counts = [0u8; 45];
    for i in 0..45 {
        counts[i] = buf[idx];
        idx += 1;
    }

    let selected_slot = buf[idx] as usize;
    idx += 1;
    let cursor_item = i16::from_le_bytes([buf[idx], buf[idx + 1]]);
    idx += 2;
    let cursor_count = buf[idx];

    Some(PlayerData {
        x, y, z, yaw, pitch, gamemode, health,
        inventory, counts, selected_slot, cursor_item, cursor_count,
    })
}

pub fn delete_player(uuid: &str) -> std::io::Result<()> {
    let path = player_path(uuid);
    if Path::new(&path).exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_load_player() {
        let player = Player {
            entity_id: 1,
            uuid: "test-uuid-1234".to_string(),
            username: "TestPlayer".to_string(),
            x: 100.5,
            y: 64.0,
            z: -200.3,
            yaw: 45.0,
            pitch: -10.0,
            gamemode: 0,
            inventory: {
                let mut inv = [-1i16; 45];
                inv[36] = 276;
                inv
            },
            counts: {
                let mut c = [0u8; 45];
                c[36] = 1;
                c
            },
            selected_slot: 0,
            cursor_item: -1,
            cursor_count: 0,
            health: 18.5,
            highest_y: 70.0,
            sneaking: false,
            sender: tokio::sync::mpsc::unbounded_channel().0,
            loaded_chunks: std::collections::HashSet::new(),
        };

        save_player(&player).unwrap();
        let loaded = load_player("test-uuid-1234").unwrap();

        assert_eq!(loaded.x, 100.5);
        assert_eq!(loaded.y, 64.0);
        assert_eq!(loaded.z, -200.3);
        assert_eq!(loaded.yaw, 45.0);
        assert_eq!(loaded.pitch, -10.0);
        assert_eq!(loaded.gamemode, 0);
        assert_eq!(loaded.health, 18.5);
        assert_eq!(loaded.inventory[36], 276);
        assert_eq!(loaded.counts[36], 1);

        let _ = delete_player("test-uuid-1234");
    }
}
