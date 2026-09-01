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

/// Encode un tableau de chunk dense au format disque : 1 octet "nombre de
/// sections", puis les blocs u16 LE section par section. Format identique à
/// l'ancien encodage map-based.
pub fn encode_chunk_data(data: &[u16; crate::world::CHUNK_LEN]) -> Vec<u8> {
    let mut section_count = 0u8;
    for sy in (0..16).rev() {
        let y_start = sy * 16 * 256;
        let y_end = y_start + 16 * 256;
        if data[y_start..y_end].iter().any(|&b| b != 0) {
            section_count = sy as u8 + 1;
            break;
        }
    }

    let n_sections = section_count as usize;
    let mut out = Vec::with_capacity(1 + n_sections * 16 * 256 * 2);
    out.push(section_count);
    let end = n_sections * 16 * 256;
    for &b in &data[..end] {
        out.extend_from_slice(&b.to_le_bytes());
    }
    out
}

/// Sauvegarde tous les chunks fusionnés présents en mémoire.
///
/// `encoded` = liste ((cx, cz), données encodées) préparée par
/// `persist_world` (encodage sous lock, IO hors lock).
///
/// `known` = ensemble des chunks légitimes (générés cette session ou
/// chargés du disque) : les fichiers .dat hors de cet ensemble sont
/// supprimés. Avec la génération paresseuse, un chunk pas encore visité ne
/// figure PAS dans la map monde et son fichier ne doit surtout pas être
/// supprimé.
pub fn save_all_chunks(
    encoded: Vec<((i32, i32), Vec<u8>)>,
    known: &std::collections::HashSet<(i32, i32)>,
) -> std::io::Result<()> {
    ensure_dirs();
    for ((cx, cz), data) in &encoded {
        fs::write(chunk_path(*cx, *cz), data)?;
    }

    // Les chunks connus mais absents de la map monde (éditions en attente,
    // jamais générés) doivent garder leur fichier existant tel quel.
    if let Ok(entries) = fs::read_dir(CHUNKS_DIR) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(stripped) = name.strip_suffix(".dat") {
                    let parts: Vec<&str> = stripped.split('_').collect();
                    if parts.len() == 2 {
                        if let (Ok(cx), Ok(cz)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            if known.contains(&(cx, cz)) {
                                continue;
                            }
                        }
                    }
                    let _ = fs::remove_file(format!("{}/{}", CHUNKS_DIR, name));
                }
            }
        }
    }

    Ok(())
}

/// Sauvegarde des éditions en attente (chunks chargés du disque mais pas
/// encore régénérés/fusionnés). Le fichier reste partiel : il sera
/// réécrit en entier au prochain autosave suivant la fusion.
pub fn save_pending_edits(pending: &HashMap<(i32, i32), HashMap<(i32, i32, i32), u16>>) -> std::io::Result<()> {
    ensure_dirs();
    for ((cx, cz), blocks) in pending {
        save_chunk(*cx, *cz, blocks)?;
    }
    Ok(())
}

/// Charge toutes les éditions disque, groupées par chunk.
///
/// Contrairement à l'ancien `load_all_chunks`, les blocs à 0 explicites
/// (blocs détruits par un joueur) sont CONSERVÉS : ils doivent pouvoir
/// effacer le terrain régénéré au moment de la fusion.
pub fn load_all_edits() -> std::io::Result<(HashMap<(i32, i32), HashMap<(i32, i32, i32), u16>>, usize)> {
    ensure_dirs();
    let mut per_chunk: HashMap<(i32, i32), HashMap<(i32, i32, i32), u16>> = HashMap::new();
    let mut total = 0usize;

    if let Ok(entries) = fs::read_dir(CHUNKS_DIR) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(stripped) = name_str.strip_suffix(".dat") {
                let parts: Vec<&str> = stripped.split('_').collect();
                if parts.len() == 2 {
                    if let (Ok(cx), Ok(cz)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                        if let Ok(chunk_blocks) = load_chunk_with_zeros(cx, cz) {
                            total += chunk_blocks.len();
                            per_chunk.insert((cx, cz), chunk_blocks);
                        }
                    }
                }
            }
        }
    }

    Ok((per_chunk, total))
}

/// Comme `load_chunk` mais conserve les zéros explicites.
fn load_chunk_with_zeros(cx: i32, cz: i32) -> std::io::Result<HashMap<(i32, i32, i32), u16>> {
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

    'outer: for sy in 0..section_count {
        let y_start = (sy as i32) * 16;
        for ly in 0..16 {
            for lz in 0..16 {
                for lx in 0..16 {
                    if idx + 2 > buf.len() {
                        break 'outer;
                    }
                    let block = u16::from_le_bytes([buf[idx], buf[idx + 1]]);
                    idx += 2;
                    let wx = base_x + lx;
                    let wz = base_z + lz;
                    let wy = y_start + ly;
                    blocks.insert((wx, wy, wz), block);
                }
            }
        }
    }

    Ok(blocks)
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

#[derive(Clone)]
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
            last_bcast_x: i32::MIN,
            last_bcast_y: i32::MIN,
            last_bcast_z: i32::MIN,
            last_bcast_yaw: 0,
            last_bcast_pitch: 0,
            last_chunk: (i32::MIN, i32::MIN),
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