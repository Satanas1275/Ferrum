use rustyline::completion::Completer;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::Helper;

use std::time::Duration;

use crate::game::commands::teleport_entity;
use crate::packets;
use crate::save;
use crate::util::{get_process_ram_mb, parse_rel_coord};
use crate::world::SharedState;

struct ConsoleHelper;

impl Completer for ConsoleHelper {
    type Candidate = String;
    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        let cmds = ["help", "list", "say", "stop", "save-all", "gamemode", "tp", "teleport", "tps", "load"];
        let trimmed = line.trim();
        let completions: Vec<String> = cmds.iter()
            .filter(|c| c.starts_with(trimmed))
            .map(|c| c.to_string())
            .collect();
        Ok((0, completions))
    }
}

impl Highlighter for ConsoleHelper {}
impl Hinter for ConsoleHelper { type Hint = String; }
impl Validator for ConsoleHelper {}
impl Helper for ConsoleHelper {}

fn handle_console_command(parts: &[&str], state: &SharedState) {
    if parts.is_empty() { return; }
    let raw = parts[0].strip_prefix('/').unwrap_or(parts[0]);
    let command = raw;
    match command {
        "help" => {
            println!("Commands:");
            println!("  gamemode <mode> <player>    - Change game mode");
            println!("  tp <player> <x> <y> <z>     - Teleport");
            println!("  tp <player> <target>        - Teleport to player");
            println!("  list                        - List online players");
            println!("  say <message>               - Broadcast a message");
            println!("  save-all                    - Save world and players");
            println!("  stop                        - Shutdown server");
            println!("  tps                         - Show ticks per second");
            println!("  load                        - Show RAM and CPU usage");
        }
        "list" => {
            let players = state.players.lock().unwrap();
            let names: Vec<String> = players.values().map(|p| p.username.clone()).collect();
            println!("Online players ({}/{}): {}", names.len(), state.config.max_players, names.join(", "));
        }
        "say" => {
            if parts.len() > 1 {
                let msg = parts[1..].join(" ");
                let json = format!(r#"{{"text":"[Server] {msg}","color":"gold"}}"#);
                let packet = packets::build_chat(&json);
                let players = state.players.lock().unwrap();
                for (_, p) in players.iter() {
                    let _ = p.sender.send(packet.clone());
                }
                println!("[Server] {msg}");
            } else {
                println!("§cUsage: say <message>");
            }
        }
        "stop" => {
            println!("Saving world...");
            {
                let blocks = state.world.lock().unwrap().clone();
                match save::save_all_chunks(&blocks) {
                    Ok(_) => println!("World saved ({} blocks)", blocks.len()),
                    Err(e) => println!("Error saving world: {e}"),
                }
                let players = state.players.lock().unwrap();
                let mut saved = 0;
                for (_, player) in players.iter() {
                    if save::save_player(player).is_ok() {
                        saved += 1;
                    }
                }
                drop(players);
                if saved > 0 {
                    println!("Saved {saved} players");
                }
            }
            println!("Shutting down server...");
            let reason = r#"{"text":"Server shutting down","color":"red"}"#;
            let packet = packets::build_disconnect(reason);
            let players = state.players.lock().unwrap();
            for (_, p) in players.iter() {
                let _ = p.sender.send(packet.clone());
            }
            drop(players);
            std::thread::sleep(std::time::Duration::from_millis(200));
            std::process::exit(0);
        }
        "save-all" => {
            println!("Saving world...");
            let blocks = state.world.lock().unwrap().clone();
            match save::save_all_chunks(&blocks) {
                Ok(_) => println!("World saved ({} blocks)", blocks.len()),
                Err(e) => println!("Error saving world: {e}"),
            }
            let players = state.players.lock().unwrap();
            let mut saved = 0;
            for (_, player) in players.iter() {
                if save::save_player(player).is_ok() {
                    saved += 1;
                }
            }
            drop(players);
            println!("Saved {saved} players");
        }
        "gamemode" => {
            if parts.len() >= 2 {
                let mode = match parts[1] {
                    "0" | "survival" => 0u8,
                    "1" | "creative" => 1u8,
                    "2" | "adventure" => 2u8,
                    "3" | "spectator" => 3u8,
                    _ => 255u8,
                };
                if mode > 3 {
                    println!("§cInvalid mode. Use: 0=survival, 1=creative, 2=adventure, 3=spectator");
                    return;
                }
                let player_name = if parts.len() >= 3 { Some(parts[2]) } else { None };
                let mode_packet = packets::build_game_mode_change(mode);
                let mut players = state.players.lock().unwrap();
                let targets: Vec<i32> = if let Some(name) = player_name {
                    players.values().filter(|p| p.username == name).map(|p| p.entity_id).collect()
                } else {
                    println!("Usage: gamemode <mode> <player>");
                    return;
                };
                if targets.is_empty() {
                    println!("Player '{:?}' not found", player_name);
                    return;
                }
                for id in &targets {
                    if let Some(p) = players.get_mut(id) {
                        p.gamemode = mode;
                        let _ = p.sender.send(mode_packet.clone());
                    }
                }
                println!("Gamemode changed");
            } else {
                println!("Usage: gamemode <mode> <player>");
            }
        }
        "tp" | "teleport" => {
            if parts.len() == 3 {
                let target_name = parts[1];
                let dest_name = parts[2];
                let players_lock = state.players.lock().unwrap();
                let target_id = players_lock.values().find(|p| p.username == target_name).map(|p| p.entity_id);
                let dest_pos = players_lock.values().find(|p| p.username == dest_name).map(|p| (p.x, p.y, p.z, p.yaw, p.pitch));
                drop(players_lock);
                if let Some(id) = target_id {
                    if let Some((dx, dy, dz, dyaw, dpitch)) = dest_pos {
                        teleport_entity(id, dx, dy, dz, dyaw, dpitch, state);
                        println!("Teleported {target_name} to {dest_name}");
                    } else {
                        println!("Player '{dest_name}' not found");
                    }
                } else {
                    println!("Player '{target_name}' not found");
                }
            } else if parts.len() == 4 {
                let target_name = parts[1];
                let players_lock = state.players.lock().unwrap();
                let target = players_lock.values().find(|p| p.username == target_name).map(|p| (p.entity_id, p.x, p.y, p.z));
                drop(players_lock);
                if let Some((id, cx, cy, cz)) = target {
                    if let (Some(x), Some(y), Some(z)) = (
                        parse_rel_coord(parts[2], cx),
                        parse_rel_coord(parts[3], cy),
                        parse_rel_coord(parts[4], cz),
                    ) {
                        let tx = crate::util::center_coord(x);
                        let tz = crate::util::center_coord(z);
                        teleport_entity(id, tx, y, tz, 0.0, 0.0, state);
                        println!("Teleported {target_name} to ({tx:.1}, {y:.1}, {tz:.1})");
                    } else {
                        println!("Invalid coordinates");
                    }
                } else {
                    println!("Player '{target_name}' not found");
                }
            } else {
                println!("Usage: tp <player> <x> <y> <z> or tp <player> <target>");
            }
        }
        "tps" => {
            let tps = state.tps.lock().unwrap();
            let tps_5s = tps.tps(Duration::from_secs(5));
            let tps_30s = tps.tps(Duration::from_secs(30));
            let tps_5min = tps.tps(Duration::from_secs(300));
            let tps_15min = tps.tps(Duration::from_secs(900));
            drop(tps);
            println!("TPS (5s):   {tps_5s:.1}");
            println!("TPS (30s):  {tps_30s:.1}");
            println!("TPS (5m):   {tps_5min:.1}");
            println!("TPS (15m):  {tps_15min:.1}");
        }
        "load" => {
            let ram = get_process_ram_mb();
            let cpu = state.tps.lock().unwrap().cpu();
            println!("RAM: {ram:.1} MB");
            println!("CPU: {cpu:.1}%");
        }
        _ => {
            println!("Unknown command: {command}. Type help");
        }
    }
}

pub fn console_loop(state: SharedState) {
    let mut rl = match rustyline::Editor::<ConsoleHelper, rustyline::history::DefaultHistory>::new() {
        Ok(editor) => editor,
        Err(e) => { eprintln!("Console error: {e}"); return; }
    };
    rl.set_helper(Some(ConsoleHelper));
    let _ = rl.load_history("server_history.txt");
    loop {
        match rl.readline("> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }
                let _ = rl.add_history_entry(trimmed);
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                handle_console_command(&parts, &state);
            }
            Err(rustyline::error::ReadlineError::Interrupted)
            | Err(rustyline::error::ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                println!("Console error: {e}");
                break;
            }
        }
    }
    let _ = rl.save_history("server_history.txt");
}
