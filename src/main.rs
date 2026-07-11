mod config;
mod console;
mod game;
mod items;
mod net;
mod packets;
mod player;
mod util;
mod world;

use std::sync::atomic::{AtomicI32, AtomicU64};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;

use crate::config::ServerConfig;
use crate::game::connection::handle_client;
use crate::world::{SharedState, State, TpsTracker};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> std::io::Result<()> {
    let config = ServerConfig::load();

    let port = config.port;
    let state: SharedState = Arc::new(State {
        players: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        world: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        items: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        next_id: AtomicI32::new(1),
        tps: tokio::sync::Mutex::new(TpsTracker::new()),
        redstone_queue: tokio::sync::Mutex::new(std::collections::VecDeque::new()),
        redstone_delayed: tokio::sync::Mutex::new(std::collections::VecDeque::new()),
        tick_counter: AtomicU64::new(0),
        config,
    });

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    println!("Server listening on {}", addr);
    println!("Type help for server commands");

    {
        let state = state.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || console::console_loop(state, handle));
    }

    {
        let state = state.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            loop {
                let start = std::time::Instant::now();

                handle.block_on(async {
                    state.tps.lock().await.tick();

                    let to_remove = {
                        let mut items = state.items.lock().await;
                        let mut removed = Vec::new();
                        for (&eid, item) in items.iter_mut() {
                            item.age = item.age.saturating_add(1);
                            if item.age >= 6000 {
                                removed.push(eid);
                            }
                        }
                        for eid in &removed {
                            items.remove(eid);
                        }
                        removed
                    };

                    if !to_remove.is_empty() {
                        let destroys: Vec<Vec<u8>> = to_remove.iter().map(|eid| packets::build_destroy_entity(*eid)).collect();
                        let players = state.players.lock().await;
                        for (_, p) in players.iter() {
                            for pkt in &destroys {
                                let _ = p.sender.send(pkt.clone());
                            }
                        }
                    }

                    crate::game::redstone::tick(&state).await;
                });

                let elapsed = start.elapsed();
                if let Some(sleep) = Duration::from_millis(50).checked_sub(elapsed) {
                    std::thread::sleep(sleep);
                }
            }
        });
    }

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("New connection: {addr}");
        let state = state.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, state).await {
                println!("Error with {addr}: {e}");
            }
        });
    }
}
