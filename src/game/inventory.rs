use crate::items;
use crate::packets;
use crate::world::SharedState;

pub async fn broadcast_equipment(state: &SharedState, entity_id: i32) {
    let packets = {
        let players = state.players.lock().await;
        match players.get(&entity_id) {
            Some(player) => packets::build_equipment_packets(entity_id, player),
            None => return,
        }
    };
    let players = state.players.lock().await;
    for (other_id, other) in players.iter() {
        if *other_id != entity_id {
            for pkt in &packets {
                let _ = other.sender.send(pkt.clone());
            }
        }
    }
}

pub async fn handle_click_window(state: &SharedState, entity_id: i32, data: &[u8]) {
    let mut idx = 0;
    let window_id = data[idx]; idx += 1;
    let slot = i16::from_be_bytes([data[idx], data[idx + 1]]); idx += 2;
    let button = data[idx]; idx += 1;
    let action_number = i16::from_be_bytes([data[idx], data[idx + 1]]); idx += 2;
    let mode_val = data[idx]; idx += 1;
    let (_reported_cursor_item, _reported_cursor_count) = {
        let item_id = i16::from_be_bytes([data[idx], data[idx + 1]]);
        let mut count = 1u8;
        if item_id >= 0 {
            count = data[idx + 2];
        }
        (item_id, count)
    };

    if window_id != 0 { return; }

    let click_result: Option<(Vec<(i16, i16, i8)>, Option<(i16, i8, f64, f64, f64, i16, i16, i16)>)> = {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(&entity_id) {
            let mut changed: Vec<(i16, i16, i8)> = Vec::new();
            let mut drop_item: Option<(i16, i8, f64, f64, f64, i16, i16, i16)> = None;
            let mode = mode_val;

            match mode {
                0 => {
                    if slot == -1 {
                        if player.cursor_item >= 0 && player.cursor_count > 0 {
                            let drop_count = if button == 0 { player.cursor_count } else { 1 };
                            let item_id = player.cursor_item;
                            let remaining = player.cursor_count.saturating_sub(drop_count);
                            if remaining == 0 {
                                player.cursor_item = -1;
                                player.cursor_count = 0;
                            } else {
                                player.cursor_count = remaining;
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
                            drop_item = Some((item_id, drop_count as i8, drop_x, drop_y, drop_z, vel_x, vel_y, vel_z));
                        }
                    } else if slot >= 0 && (slot as usize) < 45 {
                        let s = slot as usize;
                        if button == 0 {
                            let tmp_i = player.inventory[s];
                            let tmp_c = if tmp_i >= 0 { player.counts[s] } else { 0 };
                            player.inventory[s] = player.cursor_item;
                            player.counts[s] = if player.cursor_item >= 0 { player.cursor_count } else { 0 };
                            player.cursor_item = tmp_i;
                            player.cursor_count = tmp_c;
                            changed.push((slot, player.inventory[s], player.counts[s] as i8));
                        } else if button == 1 {
                            if player.cursor_item < 0 && player.inventory[s] >= 0 && player.counts[s] > 0 {
                                let total = player.counts[s];
                                let half = (total + 1) / 2;
                                player.cursor_item = player.inventory[s];
                                player.cursor_count = half;
                                if total > half {
                                    player.counts[s] = total - half;
                                } else {
                                    player.inventory[s] = -1;
                                    player.counts[s] = 0;
                                }
                                changed.push((slot, player.inventory[s], player.counts[s] as i8));
                            } else if player.cursor_item >= 0 && player.inventory[s] < 0 {
                                player.inventory[s] = player.cursor_item;
                                player.counts[s] = 1;
                                if player.cursor_count > 1 {
                                    player.cursor_count -= 1;
                                } else {
                                    player.cursor_item = -1;
                                    player.cursor_count = 0;
                                }
                                changed.push((slot, player.inventory[s], player.counts[s] as i8));
                            } else if player.cursor_item >= 0 && player.cursor_item == player.inventory[s] && player.counts[s] < 64 {
                                player.counts[s] += 1;
                                if player.cursor_count > 1 {
                                    player.cursor_count -= 1;
                                } else {
                                    player.cursor_item = -1;
                                    player.cursor_count = 0;
                                }
                                changed.push((slot, player.inventory[s], player.counts[s] as i8));
                            }
                        }
                    }
                }
                1 => {
                    if slot >= 0 && (slot as usize) < 45 {
                        let s = slot as usize;
                        if player.inventory[s] >= 0 {
                            let item = player.inventory[s];
                            let count = player.counts[s];

                            let target_slots: Vec<usize> = if (36..45).contains(&s) {
                                (9..36).collect()
                            } else if (9..36).contains(&s) {
                                (36..45).collect()
                            } else {
                                let mut v: Vec<usize> = (9..36).collect();
                                v.extend(36..45);
                                v
                            };

                            let mut remaining = count;
                            let mut moved = false;

                            for &t in &target_slots {
                                if remaining == 0 { break; }
                                if player.inventory[t] == item && player.counts[t] < 64 {
                                    let space = 64 - player.counts[t];
                                    let transfer = remaining.min(space);
                                    player.counts[t] += transfer;
                                    remaining -= transfer;
                                    moved = true;
                                    changed.push((t as i16, player.inventory[t], player.counts[t] as i8));
                                }
                            }

                            if remaining > 0 {
                                for &t in &target_slots {
                                    if remaining == 0 { break; }
                                    if player.inventory[t] < 0 {
                                        let transfer = remaining.min(64);
                                        player.inventory[t] = item;
                                        player.counts[t] = transfer;
                                        remaining -= transfer;
                                        moved = true;
                                        changed.push((t as i16, player.inventory[t], player.counts[t] as i8));
                                    }
                                }
                            }

                            if moved {
                                if remaining == 0 {
                                    player.inventory[s] = -1;
                                    player.counts[s] = 0;
                                } else {
                                    player.counts[s] = remaining;
                                }
                                changed.push((slot, player.inventory[s], player.counts[s] as i8));
                                if player.cursor_item >= 0 {
                                    player.cursor_item = -1;
                                    player.cursor_count = 0;
                                }
                            }
                        }
                    }
                }
                2 => {
                    if slot >= 0 && (slot as usize) < 45 && button < 9 {
                        let hotbar_slot = 36 + button as usize;
                        let s = slot as usize;
                        let tmp_i = player.inventory[s];
                        let tmp_c = if tmp_i >= 0 { player.counts[s] } else { 0 };
                        player.inventory[s] = player.inventory[hotbar_slot];
                        player.counts[s] = if player.inventory[hotbar_slot] >= 0 { player.counts[hotbar_slot] } else { 0 };
                        player.inventory[hotbar_slot] = tmp_i;
                        player.counts[hotbar_slot] = tmp_c;
                        changed.push((slot, player.inventory[s], player.counts[s] as i8));
                        changed.push((hotbar_slot as i16, player.inventory[hotbar_slot], player.counts[hotbar_slot] as i8));
                    }
                }
                4 => {
                    if slot >= 0 && (slot as usize) < 45 {
                        let s = slot as usize;
                        if player.inventory[s] >= 0 && player.counts[s] > 0 {
                            let item_id = player.inventory[s];
                            let drop_count = if button == 0 { 1 } else { player.counts[s] };
                            let remaining = player.counts[s].saturating_sub(drop_count);
                            if remaining == 0 {
                                player.inventory[s] = -1;
                                player.counts[s] = 0;
                            } else {
                                player.counts[s] = remaining;
                            }
                            changed.push((slot, player.inventory[s], player.counts[s] as i8));
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
                            drop_item = Some((item_id, drop_count as i8, drop_x, drop_y, drop_z, vel_x, vel_y, vel_z));
                        }
                    }
                }
                _ => {
                    if slot >= 0 && (slot as usize) < 45 {
                        changed.push((slot, player.inventory[slot as usize], player.counts[slot as usize] as i8));
                    }
                }
            }

            let confirm = packets::build_transaction_confirmation(window_id, action_number, true);
            let _ = player.sender.send(confirm);

            Some((changed, drop_item))
        } else {
            None
        }
    };

    if let Some((changed, drop_item)) = click_result {
        {
            let players = state.players.lock().await;
            if let Some(player) = players.get(&entity_id) {
                for &(s, iid, cnt) in &changed {
                    let pkt = packets::build_set_slot(window_id, s, iid, cnt, 0);
                    let _ = player.sender.send(pkt);
                }
            }
        }
        if let Some((iid, cnt, px, py, pz, vx, vy, vz)) = drop_item {
            items::spawn_item_entity(state, iid, cnt, 0, px, py, pz, vx, vy, vz).await;
        }
        broadcast_equipment(state, entity_id).await;
    }
}

pub async fn handle_creative_inventory(state: &SharedState, entity_id: i32, data: &[u8]) {
    let mut idx = 0;
    let slot = i16::from_be_bytes([data[idx], data[idx + 1]]); idx += 2;
    let (item_id, count) = {
        let item_id = i16::from_be_bytes([data[idx], data[idx + 1]]);
        let mut count = 1u8;
        if item_id >= 0 {
            count = data[idx + 2];
        }
        (item_id, count)
    };

    let is_creative = {
        let players = state.players.lock().await;
        players.get(&entity_id).map(|p| p.gamemode == 1).unwrap_or(false)
    };
    if !is_creative { return; }

    if slot == -999 && item_id >= 0 {
        let player_pos = {
            let players = state.players.lock().await;
            players.get(&entity_id).map(|p| (p.x, p.y, p.z))
        };
        if let Some((px, py, pz)) = player_pos {
            items::spawn_item_entity(state, item_id, count as i8, 0, px, py, pz, 0, 0, 0).await;
        }
    } else if slot >= 0 && (slot as usize) < 45 {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(&entity_id) {
            player.inventory[slot as usize] = item_id;
            player.counts[slot as usize] = if item_id >= 0 { count } else { 0 };
        }
        drop(players);
        broadcast_equipment(state, entity_id).await;
    } else if slot == -1 {
        let mut players = state.players.lock().await;
        if let Some(player) = players.get_mut(&entity_id) {
            player.cursor_item = item_id;
            player.cursor_count = if item_id >= 0 { count } else { 0 };
        }
    }
}
