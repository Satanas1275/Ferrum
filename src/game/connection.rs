use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::items::{self, PICKUP_DELAY_TICKS};
use crate::net::reading::{read_packet, read_varint_buf, read_string_buf, read_f64_buf, read_f32_buf, read_i32_buf, read_i64_buf, read_u8_buf, read_i16_buf, read_slot_full};
use crate::net::writing::write_string;
use crate::packets;
use crate::player::Player;
use crate::save;
use crate::util::{face_offset, offline_uuid};
use crate::world::{get_block, build_chunk_packet, SharedState};

/// Rayon de recherche horizontale (en blocs) autour du point de spawn.
const SPAWN_SEARCH_RADIUS: i32 = 8;

/// Distance de vue en chunks (rayon autour du chunk du joueur).
const VIEW_DISTANCE: i32 = 3;

fn is_interactive_block(stored: u16) -> bool {
    matches!(stored & 0xFFF,
             23 | 25 | 26 | 54 | 58 | 61 | 62 | 64 | 69 | 71 |
             77 | 84 | 92 | 93 | 94 | 96 | 107 | 116 | 117 | 118 | 130 |
             137 | 138 | 143 | 144 | 145 | 146 | 154 | 158 | 167 |
             183 | 184 | 185 | 186
    )
}

fn toggle_block(stored: u16) -> Option<u16> {
    let block_id = stored & 0xFFF;
    let meta = ((stored >> 12) & 0x0F) as u8;
    match block_id as u16 {
        64 | 71 | 96 | 167 | 107 | 183 | 184 | 185 | 186 => {
            Some(block_id | (((meta ^ 4) as u16) << 12))
        },
        69 | 77 | 143 => {
            Some(block_id | (((meta ^ 8) as u16) << 12))
        },
        93 | 94 => {
            // le repeater ne "toggle" pas ici : on fait juste tourner son
            // délai (bits 2-3, 0-3 -> 1 à 4 ticks) sans toucher à la
            // direction (bits 0-1) ni à son état on/off (encodé dans le
            // block id, pas dans le meta).
            let facing = meta & 0x03;
            let delay = (meta >> 2) & 0x03;
            let new_delay = (delay + 1) & 0x03;
            let new_meta = facing | (new_delay << 2);
            Some(block_id | ((new_meta as u16) << 12))
        },
        _ => None,
    }
}

fn item_to_block_id(item_id: i16) -> u16 {
    match item_id {
        324 => 64,
        330 => 71,
        326 => 9,
        327 => 11,
        356 => 93,
        404 => 149,
        355 => 26,
        354 => 92,
        323 => 63,
        331 => 55,
        _ => item_id as u16,
    }
}

fn is_valid_placeable_block(block_id: u16) -> bool {
    matches!(block_id,
             1 |  2 |  3 |  4 |  5 |  6 |  7 |
             9 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 |
             20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 |
             30 | 31 | 32 | 33 | 35 |
             37 | 38 | 39 | 40 | 41 | 42 | 43 | 44 | 45 | 46 |
             47 | 48 | 49 | 50 | 52 | 53 | 54 | 55 | 56 | 57 |
             58 | 60 | 61 | 63 | 64 | 65 | 66 | 67 | 68 | 69 | 70 | 71 |
             72 | 73 | 75 | 76 | 77 | 78 | 79 | 80 | 81 | 82 |
             83 | 84 | 85 | 86 | 87 | 88 | 89 | 91 | 92 | 93 |
             96 | 97 | 98 | 99 | 100 | 101 | 102 | 103 |
             106 | 107 | 108 | 109 | 110 | 111 | 112 | 113 | 114 |
             116 | 117 | 118 | 120 | 121 | 123 |
             125 | 126 | 128 | 129 | 130 | 131 | 133 | 134 | 135 |
             136 | 137 | 138 | 139 | 141 | 142 | 143 | 144 | 145 |
             146 | 147 | 148 | 149 | 151 | 152 | 153 | 154 | 155 |
             156 | 157 | 158 | 159 | 160 | 161 | 162 | 163 | 164 |
             165 | 167 | 168 | 169 | 170 | 171 | 172 | 173 | 174 |
             175 | 176 | 177 | 178 | 179 | 180 | 181 | 182 | 183 |
             184 | 185 | 186 | 187 | 188 | 189 | 190 | 191 | 192 |
             193 | 194 | 195 | 196 | 197 | 198
    )
}

/// Return whether the given `block_id` can be placed on the `face` of `support_block`.
fn can_place_on(block_id: u16, face: u8, support_block: u16) -> bool {
    let support = support_block & 0xFFF;
    let solid = support != 0;
    match block_id {
        50 | 75 | 76 => (face == 1 || (2..=5).contains(&face)) && solid,
        55 => face == 1 && solid,
        27 | 28 | 66 | 157 => (face == 1 || (2..=5).contains(&face)) && solid,
        65 => (2..=5).contains(&face) && solid,
        6 => face == 1 && matches!(support, 2 | 3 | 60),
        37 | 38 => face == 1 && matches!(support, 2 | 3 | 60),
        39 | 40 => face == 1 && solid,
        70 | 72 | 147 | 148 => face == 1 && solid,
        69 | 77 | 143 => solid,
        78 => face == 1 && solid,
        81 => face == 1 && support == 12,
        83 => face == 1 && matches!(support, 2 | 3 | 12),
        106 => (2..=5).contains(&face) && solid,
        111 => face == 1 && (support == 8 || support == 9),
        131 => (2..=5).contains(&face) && solid,
        171 => face == 1 && solid,
        63 => face == 1 && solid,
        68 => (2..=5).contains(&face) && solid,
        _ => true,
    }
}

fn block_metadata(block_id: u16, face: u8, yaw: f32, pitch: f32, cursor_y: u8, damage: u8) -> u8 {
    let yaw_dir = ((yaw * 4.0 / 360.0 + 0.5).floor() as i32 & 3) as u8;
    match block_id {
        // === Pure subtype (damage → metadata, no orientation) ===
        5 | 35 | 159 | 171 => damage & 0x0F,
        168 => damage & 0x03,
        24 | 97 | 98 | 155 => damage & 0x03,
        31 => damage & 0x03,
        6 | 18 | 175 => damage & 0x0F,
        160 => damage & 0x0F,

        // === Slabs (subtype + top/bottom from face) ===
        44 => (damage & 0x07) | if face == 0 { 0x08 } else { 0 },
        126 => (damage & 0x03) | if face == 0 { 0x08 } else { 0 },
        43 => damage & 0x07,
        125 => damage & 0x03,
        181 => damage & 0x07,
        182 => (damage & 0x07) | if face == 0 { 0x08 } else { 0 },

        // === Logs (subtype + orientation from face) ===
        17 | 161 | 162 => {
            let orientation = match face {
                0 | 1 => 0,
                4 | 5 => 4,
                2 | 3 => 8,
                _ => 0,
            };
            (damage & 0x03) | orientation
        },
        170 => match face {
            0 | 1 => 0,
            4 | 5 => 4,
            2 | 3 => 8,
            _ => 0,
        },

        // === Ladder ===
        65 => match face {
            2 | 3 | 4 | 5 => face,
            _ => 2,
        },

        // === Torch / Redstone torch ===
        50 | 75 | 76 => match face {
            0 | 1 => 5,
            2 => 4,
            3 => 3,
            4 => 2,
            5 => 1,
            _ => 5,
        },

        // === Lever ===
        // meta 0=ceiling, 5=floor, 1=east-wall, 2=west-wall, 3=south-wall, 4=north-wall
        69 => match face {
            0 => 0,
            1 => 5,
            2 => 3,
            3 => 4,
            4 => 1,
            5 => 2,
            _ => 0,
        },

        // === Buttons ===
        77 | 143 => match face {
            0 => 0,
            1 => 5,
            2 => 4,
            3 => 3,
            4 => 2,
            5 => 1,
            _ => 0,
        },

        // === Stairs ===
        53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 |
        156 | 163 | 164 | 180 | 203 => {
            let stair_dir: [u8; 4] = [2, 1, 3, 0];
            let mut meta = stair_dir[yaw_dir as usize];
            if face == 0 { meta |= 4; }
            meta
        },

        // === Chest / Furnace / Dispenser / Dropper ===
        54 | 61 | 62 | 130 | 23 | 158 => match face {
            2 => 2,
            3 => 3,
            4 => 5,
            5 => 4,
            _ => 2,
        },

        // === Piston / Sticky piston ===
        // On the top of a block, a piston faces the player's horizontal
        // direction (like stairs/trapdoors), rather than always pointing up.
        29 | 33 => match face {
            0 => 0,
            1 if pitch > 45.0 => 1,
            1 if pitch < -45.0 => 0,
            // yaw 0/180 were inverted (south/north) in the previous map.
            1 => [2, 5, 3, 4][yaw_dir as usize],
            2 | 3 | 4 | 5 => face,
            _ => 1,
        },
        34 => match face {
            0 | 1 | 2 | 3 | 4 | 5 => face,
            _ => 0,
        },

        // === Trapdoor ===
        96 | 167 => match face {
            0 | 1 => {
                let dir = [1u8, 2, 0, 3][yaw_dir as usize];
                if face == 0 { dir } else { dir | 8 }
            },
            2 => if cursor_y > 8 { 0 | 8 } else { 0 },
            3 => if cursor_y > 8 { 1 | 8 } else { 1 },
            4 => if cursor_y > 8 { 2 | 8 } else { 2 },
            5 => if cursor_y > 8 { 3 | 8 } else { 3 },
            _ => 0,
        },

        // === Fence gates ===
        107 | 183 | 184 | 185 | 186 => yaw_dir,

        // === Doors ===
        64 | 71 => (yaw_dir + 1) & 3,

        // === Bed ===
        26 => match face {
            2 => 0,
            3 => 1,
            4 => 2,
            5 => 3,
            _ => 0,
        },

        // === Sign (standing: 16 directions via yaw; wall: face directe) ===
        63 => {
            ((yaw * 16.0 / 360.0 + 0.5).floor() as i32 & 15) as u8
        },
        68 => face,

        // === Repeater / Comparator ===
        93 | 149 => (yaw_dir + 2) & 3,

        // === Rails ===
        66 => {
            if (2..=5).contains(&face) {
                match face {
                    2 => 4,
                    3 => 5,
                    4 => 3,
                    5 => 2,
                    _ => 0,
                }
            } else {
                match yaw_dir {
                    0 | 2 => 0,
                    1 | 3 => 1,
                    _ => 0,
                }
            }
        },
        27 | 28 | 157 => {
            if (2..=5).contains(&face) {
                match face {
                    2 => 4,
                    3 => 5,
                    4 => 3,
                    5 => 2,
                    _ => 0,
                }
            } else {
                match yaw_dir {
                    0 | 2 => 0,
                    1 | 3 => 1,
                    _ => 0,
                }
            }
        },

        // === Default: no metadata (subtype blocks are handled above) ===
        _ => 0,
    }
}

/// Check if the block at (nx, ny, nz) still has its required support.
/// Returns `true` if it should break.
fn lost_support(world: &std::collections::HashMap<(i32, i32, i32), u16>, nx: i32, ny: i32, nz: i32, block_id: u16, meta: u8) -> bool {
    let support_air = |ox: i32, oy: i32, oz: i32| -> bool {
        let b = crate::world::get_block(world, ox, oy, oz);
        (b & 0xFFF) == 0
    };
    match block_id {
        50 | 75 | 76 => {
            if meta == 5 {
                support_air(nx, ny - 1, nz)
            } else {
                let (off_x, off_z) = match meta & 0x07 {
                    1 => (-1, 0), 2 => (1, 0), 3 => (0, -1), 4 => (0, 1),
                    _ => (0, 0),
                };
                support_air(nx + off_x, ny, nz + off_z)
            }
        },
        65 => {
            let (off_x, off_z) = match meta {
                2 => (0, 1), 3 => (0, -1), 4 => (1, 0), 5 => (-1, 0),
                _ => (0, 0),
            };
            support_air(nx + off_x, ny, nz + off_z)
        },
        69 => {
            match meta & 0x07 {
                0 => support_air(nx, ny + 1, nz),
                5 => support_air(nx, ny - 1, nz),
                1 => support_air(nx - 1, ny, nz),
                2 => support_air(nx + 1, ny, nz),
                3 => support_air(nx, ny, nz - 1),
                4 => support_air(nx, ny, nz + 1),
                _ => false,
            }
        },
        77 | 143 => {
            match meta & 0x07 {
                0 => support_air(nx, ny - 1, nz),
                5 => support_air(nx, ny + 1, nz),
                4 => support_air(nx, ny, nz + 1),
                3 => support_air(nx, ny, nz - 1),
                2 => support_air(nx - 1, ny, nz),
                1 => support_air(nx + 1, ny, nz),
                _ => false,
            }
        },
        27 | 28 | 55 | 66 | 70 | 72 | 78 | 147 | 148 | 157 => {
            ny == 0 || support_air(nx, ny - 1, nz)
        },
        6 | 31 | 32 | 37 | 38 | 39 | 40 => {
            ny == 0 || {
                let below = crate::world::get_block(world, nx, ny - 1, nz) & 0xFFF;
                below != 2 && below != 3 && below != 60
            }
        },
        81 => ny == 0 || support_air(nx, ny - 1, nz) || (crate::world::get_block(world, nx, ny - 1, nz) & 0xFFF) != 12,
        83 => ny == 0 || support_air(nx, ny - 1, nz),
        106 => {
            let has_support = (2..=5).any(|side| {
                let (ox, oz) = match side { 2 => (0, -1), 3 => (0, 1), 4 => (-1, 0), 5 => (1, 0), _ => (0, 0) };
                crate::world::get_block(world, nx + ox, ny, nz + oz) != 0
            });
            !has_support
        },
        131 => false,
        171 => ny == 0 || support_air(nx, ny - 1, nz),
        _ => false,
    }
}

pub async fn notify_neighbors(state: &crate::world::SharedState, x: i32, y: i32, z: i32) {
    let neighbors = [
        (x - 1, y, z), (x + 1, y, z),
        (x, y - 1, z), (x, y + 1, z),
        (x, y, z - 1), (x, y, z + 1),
    ];
    let mut to_break: Vec<(i32, i32, i32, u16)> = Vec::new();
    let world = state.world.lock().await;
    for &(nx, ny, nz) in &neighbors {
        if let Some(&stored) = world.get(&(nx, ny, nz)) {
            let block_id = stored & 0xFFF;
            let meta = ((stored >> 12) & 0x0F) as u8;
            if lost_support(&world, nx, ny, nz, block_id, meta) {
                to_break.push((nx, ny, nz, stored));
            }
        }
    }
    drop(world);
    for (bx, by, bz, stored) in to_break {
        {
            let mut world = state.world.lock().await;
            world.insert((bx, by, bz), 0);
        }
        let pkt = crate::packets::build_block_change(bx, by as u8, bz, 0);
        {
            let players = state.players.lock().await;
            for (_, player) in players.iter() {
                let _ = player.sender.send(pkt.clone());
            }
        }
        let item_id = crate::packets::block_to_item(stored);
        if item_id >= 0 {
            crate::items::spawn_item_entity(state, item_id, 1, 0,
                                            bx as f64 + 0.5, by as f64 + 0.5, bz as f64 + 0.5,
                                            0, 0, 0).await;
        }
        crate::game::redstone::schedule_update(state, bx, by, bz).await;
    }
}

fn chunks_in_view(cx: i32, cz: i32, view_distance: i32) -> Vec<(i32, i32)> {
    let mut chunks = Vec::new();
    for dx in -view_distance..=view_distance {
        for dz in -view_distance..=view_distance {
            chunks.push((cx + dx, cz + dz));
        }
    }
    chunks
}

async fn update_chunks_for_player(
    state: &SharedState,
    entity_id: i32,
) {
    let (player_x, player_z, sender_clone) = {
        let players = state.players.lock().await;
        match players.get(&entity_id) {
            Some(p) => (p.x, p.z, p.sender.clone()),
            None => return,
        }
    };

    let player_cx = (player_x.floor() as i32) >> 4;
    let player_cz = (player_z.floor() as i32) >> 4;

    let desired_chunks: HashSet<(i32, i32)> = chunks_in_view(player_cx, player_cz, VIEW_DISTANCE)
        .into_iter().collect();

    let (to_load, to_unload) = {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(&entity_id) {
            let to_load: Vec<(i32, i32)> = desired_chunks.difference(&player.loaded_chunks).copied().collect();
            let to_unload: Vec<(i32, i32)> = player.loaded_chunks.difference(&desired_chunks).copied().collect();
            for chunk in &to_load {
                player.loaded_chunks.insert(*chunk);
            }
            for chunk in &to_unload {
                player.loaded_chunks.remove(chunk);
            }
            (to_load, to_unload)
        } else {
            return;
        }
    };

    if !to_unload.is_empty() {
        for &(cx, cz) in &to_unload {
            let packet = packets::build_unload_chunk(cx, cz);
            let _ = sender_clone.send(packet);
        }
    }

    if !to_load.is_empty() {
        let world_snapshot = state.world.lock().await.clone();
        let block_min_x = to_load.iter().map(|(cx, _)| cx * 16).min().unwrap_or(0);
        let block_max_x = to_load.iter().map(|(cx, _)| cx * 16 + 15).max().unwrap_or(0);
        let block_min_z = to_load.iter().map(|(_, cz)| cz * 16).min().unwrap_or(0);
        let block_max_z = to_load.iter().map(|(_, cz)| cz * 16 + 15).max().unwrap_or(0);

        for &(cx, cz) in &to_load {
            let pkt = build_chunk_packet(cx, cz, &world_snapshot);
            let _ = sender_clone.send(pkt);
        }

        for (&(wx, wy, wz), &block_id) in world_snapshot.iter() {
            if block_id != 0
                && wx >= block_min_x && wx <= block_max_x
                && wz >= block_min_z && wz <= block_max_z
                && wy >= 0 && wy <= 255
            {
                let packet = packets::build_block_change(wx, wy as u8, wz, block_id);
                let _ = sender_clone.send(packet);
            }
        }
    }
}

fn food_heal(item_id: i16) -> f32 {
    match item_id {
        260 => 4.0,   // apple
        297 => 5.0,   // bread
        319 => 3.0,   // raw porkchop
        320 => 8.0,   // cooked porkchop
        322 => 4.0,   // golden apple
        349 => 2.0,   // raw fish
        350 => 5.0,   // cooked fish
        354 => 2.0,   // cake (slice)
        357 => 2.0,   // cookie
        360 => 2.0,   // melon
        363 => 3.0,   // raw beef
        364 => 8.0,   // cooked beef
        365 => 2.0,   // raw chicken
        366 => 6.0,   // cooked chicken
        411 => 3.0,   // raw rabbit
        412 => 5.0,   // cooked rabbit
        413 => 10.0,  // rabbit stew
        423 => 2.0,   // raw mutton
        424 => 6.0,   // cooked mutton
        _ => 0.0,
    }
}

async fn handle_item_use(state: &SharedState, entity_id: i32, item_id: i16) {
    let heal = food_heal(item_id);
    if heal > 0.0 {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(&entity_id) {
            if player.health >= 20.0 { return; }
            player.health = (player.health + heal).min(20.0);
            let health = player.health;
            drop(players);
            let health_pkt = packets::build_update_health(health, 20i16, 0.0);
            let players = state.players.lock().await;
            if let Some(player) = players.get(&entity_id) {
                let _ = player.sender.send(health_pkt);
            }
        }
        // remove one food item from hand
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(&entity_id) {
            let held_idx = 36 + player.selected_slot;
            if held_idx < 45 && player.inventory[held_idx] == item_id && player.counts[held_idx] > 0 {
                player.counts[held_idx] -= 1;
                if player.counts[held_idx] == 0 {
                    player.inventory[held_idx] = -1;
                }
                let _ = player.sender.send(packets::build_set_slot(0, held_idx as i16, player.inventory[held_idx], player.counts[held_idx] as i8, 0));
            }
        }
    }
}

/// Cherche un endroit libre pour spawn/respawn (2 blocs d'air : pieds + tête).
///
/// Ordre de recherche :
/// 1. Le point central (cx, default_y, cz) — cas normal, quasi toujours pris.
/// 2. Si obstrué, balaie en anneaux concentriques autour de (cx, cz), à la
///    MÊME hauteur, jusqu'à SPAWN_SEARCH_RADIUS blocs de rayon.
/// 3. Si tout le rayon est obstrué à cette hauteur (cas extrême), on
///    retombe sur l'ancien comportement : remonter verticalement sur la
///    colonne centrale jusqu'à trouver de l'air.
///
/// Retourne (x, y, z) du point trouvé.
fn find_safe_spawn(
    world: &std::collections::HashMap<(i32, i32, i32), u16>,
    cx: i32,
    cz: i32,
    default_y: i32,
) -> (f64, f64, f64) {
    find_safe_spawn_inner(world, cx, cz, default_y)
}

fn find_safe_spawn_inner(
    world: &std::collections::HashMap<(i32, i32, i32), u16>,
                   cx: i32,
                   cz: i32,
                   default_y: i32,
) -> (f64, f64, f64) {
    let is_free = |x: i32, y: i32, z: i32| {
        get_block(world, x, y, z) == 0 && get_block(world, x, y + 1, z) == 0
    };

    // `default_y` suppose un monde plat (17 = juste au-dessus du sol plat de
    // fallback). Sur un monde avec du vrai terrain (collines, grottes...),
    // chercher à cette hauteur fixe pouvait tomber sur une poche d'air
    // souterraine (une grotte) qui satisfait "2 blocs d'air" sans être la
    // vraie surface -> le joueur se retrouvait enterré. On calcule d'abord la
    // vraie hauteur du sol à la colonne (cx, cz) en scannant depuis le haut,
    // et on part de là (surface + 1) plutôt que de faire confiance à
    // `default_y`.
    let surface_y = (0..256).rev().find(|&y| get_block(world, cx, y, cz) != 0);
    let start_y = surface_y.map(|y| y + 1).unwrap_or(default_y);

    if is_free(cx, start_y, cz) {
        return (cx as f64, start_y as f64, cz as f64);
    }

    for radius in 1..=SPAWN_SEARCH_RADIUS {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                // Ne garder que le contour de l'anneau (l'intérieur a déjà
                // été testé aux rayons précédents).
                if dx.abs() != radius && dz.abs() != radius {
                    continue;
                }
                let x = cx + dx;
                let z = cz + dz;
                // Comme pour le centre, on part de la vraie surface de CETTE
                // colonne (x, z), pas de celle du centre ni d'une hauteur
                // fixe, pour éviter de retomber dans une grotve/cavité.
                let col_surface = (0..256).rev().find(|&y| get_block(world, x, y, z) != 0);
                let y = col_surface.map(|y| y + 1).unwrap_or(start_y);
                if is_free(x, y, z) {
                    return (x as f64, y as f64, z as f64);
                }
            }
        }
    }

    // Rien trouvé horizontalement dans le rayon : on retombe sur l'ancien
    // comportement, remonter à la verticale sur la colonne centrale.
    let mut y = start_y;
    loop {
        if is_free(cx, y, cz) {
            return (cx as f64, y as f64, cz as f64);
        }
        y += 1;
        if y > 254 {
            return (cx as f64, default_y as f64, cz as f64); // fallback ultime
        }
    }
}

async fn send_status(socket: &mut TcpStream, state: &SharedState) -> std::io::Result<()> {
    let online = state.players.lock().await.len();
    let json = format!(
        r#"{{"version":{{"name":"1.7.10","protocol":5}},"players":{{"max":{},"online":{online},"sample":[]}},"description":{{"text":"{}"}}}}"#,
        state.config.max_players, state.config.motd
    );
    let mut content = write_string(&json);
    let packet = packets::build_packet_id(0x00, &mut content);
    socket.write_all(&packet).await?;
    Ok(())
}

async fn send_login_success(socket: &mut TcpStream, uuid: &str, username: &str) -> std::io::Result<()> {
    let mut content = write_string(uuid);
    content.extend(write_string(username));
    let packet = packets::build_packet_id(0x02, &mut content);
    socket.write_all(&packet).await?;
    Ok(())
}

async fn send_join_game(socket: &mut TcpStream, entity_id: i32) -> std::io::Result<()> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.push(1u8);
    content.push(0u8);
    content.push(1u8);
    content.push(20u8);
    content.extend(write_string("flat"));
    let packet = packets::build_packet_id(0x01, &mut content);
    socket.write_all(&packet).await?;
    Ok(())
}

async fn send_spawn_position(socket: &mut TcpStream, spawn_x: f64, spawn_y: f64, spawn_z: f64) -> std::io::Result<()> {
    let mut content = Vec::new();
    content.extend(spawn_x.to_be_bytes());
    content.extend(spawn_y.to_be_bytes());
    content.extend(spawn_z.to_be_bytes());
    content.extend((0.0f32).to_be_bytes());
    content.extend((0.0f32).to_be_bytes());
    content.push(1u8);
    let packet = packets::build_packet_id(0x08, &mut content);
    socket.write_all(&packet).await?;
    Ok(())
}

pub async fn handle_client(mut socket: TcpStream, state: SharedState) -> std::io::Result<()> {
    let (_id, data) = read_packet(&mut socket).await?;
    let mut idx = 0;
    let protocol_version = read_varint_buf(&data, &mut idx);
    println!("[HANDSHAKE] protocol_version={protocol_version}");
    let _address = read_string_buf(&data, &mut idx);
    let _port = u16::from_be_bytes([data[idx], data[idx + 1]]);
    idx += 2;
    let next_state = read_varint_buf(&data, &mut idx);

    if next_state == 1 {
        let (_id, _data) = read_packet(&mut socket).await?;
        send_status(&mut socket, &state).await?;
        let (_id, data) = read_packet(&mut socket).await?;
        let mut idx = 0;
        let ping_time = read_i64_buf(&data, &mut idx);
        let mut content = ping_time.to_be_bytes().to_vec();
        let pong = packets::build_packet_id(0x01, &mut content);
        socket.write_all(&pong).await?;
        return Ok(());
    }

    let (_id, data) = read_packet(&mut socket).await?;
    let mut idx = 0;
    let username = read_string_buf(&data, &mut idx);
    println!("Login Start received, username = {username}");

    let entity_id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let uuid = offline_uuid(&username);

    send_login_success(&mut socket, &uuid, &username).await?;
    send_join_game(&mut socket, entity_id).await?;

    let world_snapshot = state.world.lock().await.clone();

    let saved_data = save::load_player(&uuid);
    let (start_x, start_y, start_z) = if let Some(ref d) = saved_data {
        println!("Loaded saved data for {username}");
        // Le point sauvegardé peut avoir été enregistré enterré (ancien
        // joueur qui a subi le bug maintenant corrigé) : on revalide qu'il y a
        // bien 2 blocs d'air (pieds + tête) à cet endroit avant de faire
        // confiance à la sauvegarde ; sinon on cherche le sol libre le plus
        // proche à cette colonne (x, z), sans le téléporter ailleurs sur la
        // carte.
        let dx = d.x.floor() as i32;
        let dy = d.y.floor() as i32;
        let dz = d.z.floor() as i32;
        let embedded = get_block(&world_snapshot, dx, dy, dz) != 0
            || get_block(&world_snapshot, dx, dy + 1, dz) != 0;
        if embedded {
            println!("{username}'s saved position was embedded in terrain, correcting");
            let (sx, sy, sz) = find_safe_spawn(&world_snapshot, dx, dz, dy);
            (sx, sy, sz)
        } else {
            (d.x, d.y, d.z)
        }
    } else {
        let (sx, sy, sz) = find_safe_spawn(&world_snapshot, 8, 8, 17);
        (sx, sy, sz)
    };
    // HACK temporaire demandé explicitement par l'utilisateur : peu importe
    // le chemin emprunté ci-dessus (sauvegarde ou find_safe_spawn), le
    // joueur se retrouve systématiquement 2 blocs trop bas par rapport à ce
    // qu'il devrait être, cause pas encore identifiée avec certitude. En
    // attendant de la trouver, on rajoute +2 ici sur le Y, une seule fois,
    // après que start_y a été décidé (peu importe la branche empruntée).
    // Moche mais ça corrige le symptôme immédiatement.
    // -> Si un jour la vraie cause est trouvée, il faudra RETIRER ce +2.0.
    let start_y = start_y + 2.0;

    let start_cx = (start_x.floor() as i32) >> 4;
    let start_cz = (start_z.floor() as i32) >> 4;
    let chunk_min_x = start_cx - VIEW_DISTANCE;
    let chunk_max_x = start_cx + VIEW_DISTANCE;
    let chunk_min_z = start_cz - VIEW_DISTANCE;
    let chunk_max_z = start_cz + VIEW_DISTANCE;
    let initial_chunks: HashSet<(i32, i32)> = (chunk_min_x..=chunk_max_x)
        .flat_map(|x| (chunk_min_z..=chunk_max_z).map(move |z| (x, z)))
        .collect();

    // Le chunk du joueur (celui sous ses pieds) part en premier, tout seul,
    // suivi immédiatement de sa position. Avant, les 49 chunks partaient dans
    // un ordre arbitraire (min->max) puis SEULEMENT ENSUITE la position ;
    // le client pouvait se retrouver à traiter sa propre position avant que
    // son propre chunk soit posé, ou avec des chunks voisins sans le sien -> il
    // tombe dans le vide le temps que tout arrive, puis tout se stabilise d'un
    // coup (effet "reload").
    let own_chunk_pkt = build_chunk_packet(start_cx, start_cz, &world_snapshot);
    socket.write_all(&own_chunk_pkt).await?;
    send_spawn_position(&mut socket, start_x, start_y, start_z).await?;

    let mut remaining_chunks_buf = Vec::new();
    for cx in chunk_min_x..=chunk_max_x {
        for cz in chunk_min_z..=chunk_max_z {
            if cx == start_cx && cz == start_cz {
                continue; // déjà envoyé au-dessus
            }
            let pkt = build_chunk_packet(cx, cz, &world_snapshot);
            remaining_chunks_buf.extend(pkt);
        }
    }
    socket.write_all(&remaining_chunks_buf).await?;

    println!("{username} is now online!");

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let (start_yaw, start_pitch, start_gamemode, start_health, start_inv, start_cnt, start_sel, start_cur_item, start_cur_cnt) = if let Some(ref d) = saved_data {
        (d.yaw, d.pitch, d.gamemode, d.health, d.inventory, d.counts, d.selected_slot, d.cursor_item, d.cursor_count)
    } else {
        (0.0, 0.0, 1u8, 20.0, [-1i16; 45], [0u8; 45], 0usize, -1i16, 0u8)
    };

    let new_player = Player {
        entity_id,
        uuid: uuid.clone(),
        username: username.clone(),
        x: start_x,
        y: start_y,
        z: start_z,
        yaw: start_yaw,
        pitch: start_pitch,
        gamemode: start_gamemode,
        inventory: start_inv,
        counts: start_cnt,
        selected_slot: start_sel,
        cursor_item: start_cur_item,
        cursor_count: start_cur_cnt,
        health: start_health,
        highest_y: start_y,
        sneaking: false,
        sender: tx.clone(),
        loaded_chunks: initial_chunks,
    };

    let (mut reader, mut writer) = socket.into_split();

    tokio::spawn(async move {
        while let Some(packet) = rx.recv().await {
            // On regroupe tout ce qui est déjà en attente dans le channel (ex:
            // une rafale de chunks/block-changes envoyée par
            // update_chunks_for_player) en un seul write, plutôt qu'un
            // write_all par paquet.
            let mut batch = packet;
            while let Ok(more) = rx.try_recv() {
                batch.extend(more);
            }
            if writer.write_all(&batch).await.is_err() {
                break;
            }
        }
    });

    {
        let mut players = state.players.lock().await;

        for other in players.values() {
            let _ = tx.send(packets::build_player_list_item(&other.username, true));
            let _ = tx.send(packets::build_spawn_player(other));
            for pkt in packets::build_equipment_packets(other.entity_id, other) {
                let _ = tx.send(pkt);
            }
            let _ = tx.send(packets::build_entity_metadata_flags(other.entity_id, other.sneaking));
        }

        let list_packet = packets::build_player_list_item(&username, true);
        let spawn_packet = packets::build_spawn_player(&new_player);
        for other in players.values() {
            let _ = other.sender.send(list_packet.clone());
            let _ = other.sender.send(spawn_packet.clone());
        }
        let _ = tx.send(list_packet);

        players.insert(entity_id, new_player);
    }

    {
        let players = state.players.lock().await;
        if let Some(player) = players.get(&entity_id) {
            for i in 0..45 {
                if player.inventory[i] >= 0 {
                    let pkt = packets::build_set_slot(0, i as i16, player.inventory[i], player.counts[i] as i8, 0);
                    let _ = tx.send(pkt);
                }
            }
            let _ = tx.send(packets::build_update_health(player.health, 20i16, 0.0));
            let _ = tx.send(packets::build_game_mode_change(player.gamemode));
        }
    }

    {
        let items = state.items.lock().await;
        for item in items.values() {
            let _ = tx.send(packets::build_spawn_item(item.entity_id, item.item_id, item.x, item.y, item.z, 0, 0, 0));
            let _ = tx.send(packets::build_item_metadata(item.entity_id, item.item_id, item.count, item.damage));
        }
    }

    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            interval.tick().await;
            loop {
                interval.tick().await;
                let mut content = (0i32).to_be_bytes().to_vec();
                let packet = packets::build_packet_id(0x00, &mut content);
                if tx.send(packet).is_err() {
                    break;
                }
            }
        });
    }

    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some((px, py, pz, inv, cnt)) = ({
                    let players = state.players.lock().await;
                    players.get(&entity_id).map(|p| (p.x, p.y, p.z, p.inventory, p.counts))
                }) else { continue; };

                let mut inv_copy = inv;
                let mut cnt_copy = cnt;
                let mut to_pickup: Vec<(i32, i16, i8, i16, usize)> = Vec::new();
                {
                    let items = state.items.lock().await;
                    for (&item_eid, item) in items.iter() {
                        if item.age < PICKUP_DELAY_TICKS { continue; }
                        let dx = px - item.x;
                        let dy = py - item.y;
                        let dz = pz - item.z;
                        if dx * dx + dy * dy + dz * dz < 2.25 {
                            let mut target = (36..45).chain(9..36).find(|&i| inv_copy[i] == item.item_id && cnt_copy[i] < 64);
                            if target.is_none() {
                                target = (36..45).chain(9..36).find(|&i| inv_copy[i] < 0);
                            }
                            if let Some(slot) = target {
                                if inv_copy[slot] < 0 { inv_copy[slot] = item.item_id; }
                                cnt_copy[slot] = cnt_copy[slot].saturating_add(item.count as u8).min(64);
                                to_pickup.push((item_eid, item.item_id, item.count, item.damage, slot));
                            }
                        }
                    }
                }

                if !to_pickup.is_empty() {
                    let mut items = state.items.lock().await;
                    let mut players = state.players.lock().await;
                    let mut broadcasts: Vec<Vec<u8>> = Vec::new();
                    let mut self_packets: Vec<Vec<u8>> = Vec::new();
                    if let Some(player) = players.get_mut(&entity_id) {
                        for (item_eid, item_id, count, damage, slot) in &to_pickup {
                            if items.contains_key(item_eid) {
                                if player.inventory[*slot] < 0 { player.inventory[*slot] = *item_id; }
                                if player.inventory[*slot] == *item_id {
                                    player.counts[*slot] = player.counts[*slot].saturating_add(*count as u8).min(64);
                                    items.remove(item_eid);
                                    self_packets.push(packets::build_set_slot(0, *slot as i16, *item_id, player.counts[*slot] as i8, *damage));
                                    broadcasts.push(packets::build_collect_item(*item_eid, entity_id));
                                    broadcasts.push(packets::build_destroy_entity(*item_eid));
                                }
                            }
                        }
                    }
                    drop(items);
                    for (_, p) in players.iter() {
                        for pkt in &broadcasts { let _ = p.sender.send(pkt.clone()); }
                    }
                    if let Some(player) = players.get_mut(&entity_id) {
                        for pkt in &self_packets { let _ = player.sender.send(pkt.clone()); }
                    }
                }
            }
        });
    }

    let result = read_loop(&mut reader, &state, entity_id).await;

    {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get(&entity_id) {
            if let Err(e) = save::save_player(player) {
                println!("Error saving player {}: {e}", player.username);
            } else {
                println!("Saved player data for {}", player.username);
            }
        }
        players.remove(&entity_id);

        let list_packet = packets::build_player_list_item(&username, false);
        let destroy_packet = packets::build_destroy_entity(entity_id);
        for other in players.values() {
            let _ = other.sender.send(list_packet.clone());
            let _ = other.sender.send(destroy_packet.clone());
        }
    }
    println!("{username} left the server");

    result
}

async fn read_loop(
    reader: &mut OwnedReadHalf,
    state: &SharedState,
    entity_id: i32,
) -> std::io::Result<()> {
    loop {
        let (id, data) = read_packet(reader).await?;

        if id == 1 {
            let mut idx = 0;
            let message = read_string_buf(&data, &mut idx);
            let is_command = message.starts_with('/');

            if is_command {
                crate::game::commands::handle_player_command(state, entity_id, &message).await;
            } else {
                let (username, packet) = {
                    let players = state.players.lock().await;
                    let username = players.get(&entity_id).map(|j| j.username.clone()).unwrap_or_default();
                    let chat = format!("<{}> {}", username, message);
                    let json = format!("{{\"text\":\"{}\"}}", chat.replace('\\', "\\\\").replace('"', "\\\""));
                    (username, packets::build_chat(&json))
                };
                println!("{username}: {message}");
                let players = state.players.lock().await;
                for other in players.values() {
                    let _ = other.sender.send(packet.clone());
                }
            }
        } else if id == 4 || id == 5 || id == 6 {
            let mut idx = 0;
            let mut players = state.players.lock().await;
            let mut just_injured = false;
            if let Some(player) = players.get_mut(&entity_id) {
                if id == 4 || id == 6 {
                    player.x = read_f64_buf(&data, &mut idx);
                    player.y = read_f64_buf(&data, &mut idx);
                    let _stance = read_f64_buf(&data, &mut idx);
                    player.z = read_f64_buf(&data, &mut idx);
                }
                if id == 5 || id == 6 {
                    player.yaw = read_f32_buf(&data, &mut idx);
                    player.pitch = read_f32_buf(&data, &mut idx);
                }
                let on_ground = data.len() > idx && read_u8_buf(&data, &mut idx) != 0;
                if !on_ground && player.y > player.highest_y {
                    player.highest_y = player.y;
                }
                if on_ground && player.highest_y - player.y > 3.0 && player.gamemode != 1 && player.health > 0.0 {
                    let distance = player.highest_y - player.y;
                    let old_health = player.health;
                    player.health = (player.health - ((distance - 3.0) * 2.0) as f32).max(0.0);
                    just_injured = player.health < old_health;
                }
                if on_ground {
                    player.highest_y = player.y;
                }
                let packet_tp = packets::build_entity_teleport(player);
                let packet_head = packets::build_entity_head_look(player);
                for (other_id, other) in players.iter() {
                    if *other_id != entity_id {
                        let _ = other.sender.send(packet_tp.clone());
                        let _ = other.sender.send(packet_head.clone());
                    }
                }
            }
            drop(players);
            update_chunks_for_player(state, entity_id).await;
            if just_injured {
                let players = state.players.lock().await;
                let (is_dead, health_packet) = {
                    let player = &players[&entity_id];
                    (player.health <= 0.0, packets::build_update_health(player.health, 20i16, 0.0))
                };
                let status_packet = packets::build_entity_status(entity_id, if is_dead { 3 } else { 2 });
                for (other_id, other) in players.iter() {
                    if *other_id != entity_id {
                        let _ = other.sender.send(status_packet.clone());
                    }
                }
                if let Some(player) = players.get(&entity_id) {
                    let _ = player.sender.send(health_packet);
                }
            }
        } else if id == 0x02 {
            let mut idx = 0;
            let target_id = read_i32_buf(&data, &mut idx);
            let action = read_u8_buf(&data, &mut idx);
            println!("[USE ENTITY] entity={entity_id} target={target_id} action={action}");
            if action == 1 {
                let mut players = state.players.lock().await;
                let (attacker_creative, target_survival) = {
                    let att = players.get(&entity_id).map(|p| p.gamemode == 1).unwrap_or(false);
                    let tgt = players.get(&target_id).map(|p| p.gamemode != 1).unwrap_or(false);
                    (att, tgt)
                };
                let target_health = if attacker_creative || target_survival {
                    players.get_mut(&target_id).and_then(|t| {
                        if t.gamemode == 1 { None } else {
                            t.health = (t.health - 2.0).max(0.0);
                            Some(t.health)
                        }
                    })
                } else {
                    None
                };
                if let Some(hp) = target_health {
                    let status_packet = packets::build_entity_status(target_id, if hp <= 0.0 { 3 } else { 2 });
                    let health_packet = packets::build_update_health(hp, 20i16, 0.0);
                    for (other_id, other) in players.iter() {
                        if *other_id != target_id {
                            let _ = other.sender.send(status_packet.clone());
                        }
                    }
                    if let Some(target) = players.get(&target_id) {
                        let _ = target.sender.send(health_packet);
                    }
                }
            }
        } else if id == 0x0A {
            let mut idx = 0;
            let _anim_entity_id = read_i32_buf(&data, &mut idx);
            let animation = read_u8_buf(&data, &mut idx);
            if animation == 104 || animation == 105 {
                let sneaking = animation == 104;
                let mut players = state.players.lock().await;
                if let Some(player) = players.get_mut(&entity_id) {
                    player.sneaking = sneaking;
                }
                drop(players);
                let pkt = packets::build_entity_metadata_flags(entity_id, sneaking);
                let players = state.players.lock().await;
                for (other_id, other) in players.iter() {
                    if *other_id != entity_id {
                        let _ = other.sender.send(pkt.clone());
                    }
                }
            }
        } else if id == 0x0B {
            let mut idx = 0;
            let _target = read_i32_buf(&data, &mut idx);
            let action = read_u8_buf(&data, &mut idx);
            if action == 1 || action == 2 {
                let sneaking = action == 1;
                let mut players = state.players.lock().await;
                if let Some(player) = players.get_mut(&entity_id) {
                    player.sneaking = sneaking;
                }
                drop(players);
                let pkt = packets::build_entity_metadata_flags(entity_id, sneaking);
                let players = state.players.lock().await;
                for (other_id, other) in players.iter() {
                    if *other_id != entity_id {
                        let _ = other.sender.send(pkt.clone());
                    }
                }
            }
        } else if id == 7 {
            let mut idx = 0;
            let status = read_u8_buf(&data, &mut idx);
            let x = read_i32_buf(&data, &mut idx);
            let y = read_u8_buf(&data, &mut idx);
            let z = read_i32_buf(&data, &mut idx);
            let face = read_u8_buf(&data, &mut idx);
            println!("[DIG] status={status} pos=({x},{y},{z}) face={face}");

            if status == 3 || status == 4 {
                let dropped = {
                    let mut players = state.players.lock().await;
                    if let Some(player) = players.get_mut(&entity_id) {
                        let held_idx = 36 + player.selected_slot;
                        let item = player.inventory[held_idx];
                        let current_count = player.counts[held_idx].max(if item >= 0 { 1 } else { 0 });
                        println!("[DROP] held_idx={held_idx} item={item} count={current_count} status={status}");
                        if item >= 0 && current_count > 0 {
                            let drop_count = if status == 3 { current_count } else { 1 };
                            let remaining = current_count.saturating_sub(drop_count);
                            if remaining == 0 {
                                player.inventory[held_idx] = -1;
                                player.counts[held_idx] = 0;
                            } else {
                                player.counts[held_idx] = remaining;
                            }
                            let yaw_rad = (player.yaw as f64).to_radians();
                            let pitch_rad = (player.pitch as f64).to_radians();
                            let dir_x = -pitch_rad.cos() * yaw_rad.sin();
                            let dir_y = -pitch_rad.sin();
                            let dir_z = pitch_rad.cos() * yaw_rad.cos();
                            let drop_x = player.x + dir_x * 0.5;
                            let drop_y = player.y + 1.2 + dir_y * 0.3;
                            let drop_z = player.z + dir_z * 0.5;
                            let vel_x = (dir_x * 2400.0) as i16;
                            let vel_y = (dir_y * 2400.0 + 600.0) as i16;
                            let vel_z = (dir_z * 2400.0) as i16;
                            Some((item, held_idx as i16, drop_x, drop_y, drop_z, drop_count, remaining, vel_x, vel_y, vel_z))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some((item, held_slot, px, py, pz, drop_count, remaining, vel_x, vel_y, vel_z)) = dropped {
                    println!("[DROP] spawning item={item} count={drop_count} at ({px:.1},{py:.1},{pz:.1})");
                    let set_slot = if remaining > 0 {
                        packets::build_set_slot(0, held_slot, item, remaining as i8, 0)
                    } else {
                        packets::build_set_slot(0, held_slot, -1, 1, 0)
                    };
                    {
                        let players = state.players.lock().await;
                        if let Some(player) = players.get(&entity_id) {
                            let _ = player.sender.send(set_slot);
                        }
                    }
                    items::spawn_item_entity(state, item, drop_count as i8, 0, px, py, pz, vel_x, vel_y, vel_z).await;
                    crate::game::inventory::broadcast_equipment(state, entity_id).await;
                }
            }

            let (is_creative, _) = {
                let players = state.players.lock().await;
                let gamemode = players.get(&entity_id).map(|j| j.gamemode).unwrap_or(1);
                (gamemode == 1, gamemode)
            };
            let should_break = status == 2 || (status == 0 && is_creative);
            if should_break {
                let old_block = {
                    let mut world = state.world.lock().await;
                    let block = get_block(&world, x, y as i32, z);
                    world.insert((x, y as i32, z), 0);
                    block
                };
                let mut packets = vec![packets::build_block_change(x, y, z, 0)];
                let block_id = old_block & 0xFFF;
                let meta = ((old_block >> 12) & 0x0F) as u8;
                if block_id == 64 || block_id == 71 {
                    let yi = y as i32;
                    let other_yi = if (meta & 0x08) != 0 {
                        yi - 1
                    } else if yi < 255 {
                        yi + 1
                    } else {
                        0
                    };
                    if other_yi >= 0 && other_yi <= 255 && other_yi != yi {
                        let mut world = state.world.lock().await;
                        world.insert((x, other_yi, z), 0);
                        packets.push(packets::build_block_change(x, other_yi as u8, z, 0));
                        crate::game::redstone::schedule_update(state, x, other_yi, z).await;
                    }
                }
                // The piston base and its head are one logical block.  Break
                // either one and remove the other half as well (without a
                // duplicate head item drop).
                let piston_offset = |face: u8| -> (i32, i32, i32) {
                    match face & 0x07 {
                        0 => (0, -1, 0), 1 => (0, 1, 0),
                        2 => (0, 0, -1), 3 => (0, 0, 1),
                        4 => (-1, 0, 0), 5 => (1, 0, 0), _ => (0, 0, 0),
                    }
                };
                if block_id == 29 || block_id == 33 {
                    let (dx, dy, dz) = piston_offset(meta);
                    let head = (x + dx, y as i32 + dy, z + dz);
                    let removed_head = {
                        let mut world = state.world.lock().await;
                        if (get_block(&world, head.0, head.1, head.2) & 0xFFF) == 34 {
                            world.insert(head, 0);
                            true
                        } else { false }
                    };
                    if removed_head {
                        packets.push(packets::build_block_change(head.0, head.1 as u8, head.2, 0));
                        crate::game::redstone::schedule_update(state, head.0, head.1, head.2).await;
                    }
                } else if block_id == 34 {
                    let (dx, dy, dz) = piston_offset(meta);
                    let base = (x - dx, y as i32 - dy, z - dz);
                    let removed_base = {
                        let mut world = state.world.lock().await;
                        let candidate = get_block(&world, base.0, base.1, base.2);
                        if matches!(candidate & 0xFFF, 29 | 33) {
                            world.insert(base, 0);
                            true
                        } else { false }
                    };
                    if removed_base {
                        packets.push(packets::build_block_change(base.0, base.1 as u8, base.2, 0));
                        crate::game::redstone::schedule_update(state, base.0, base.1, base.2).await;
                    }
                }
                notify_neighbors(state, x, y as i32, z).await;
                crate::game::redstone::schedule_update(state, x, y as i32, z).await;
                let players = state.players.lock().await;
                for (other_id, other) in players.iter() {
                    for pkt in &packets {
                        if *other_id != entity_id || pkt != &packets[0] {
                            let _ = other.sender.send(pkt.clone());
                        }
                    }
                }
                if !is_creative && old_block != 0 && block_id != 34 {
                    let item_id = packets::block_to_item(old_block);
                    if item_id >= 0 {
                        drop(players);
                        items::spawn_item_entity(
                            state,
                            item_id,
                            1,
                            0,
                            x as f64 + 0.5,
                            y as f64 + 0.5,
                            z as f64 + 0.5,
                            0, 0, 0,
                        ).await;
                    }
                }
            }
        } else if id == 8 {
            let mut idx = 0;
            let x = read_i32_buf(&data, &mut idx);
            let y = read_u8_buf(&data, &mut idx);
            let z = read_i32_buf(&data, &mut idx);
            let face = read_u8_buf(&data, &mut idx);
            let (item_id, _, damage) = read_slot_full(&data, &mut idx);
            let _cursor_x = read_u8_buf(&data, &mut idx);
            let cursor_y = read_u8_buf(&data, &mut idx);
            let _cursor_z = read_u8_buf(&data, &mut idx);

            if face >= 6 {
                if item_id >= 0 {
                    handle_item_use(state, entity_id, item_id).await;
                }
            } else {
                let (nx, ny, nz) = face_offset(x, y, z, face);
                if ny >= 0 && ny <= 255 {
                    let clicked_block = {
                        let world = state.world.lock().await;
                        get_block(&world, x, y as i32, z)
                    };

                    if is_interactive_block(clicked_block) {
                        // Compute the toggle and mutate the world, then DROP the world
                        // lock before calling notify_neighbors (which itself locks
                        // state.world) to avoid a self-deadlock.
                        let toggled: Option<(i32, i32, i32, u16)> = {
                            let mut world = state.world.lock().await;
                            if let Some(&stored) = world.get(&(x, y as i32, z)) {
                                let block_id = stored & 0xFFF;
                                let meta = ((stored >> 12) & 0x0F) as u8;
                                if (block_id == 64 || block_id == 71) && (meta & 0x08) != 0 && y > 0 {
                                    let bottom = get_block(&world, x, y as i32 - 1, z);
                                    toggle_block(bottom).map(|new_stored| {
                                        world.insert((x, y as i32 - 1, z), new_stored);
                                        (x, y as i32 - 1, z, new_stored)
                                    })
                                } else {
                                    toggle_block(stored).map(|new_stored| {
                                        world.insert((x, y as i32, z), new_stored);
                                        (x, y as i32, z, new_stored)
                                    })
                                }
                            } else {
                                None
                            }
                        };

                        if let Some((bx, by, bz, new_stored)) = toggled {
                            let pkt = packets::build_block_change(bx, by as u8, bz, new_stored);
                            notify_neighbors(state, bx, by, bz).await;
                            crate::game::redstone::schedule_update(state, bx, by, bz).await;
                            // Buttons are momentary switches.  The delayed
                            // queue is also used for repeaters, so it can
                            // restore their unpressed state without another
                            // client action (stone: 1 s, wood: 1.5 s).
                            let toggled_id = new_stored & 0xFFF;
                            let toggled_meta = ((new_stored >> 12) & 0x0F) as u8;
                            if matches!(toggled_id, 77 | 143) && (toggled_meta & 0x08) != 0 {
                                let delay = if toggled_id == 77 { 20 } else { 30 };
                                let due = state.tick_counter.load(Ordering::SeqCst) + delay;
                                let released = (toggled_id as u16) | (((toggled_meta & !0x08) as u16) << 12);
                                state.redstone_delayed.lock().await.push_back((due, bx, by, bz, released));
                            }
                            let players = state.players.lock().await;
                            for (_, other) in players.iter() {
                                let _ = other.sender.send(pkt.clone());
                            }
                        }
                    } else if item_id >= 0 {
                        let (yaw, pitch) = {
                            let players = state.players.lock().await;
                            players.get(&entity_id).map(|p| (p.yaw, p.pitch)).unwrap_or((0.0, 0.0))
                        };
                        let mut block_id = item_to_block_id(item_id);
                        if block_id == 63 && (2..=5).contains(&face) {
                            block_id = 68;
                        }
                        if !is_valid_placeable_block(block_id) {
                            let players = state.players.lock().await;
                            if let Some(player) = players.get(&entity_id) {
                                let _ = player.sender.send(packets::build_disconnect("{\"text\":\"Invalid block\"}"));
                            }
                            continue;
                        }
                        if !can_place_on(block_id, face, clicked_block) {
                            continue;
                        }
                        {
                            let world = state.world.lock().await;
                            if get_block(&world, nx, ny, nz) != 0 {
                                continue;
                            }
                        }
                        let is_bucket = item_id == 326 || item_id == 327;
                        let is_door = block_id == 64 || block_id == 71;
                        let meta = block_metadata(block_id, face, yaw, pitch, cursor_y, damage as u8);
                        let stored = (block_id as u16) | ((meta as u16) << 12);

                        {
                            let mut world = state.world.lock().await;
                            world.insert((nx, ny, nz), stored);
                        }
                        notify_neighbors(state, nx, ny, nz).await;
                        crate::game::redstone::schedule_update(state, nx, ny, nz).await;

                        let mut packets_to_broadcast: Vec<Vec<u8>> = Vec::new();
                        packets_to_broadcast.push(packets::build_block_change(nx, ny as u8, nz, stored));

                        if is_door && ny < 255 {
                            let hinge_right = {
                                let world = state.world.lock().await;
                                let right_of = [(0i32, 0, -1), (-1, 0, 0), (0, 0, 1), (1, 0, 0)];
                                let left_of = [(0i32, 0, 1), (1, 0, 0), (0, 0, -1), (-1, 0, 0)];
                                let (rx, _, rz) = right_of[meta as usize];
                                let (lx, _, lz) = left_of[meta as usize];
                                let right = get_block(&world, nx + rx, ny, nz + rz);
                                let left = get_block(&world, nx + lx, ny, nz + lz);
                                let right_id = right & 0xFFF;
                                let left_id = left & 0xFFF;
                                let right_solid = right != 0 && right_id != 64 && right_id != 71;
                                let left_solid = left != 0 && left_id != 64 && left_id != 71;
                                if right_solid && !left_solid {
                                    false
                                } else if left_id == 64 || left_id == 71 {
                                    let left_top = get_block(&world, nx + lx, ny + 1, nz + lz);
                                    if ((left_top >> 12) & 0x0F) as u8 & 0x01 != 0 {
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                }
                            };
                            let top_meta: u16 = if hinge_right { 0x08 } else { 0x09 };
                            let top_stored = (block_id as u16) | (top_meta << 12);
                            {
                                let mut world = state.world.lock().await;
                                world.insert((nx, ny + 1, nz), top_stored);
                            }
                            packets_to_broadcast.push(packets::build_block_change(nx, (ny + 1) as u8, nz, top_stored));
                        }

                        let mut players = state.players.lock().await;
                        for (_, other) in players.iter() {
                            for pkt in &packets_to_broadcast {
                                let _ = other.sender.send(pkt.clone());
                            }
                        }

                        if let Some(player) = players.get_mut(&entity_id) {
                            if player.gamemode != 1 {
                                let held_idx = 36 + player.selected_slot;
                                if is_bucket {
                                    if held_idx < 45 && player.inventory[held_idx] == item_id {
                                        player.inventory[held_idx] = 325;
                                        player.counts[held_idx] = 1;
                                        let _ = player.sender.send(packets::build_set_slot(0, held_idx as i16, 325, 1, 0));
                                    }
                                } else if held_idx < 45 && player.inventory[held_idx] == item_id && player.counts[held_idx] > 0 {
                                    player.counts[held_idx] -= 1;
                                    if player.counts[held_idx] == 0 {
                                        player.inventory[held_idx] = -1;
                                    }
                                    let _ = player.sender.send(packets::build_set_slot(0, held_idx as i16, player.inventory[held_idx], player.counts[held_idx] as i8, 0));
                                }
                            }
                        }
                    }
                }
            }
        } else if id == 9 {
            let mut idx = 0;
            let slot = read_i16_buf(&data, &mut idx);

            {
                let mut players = state.players.lock().await;
                if let Some(player) = players.get_mut(&entity_id) {
                    if (0..9).contains(&slot) {
                        player.selected_slot = slot as usize;
                    }
                }
            }
            crate::game::inventory::broadcast_equipment(state, entity_id).await;
        } else if id == 0x0E {
            crate::game::inventory::handle_click_window(state, entity_id, &data).await;
        } else if id == 0x10 {
            crate::game::inventory::handle_creative_inventory(state, entity_id, &data).await;
        } else if id == 0x16 {
            let mut idx = 0;
            let action = read_u8_buf(&data, &mut idx);
            if action == 0 {
                // Le client 1.7.10 envoie ce paquet (Client Status, action 0)
                // À CHAQUE connexion, pas seulement après une vraie mort (voir
                // wiki.vg Protocol FAQ : "Client Status: sent either before or
                // while receiving chunks"). Sans ce garde-fou, tout join
                // déclenchait un faux respawn vers le point fixe (8,8) avec
                // l'astuce de double changement de dimension (pour forcer un
                // reload de chunks côté client) -> c'est ça qui causait le
                // déchargement/rechargement du monde juste après le join, la
                // téléportation vers (8,8) même en jouant ailleurs, et le fait
                // de se retrouver enterré (find_safe_spawn rappelé par-dessus
                // le spawn déjà correct fait au login).
                let is_actually_dead = {
                    let players = state.players.lock().await;
                    players.get(&entity_id).map(|p| p.health <= 0.0).unwrap_or(false)
                };
                if is_actually_dead {
                let world_snapshot = state.world.lock().await.clone();
                let (spawn_x, spawn_y, spawn_z) = find_safe_spawn(&world_snapshot, 8, 8, 17);

                let spawn_cx = (spawn_x.floor() as i32) >> 4;
                let spawn_cz = (spawn_z.floor() as i32) >> 4;
                let rmin_x = spawn_cx - VIEW_DISTANCE;
                let rmax_x = spawn_cx + VIEW_DISTANCE;
                let rmin_z = spawn_cz - VIEW_DISTANCE;
                let rmax_z = spawn_cz + VIEW_DISTANCE;
                // Même principe qu'au join : son propre chunk + sa position en
                // premier, le reste après (voir commentaire au join plus haut).
                let own_chunk_pkt = build_chunk_packet(spawn_cx, spawn_cz, &world_snapshot);
                let mut chunk_packets: Vec<Vec<u8>> = Vec::new();
                for cx in rmin_x..=rmax_x {
                    for cz in rmin_z..=rmax_z {
                        if cx == spawn_cx && cz == spawn_cz {
                            continue;
                        }
                        chunk_packets.push(build_chunk_packet(cx, cz, &world_snapshot));
                    }
                }

                let mut players = state.players.lock().await;
                if let Some(player) = players.get_mut(&entity_id) {
                    let sender = player.sender.clone();
                    let spawn_username = player.username.clone();
                    let gamemode = player.gamemode;
                    player.health = 20.0;
                    player.x = spawn_x;
                    player.y = spawn_y;
                    player.z = spawn_z;
                    player.highest_y = spawn_y;
                    player.loaded_chunks = (rmin_x..=rmax_x).flat_map(|x| (rmin_z..=rmax_z).map(move |z| (x, z))).collect();
                    let fake_dim = if gamemode == 1 { -1 } else { 1 };
                    let _ = sender.send(packets::build_respawn(fake_dim, 1, gamemode));
                    let _ = sender.send(packets::build_respawn(0, 1, gamemode));
                    let _ = sender.send(own_chunk_pkt);
                    let _ = sender.send(packets::build_player_position_look(spawn_x, spawn_y, spawn_z, 0.0, 0.0));
                    for pkt in &chunk_packets {
                        let _ = sender.send(pkt.clone());
                    }
                    let _ = sender.send(packets::build_update_health(20.0, 20i16, 0.0));
                    let destroy = packets::build_destroy_entity(entity_id);
                    let spawn = packets::build_spawn_player(player);
                    for (other_id, other) in players.iter() {
                        if *other_id != entity_id {
                            let _ = other.sender.send(destroy.clone());
                            let _ = other.sender.send(spawn.clone());
                        }
                    }
                    let _ = sender.send(packets::build_player_list_item(&spawn_username, true));
                }
                if let Some(player) = players.get(&entity_id) {
                    let sender = player.sender.clone();
                    for (other_id, other) in players.iter() {
                        if *other_id != entity_id {
                            let _ = sender.send(packets::build_player_list_item(&other.username, true));
                            let _ = sender.send(packets::build_spawn_player(other));
                            for pkt in packets::build_equipment_packets(other.entity_id, other) {
                                let _ = sender.send(pkt);
                            }
                            let _ = sender.send(packets::build_entity_metadata_flags(other.entity_id, other.sneaking));
                        }
                    }
                }
                }
            }
        }
    }
}
