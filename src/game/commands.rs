use std::time::Duration;

use crate::packets;
use crate::util::{get_process_ram_mb, parse_rel_coord};
use crate::world::SharedState;

pub fn teleport_entity(entity_id: i32, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, state: &SharedState) {
    let packet_view = packets::build_entity_teleport_pos(entity_id, x, y, z, yaw, pitch);
    let packet_self = packets::build_player_position_look(x, y, z, yaw, pitch);
    let mut players = state.players.lock().unwrap();
    if let Some(player) = players.get_mut(&entity_id) {
        player.x = x;
        player.y = y;
        player.z = z;
        player.yaw = yaw;
        player.pitch = pitch;
        let _ = player.sender.send(packet_self);
    }
    for (other_id, other) in players.iter() {
        if *other_id != entity_id {
            let _ = other.sender.send(packet_view.clone());
        }
    }
}

pub fn handle_player_command(state: &SharedState, entity_id: i32, message: &str) {
    let parts: Vec<&str> = message.split_whitespace().collect();
    let command = parts[0];
    match command {
        "/gamemode" => {
            let response = if parts.len() > 1 {
                let mode = match parts[1] {
                    "0" | "survival" => 0u8,
                    "1" | "creative" => 1u8,
                    "2" | "adventure" => 2u8,
                    "3" | "spectator" => 3u8,
                    _ => 255u8,
                };
                if mode <= 3 {
                    let mode_packet = packets::build_game_mode_change(mode);
                    let mut players = state.players.lock().unwrap();
                    if let Some(player) = players.get_mut(&entity_id) {
                        player.gamemode = mode;
                        let _ = player.sender.send(mode_packet);
                    }
                    format!("§aGamemode changed to §f{}", parts[1])
                } else {
                    "§cInvalid mode. Use: 0=survival, 1=creative, 2=adventure, 3=spectator".to_string()
                }
            } else {
                "§cUsage: /gamemode <mode>".to_string()
            };
            let packet = packets::build_chat(&format!("{{\"text\":\"{response}\"}}"));
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                let _ = player.sender.send(packet);
            }
        }
        "/help" => {
            let lines = vec![
                "§6Commands: §f/gamemode <mode> §7- Change game mode",
                "§f/tp [<player>|<@a>] [<x> <y> <z>|<player>] §7- Teleport (use ~ §7for relative coords)",
                "§f/tps §7- Show ticks per second",
                "§f/load §7- Show RAM and CPU usage",
                "§f/help §7- This help",
            ];
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                for line in lines {
                    let packet = packets::build_chat(&format!("{{\"text\":\"{line}\"}}"));
                    let _ = player.sender.send(packet);
                }
            }
        }
        "/tp" | "/teleport" => {
            let response = if parts.len() == 2 {
                let dest_name = parts[1];
                let dest = {
                    let players = state.players.lock().unwrap();
                    players.values().find(|p| p.username == dest_name)
                        .map(|p| (p.x, p.y, p.z, p.yaw, p.pitch))
                };
                if let Some((dx, dy, dz, dyaw, dpitch)) = dest {
                    teleport_entity(entity_id, dx, dy, dz, dyaw, dpitch, state);
                    format!("§aTeleported to §f{dest_name}")
                } else {
                    format!("§cPlayer '{dest_name}' not found")
                }
            } else if parts.len() == 3 {
                let target_name = parts[1];
                let dest_name = parts[2];
                let (target_ids, dest_pos) = {
                    let players = state.players.lock().unwrap();
                    let ids: Vec<i32> = if target_name == "@a" {
                        players.keys().copied().collect()
                    } else {
                        players.values().find(|p| p.username == target_name).map(|p| p.entity_id).into_iter().collect()
                    };
                    let pos = players.values().find(|p| p.username == dest_name)
                        .map(|p| (p.x, p.y, p.z, p.yaw, p.pitch));
                    (ids, pos)
                };
                if let Some((dx, dy, dz, dyaw, dpitch)) = dest_pos {
                    for id in &target_ids {
                        teleport_entity(*id, dx, dy, dz, dyaw, dpitch, state);
                    }
                    if target_name == "@a" {
                        format!("§aAll players teleported to §f{dest_name}")
                    } else {
                        format!("§aPlayer teleported to §f{dest_name}")
                    }
                } else {
                    format!("§cPlayer '{dest_name}' not found")
                }
            } else if parts.len() == 4 {
                let players = state.players.lock().unwrap();
                let response = if let Some(player) = players.get(&entity_id) {
                    let cx = player.x; let cy = player.y; let cz = player.z;
                    drop(players);
                    match (parse_rel_coord(&parts[1], cx), parse_rel_coord(&parts[2], cy), parse_rel_coord(&parts[3], cz)) {
                        (Some(x), Some(y), Some(z)) => {
                            let tx = crate::util::center_coord(x);
                            let tz = crate::util::center_coord(z);
                            teleport_entity(entity_id, tx, y, tz, 0.0, 0.0, state);
                            format!("§aTeleported to §f({tx:.1}, {y:.1}, {tz:.1})")
                        }
                        _ => "§cInvalid coordinates".to_string(),
                    }
                } else {
                    "§cPlayer not found".to_string()
                };
                response
            } else if parts.len() == 5 {
                let (target_str, x_str, y_str, z_str) = (parts[1], parts[2], parts[3], parts[4]);
                let players = state.players.lock().unwrap();
                let response = if target_str == "@a" {
                    let cx = players.values().next().map(|p| p.x).unwrap_or(0.0);
                    let cy = players.values().next().map(|p| p.y).unwrap_or(0.0);
                    let cz = players.values().next().map(|p| p.z).unwrap_or(0.0);
                    let ids: Vec<i32> = players.keys().copied().collect();
                    drop(players);
                    match (parse_rel_coord(x_str, cx), parse_rel_coord(y_str, cy), parse_rel_coord(z_str, cz)) {
                        (Some(x), Some(y), Some(z)) => {
                            let tx = crate::util::center_coord(x);
                            let tz = crate::util::center_coord(z);
                            for id in &ids {
                                teleport_entity(*id, tx, y, tz, 0.0, 0.0, state);
                            }
                            format!("§aAll teleported to §f({tx:.1}, {y:.1}, {tz:.1})")
                        }
                        _ => "§cInvalid coordinates".to_string(),
                    }
                } else if let Some(target) = players.values().find(|p| p.username == target_str) {
                    let cx = target.x; let cy = target.y; let cz = target.z;
                    let target_id = target.entity_id;
                    drop(players);
                    match (parse_rel_coord(x_str, cx), parse_rel_coord(y_str, cy), parse_rel_coord(z_str, cz)) {
                        (Some(x), Some(y), Some(z)) => {
                            let tx = crate::util::center_coord(x);
                            let tz = crate::util::center_coord(z);
                            teleport_entity(target_id, tx, y, tz, 0.0, 0.0, state);
                            format!("§a{target_str} teleported to §f({tx:.1}, {y:.1}, {tz:.1})")
                        }
                        _ => "§cInvalid coordinates".to_string(),
                    }
                } else {
                    format!("§cPlayer '{target_str}' not found")
                };
                response
            } else {
                "§cUsage: /tp <x> <y> <z> or /tp <player> <x> <y> <z> or /tp <player> <target>".to_string()
            };
            let packet = packets::build_chat(&format!("{{\"text\":\"{response}\"}}"));
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                let _ = player.sender.send(packet);
            }
        }
        "/tps" => {
            let tps = state.tps.lock().unwrap();
            let tps_5s = tps.tps(Duration::from_secs(5));
            let tps_30s = tps.tps(Duration::from_secs(30));
            let tps_5min = tps.tps(Duration::from_secs(300));
            let tps_15min = tps.tps(Duration::from_secs(900));
            drop(tps);
            let lines = vec![
                format!("§6TPS (5s):   §f{tps_5s:.1}"),
                format!("§6TPS (30s):  §f{tps_30s:.1}"),
                format!("§6TPS (5m):   §f{tps_5min:.1}"),
                format!("§6TPS (15m):  §f{tps_15min:.1}"),
            ];
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                for line in &lines {
                    let packet = packets::build_chat(&format!("{{\"text\":\"{line}\"}}"));
                    let _ = player.sender.send(packet);
                }
            }
        }
        "/load" => {
            let ram = get_process_ram_mb();
            let cpu = state.tps.lock().unwrap().cpu();
            let lines = vec![
                format!("§6RAM: §f{ram:.1} MB"),
                format!("§6CPU: §f{cpu:.1}%"),
            ];
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                for line in &lines {
                    let packet = packets::build_chat(&format!("{{\"text\":\"{line}\"}}"));
                    let _ = player.sender.send(packet);
                }
            }
        }
        _ => {
            let response = format!("§cUnknown command: {}", command);
            let packet = packets::build_chat(&format!("{{\"text\":\"{response}\"}}"));
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                let _ = player.sender.send(packet);
            }
        }
    }
}
