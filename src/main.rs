mod config;
mod console;
mod game;
mod items;
mod net;
mod packets;
mod player;
mod util;
mod world;

use std::sync::atomic::AtomicI32;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;

use crate::config::ServerConfig;
use crate::game::connection::handle_client;
use crate::world::{SharedState, State};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = ServerConfig::load();

    let port = config.port;
    let state: SharedState = Arc::new(State {
        players: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        world: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        items: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        next_id: AtomicI32::new(1),
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
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.tick().await;
            loop {
                interval.tick().await;
                let to_remove = {
                    let mut items = state.items.lock().await;
                    let mut removed = Vec::new();
                    for (&eid, item) in items.iter_mut() {
                        item.age = item.age.saturating_add(20);
                        if item.age >= 6000 {
                            removed.push(eid);
                        }
                    }
                    for eid in &removed {
                        items.remove(eid);
                    }
                    removed
                };
                for eid in &to_remove {
                    let destroy = packets::build_destroy_entity(*eid);
                    let players = state.players.lock().await;
                    for (_, p) in players.iter() {
                        let _ = p.sender.send(destroy.clone());
                    }
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
