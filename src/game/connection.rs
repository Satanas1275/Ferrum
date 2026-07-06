use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::items::{self, PICKUP_DELAY_TICKS};
use crate::net::reading::{read_packet, read_varint_buf, read_string_buf, read_f64_buf, read_f32_buf, read_i32_buf, read_i64_buf, read_u8_buf, read_i16_buf, read_slot};
use crate::net::writing::write_string;
use crate::packets;
use crate::player::Player;
use crate::util::{face_offset, offline_uuid};
use crate::world::{get_block, build_chunk_packet, SharedState};

/// Rayon de recherche horizontale (en blocs) autour du point de spawn.
const SPAWN_SEARCH_RADIUS: i32 = 8;

fn is_interactive_block(stored: u16) -> bool {
    matches!(stored & 0xFFF,
        23 | 25 | 26 | 54 | 58 | 61 | 62 | 64 | 69 | 71 |
        77 | 84 | 92 | 96 | 107 | 116 | 117 | 118 | 130 |
        137 | 138 | 143 | 144 | 145 | 146 | 154 | 158 | 167 |
        183 | 184 | 185 | 186
    )
}

fn toggle_block(stored: u16) -> Option<u16> {
    let block_id = stored & 0xFFF;
    let meta = ((stored >> 12) & 0x0F) as u8;
    let new_meta = match block_id as u16 {
        64 | 71 | 96 | 167 | 107 | 183 | 184 | 185 | 186 => meta ^ 4,
        69 | 143 => meta ^ 8,
        77 => meta | 8,
        _ => return None,
    };
    Some(block_id | ((new_meta as u16) << 12))
}

fn item_to_block_id(item_id: i16) -> u16 {
    match item_id {
        324 => 64,
        330 => 71,
        326 => 9,
        327 => 11,
        _ => item_id as u16,
    }
}

fn block_metadata(block_id: u16, face: u8, yaw: f32) -> u8 {
    let yaw_dir = ((yaw * 4.0 / 360.0 + 0.5).floor() as i32 & 3) as u8;
    match block_id {
        65 => match face {
            2 | 3 | 4 | 5 => face,
            _ => 2,
        },
        50 => match face {
            0 | 1 => 5,
            2 => 4,
            3 => 3,
            4 => 2,
            5 => 1,
            _ => 5,
        },
        75 | 76 => match face {
            0 | 1 => 5,
            2 => 4,
            3 => 3,
            4 => 2,
            5 => 1,
            _ => 5,
        },
        69 => match face {
            0 => 0,
            1 => 7,
            2 => 4,
            3 => 3,
            4 => 2,
            5 => 1,
            _ => 0,
        },
        77 | 143 => match face {
            0 => 0,
            1 => 5,
            2 => 4,
            3 => 3,
            4 => 2,
            5 => 1,
            _ => 0,
        },
        53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 |
        156 | 163 | 164 | 180 | 203 => {
            let stair_dir: [u8; 4] = [2, 1, 3, 0];
            let mut meta = stair_dir[yaw_dir as usize];
            if face == 0 { meta |= 4; }
            meta
        },
        54 | 61 | 62 | 130 => match face {
            2 => 2,
            3 => 3,
            4 => 5,
            5 => 4,
            _ => 2,
        },
        33 | 34 => match face {
            0 | 1 | 2 | 3 | 4 | 5 => face,
            _ => 0,
        },
        23 | 158 => match face {
            0 | 1 | 2 | 3 | 4 | 5 => face,
            _ => 0,
        },
        96 | 167 => match face {
            0 => 8,
            1 => 0,
            2 => 1,
            3 => 0,
            4 => 3,
            5 => 2,
            _ => 0,
        },
         107 | 183 | 184 | 185 | 186 => yaw_dir,
         64 | 71 => (yaw_dir + 2) & 3,
        26 => match face {
            2 => 0,
            3 => 1,
            4 => 2,
            5 => 3,
            _ => 0,
        },
        _ => 0,
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
    let is_free = |x: i32, y: i32, z: i32| {
        get_block(world, x, y, z) == 0 && get_block(world, x, y + 1, z) == 0
    };

    if is_free(cx, default_y, cz) {
        return (cx as f64, default_y as f64, cz as f64);
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
                if is_free(x, default_y, z) {
                    return (x as f64, default_y as f64, z as f64);
                }
            }
        }
    }

    // Rien trouvé horizontalement dans le rayon : on retombe sur l'ancien
    // comportement, remonter à la verticale sur la colonne centrale.
    let mut y = default_y;
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
    content.extend((spawn_y + 0.63).to_be_bytes());
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
    let chunk_min = -2i32;
    let chunk_max = 2i32;
    for x in chunk_min..=chunk_max {
        for z in chunk_min..=chunk_max {
            let pkt = build_chunk_packet(x, z, &world_snapshot);
            socket.write_all(&pkt).await?;
        }
    }

    let block_min = chunk_min * 16;
    let block_max = chunk_max * 16 + 15;
    for (&(wx, wy, wz), &block_id) in world_snapshot.iter() {
        if block_id != 0
            && wx >= block_min && wx <= block_max
            && wz >= block_min && wz <= block_max
            && wy >= 0 && wy <= 255
        {
            let packet = packets::build_block_change(wx, wy as u8, wz, block_id);
            socket.write_all(&packet).await?;
        }
    }

    // Cherche un endroit sûr pour spawn (voir find_safe_spawn : anneaux
    // horizontaux d'abord, fallback vertical seulement si tout le rayon
    // est bloqué)
    let (spawn_x, spawn_y, spawn_z) = find_safe_spawn(&world_snapshot, 8, 8, 17);

    send_spawn_position(&mut socket, spawn_x, spawn_y, spawn_z).await?;
    println!("{username} is now online!");

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let new_player = Player {
        entity_id,
        uuid: uuid.clone(),
        username: username.clone(),
        x: spawn_x,
        y: spawn_y,
        z: spawn_z,
        yaw: 0.0,
        pitch: 0.0,
        gamemode: 1,
        inventory: [-1i16; 45],
        counts: [0u8; 45],
        selected_slot: 0,
        cursor_item: -1,
        cursor_count: 0,
        health: 20.0,
        highest_y: spawn_y,
        sneaking: false,
        sender: tx.clone(),
    };

    let (mut reader, mut writer) = socket.into_split();

    tokio::spawn(async move {
        while let Some(packet) = rx.recv().await {
            if writer.write_all(&packet).await.is_err() {
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
            if just_injured {
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
                let packet = packets::build_block_change(x, y, z, 0);
                let players = state.players.lock().await;
                for (other_id, other) in players.iter() {
                    if *other_id != entity_id {
                        let _ = other.sender.send(packet.clone());
                    }
                }
                if !is_creative && old_block != 0 {
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
            let item_id = read_slot(&data, &mut idx);

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
                        let mut world = state.world.lock().await;
                        if let Some(&stored) = world.get(&(x, y as i32, z)) {
                            if let Some(new_stored) = toggle_block(stored) {
                                world.insert((x, y as i32, z), new_stored);
                                let pkt = packets::build_block_change(x, y, z, new_stored);
                                let players = state.players.lock().await;
                                for (_, other) in players.iter() {
                                    let _ = other.sender.send(pkt.clone());
                                }
                            }
                        }
                    } else if item_id >= 0 {
                        let yaw = {
                            let players = state.players.lock().await;
                            players.get(&entity_id).map(|p| p.yaw).unwrap_or(0.0)
                        };
                        let block_id = item_to_block_id(item_id);
                        let is_bucket = item_id == 326 || item_id == 327;
                        let is_door = block_id == 64 || block_id == 71;
                        let meta = block_metadata(block_id, face, yaw);
                        let stored = (block_id as u16) | ((meta as u16) << 12);

                        {
                            let mut world = state.world.lock().await;
                            world.insert((nx, ny, nz), stored);
                        }

                        let mut packets_to_broadcast: Vec<Vec<u8>> = Vec::new();
                        packets_to_broadcast.push(packets::build_block_change(nx, ny as u8, nz, stored));

                        if is_door && ny < 255 {
                            let hinge_right = {
                                let world = state.world.lock().await;
                                let right_of = [(1i32, 0, 0), (0, 0, -1), (-1, 0, 0), (0, 0, 1)];
                                let left_of = [(-1i32, 0, 0), (0, 0, 1), (1, 0, 0), (0, 0, -1)];
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
                                        false
                                    } else {
                                        true
                                    }
                                } else {
                                    true
                                }
                            };
                            let top_meta: u16 = if hinge_right { 0x09 } else { 0x08 };
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
                let world_snapshot = state.world.lock().await.clone();
                let (spawn_x, spawn_y, spawn_z) = find_safe_spawn(&world_snapshot, 8, 8, 17);

                let mut chunk_packets: Vec<Vec<u8>> = Vec::with_capacity(25);
                for x in -2..=2 {
                    for z in -2..=2 {
                        chunk_packets.push(build_chunk_packet(x, z, &world_snapshot));
                    }
                }

                let block_min = -2 * 16;
                let block_max = 2 * 16 + 15;
                let mut block_packets: Vec<Vec<u8>> = Vec::new();
                for (&(wx, wy, wz), &block_id) in world_snapshot.iter() {
                    if block_id != 0
                        && wx >= block_min && wx <= block_max
                        && wz >= block_min && wz <= block_max
                        && wy >= 0 && wy <= 255
                    {
                        block_packets.push(packets::build_block_change(wx, wy as u8, wz, block_id));
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
                    let fake_dim = if gamemode == 1 { -1 } else { 1 };
                    let _ = sender.send(packets::build_respawn(fake_dim, 1, gamemode));
                    let _ = sender.send(packets::build_respawn(0, 1, gamemode));
                    for pkt in &chunk_packets {
                        let _ = sender.send(pkt.clone());
                    }
                    for pkt in &block_packets {
                        let _ = sender.send(pkt.clone());
                    }
                    let _ = sender.send(packets::build_player_position_look(spawn_x, spawn_y, spawn_z, 0.0, 0.0));
                    let _ = sender.send(packets::build_update_health(20.0, 20i16, 0.0));
                    let destroy = packets::build_destroy_entity(entity_id);
                    let spawn = packets::build_spawn_player(player);
                    for (other_id, other) in players.iter() {
                        if *other_id != entity_id {
                            let _ = other.sender.send(destroy.clone());
                            let _ = other.sender.send(spawn.clone());
                        }
                    }
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
                    let _ = sender.send(packets::build_player_list_item(&spawn_username, true));
                }
            }
        }
    }
}