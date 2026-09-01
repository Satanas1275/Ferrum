use std::time::Duration;

use crate::packets;
use crate::util::{get_process_ram_mb, parse_rel_coord};
use crate::world::SharedState;
use crate::worldgen::biomes::{self, Biome};

/// Recherche du biome le plus proche en anneaux carrés croissants autour
/// du joueur (échantillonnage tous les 16 blocs via preview_column, donc
/// SANS génération de chunk ; portée max 1536 blocs).
fn locate_nearest_biome(state: &SharedState, entity_id: i32, target: Biome) -> Option<((i32, i32, i32), i32)> {
    let (px, pz) = {
        let players = state.players.lock().unwrap();
        players.get(&entity_id).map(|p| (p.x as i32, p.z as i32)).unwrap_or((0, 0))
    };
    let generator = &state.generator;
    let check = |x: i32, z: i32| -> Option<((i32, i32, i32), i32)> {
        let col = generator.preview_column(x, z);
        if col.biome == target {
            let dist = (((x - px).pow(2) + (z - pz).pow(2)) as f64).sqrt() as i32;
            Some(((x, col.height + 1, z), dist))
        } else {
            None
        }
    };
    if let Some(hit) = check(px, pz) {
        return Some(hit);
    }
    for ring in 1..=96i32 {
        let r = ring * 16;
        for x in (px - r..=px + r).step_by(16) {
            for &z in &[pz - r, pz + r] {
                if let Some(hit) = check(x, z) {
                    return Some(hit);
                }
            }
        }
        for z in (pz - r + 16..pz + r).step_by(16) {
            for &x in &[px - r, px + r] {
                if let Some(hit) = check(x, z) {
                    return Some(hit);
                }
            }
        }
    }
    None
}

pub fn teleport_entity(entity_id: i32, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, state: &SharedState) {
    // Même compensation Y que le spawn (cf. le hack +2 au join) : sans elle,
    // le joueur apparaît 2 blocs trop bas après une téléportation.
    let y = y + 2.0;
    let packet_view = packets::build_entity_teleport_pos(entity_id, x, y, z, yaw, pitch);
    let packet_self = packets::build_player_position_look(x, y, z, yaw, pitch);

    {
        let mut players = state.players.lock().unwrap();
        if let Some(player) = players.get_mut(&entity_id) {
            player.x = x;
            player.y = y;
            player.z = z;
            player.yaw = yaw;
            player.pitch = pitch;
            // Force un rechargement des chunks même si le client renvoie sa
            // position après coup.
            player.last_chunk = (i32::MIN, i32::MIN);
        }
    }

    // Recharge les chunks autour de la nouvelle position AVANT d'envoyer la
    // position (comme au spawn : son chunk d'abord, le reste ensuite), sinon
    // un tp loin fait tomber le client dans le vide le temps que le monde se
    // charge.
    crate::game::connection::update_chunks_for_player(state, entity_id);

    {
        let mut players = state.players.lock().unwrap();
        if let Some(player) = players.get_mut(&entity_id) {
            let _ = player.sender.send(packet_self);
        }
        for (other_id, other) in players.iter() {
            if *other_id != entity_id {
                let _ = other.sender.send(packet_view.clone());
            }
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
            let lines: Vec<String> = if parts.len() >= 2 {
                // Aide contextuelle : /help <commande> [sujet]
                let topic = parts[1];
                match topic {
                    "gamemode" => vec![
                        "§6/gamemode <mode>".to_string(),
                        "§7Modes: §f0§7=survival, §f1§7=creative, §f2§7=adventure, §f3§7=spectator".to_string(),
                        "§7Aliases acceptés: survival, creative, adventure, spectator".to_string(),
                    ],
                    "tp" | "teleport" => vec![
                        "§6/tp <x> <y> <z> §7- se téléporter (~ = coordonnée relative)".to_string(),
                        "§6/tp <player> <x> <y> <z> §7- téléporter un joueur".to_string(),
                        "§6/tp <player> <target> §7- téléporter vers un joueur".to_string(),
                        "§6/tp @a <x> <y> <z> §7- téléporter tous les joueurs".to_string(),
                    ],
                    "locate" => {
                        if parts.len() >= 3 && parts[2] == "biome" {
                            let mut lines: Vec<String> =
                                vec!["§6Biomes disponibles pour /locate biome :".to_string()];
                            for &b in biomes::ALL.iter() {
                                lines.push(format!("§f- {} §7({})", biomes::display_name(b), biomes::params(b).name));
                            }
                            lines
                        } else if parts.len() >= 3 && parts[2] == "structure" {
                            vec![
                                "§6/locate structure".to_string(),
                                "§7Aucune structure n'existe encore dans ce monde.".to_string(),
                            ]
                        } else {
                            vec![
                                "§6/locate biome <name> §7- biome le plus proche (1536 blocs max)".to_string(),
                                "§6/locate structure §7- recherche de structure".to_string(),
                                "§7Liste des biomes: §f/help locate biome".to_string(),
                            ]
                        }
                    }
                    "tps" => vec!["§6/tps §7- TPS sur 5s, 30s, 5m et 15m".to_string()],
                    "load" => vec!["§6/load §7- RAM et CPU du processus serveur".to_string()],
                    _ => vec![format!("§cCommande inconnue: /{topic}")],
                }
            } else {
                vec![
                    "§6Commands: §f/gamemode <mode> §7- Change game mode",
                    "§f/tp [<player>|<@a>] [<x> <y> <z>|<player>] §7- Teleport (use ~ §7for relative coords)",
                    "§f/locate biome <name> §7- Show the nearest biome of that type",
                    "§f/locate structure §7- Locate a structure",
                    "§f/tps §7- Show ticks per second",
                    "§f/load §7- Show RAM and CPU usage",
                    "§7Type §f/help <command> §7for details (ex: §f/help locate biome§7)",
                ]
                .into_iter()
                .map(String::from)
                .collect()
            };
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
        "/locate" => {
            let lines: Vec<String> = if parts.len() >= 2 && parts[1] == "structure" {
                // Aucune structure générée pour l'instant dans ce monde.
                vec!["§7There are no structures in this world yet.".to_string()]
            } else if parts.len() >= 3 && parts[1] == "biome" {
                let query = parts[2..].join("_");
                match biomes::by_name(&query) {
                    Some(target) => match locate_nearest_biome(state, entity_id, target) {
                        Some(((x, y, z), dist)) => {
                            let name = biomes::display_name(target);
                            vec![format!(
                                "§aThe nearest §f{name}§a is at §f[{x}, {y}, {z}]§7 ({dist} blocks away)"
                            )]
                        }
                        None => vec![format!(
                            "§cNo {} found within 1536 blocks.",
                            biomes::display_name(target)
                        )],
                    },
                    None => vec![format!("§cUnknown biome: §f{query}§7 (see /help locate biome)")],
                }
            } else {
                vec!["§cUsage: /locate biome <name> or /locate structure".to_string()]
            };
            let players = state.players.lock().unwrap();
            if let Some(player) = players.get(&entity_id) {
                for line in &lines {
                    let packet = packets::build_chat(&format!("{{\"text\":\"{line}\"}}"));
                    let _ = player.sender.send(packet);
                }
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
