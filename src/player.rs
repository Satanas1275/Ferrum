use std::collections::HashSet;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct Player {
    pub entity_id: i32,
    pub uuid: String,
    pub username: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub gamemode: u8,
    pub inventory: [i16; 45],
    pub counts: [u8; 45],
    pub selected_slot: usize,
    pub cursor_item: i16,
    pub cursor_count: u8,
    pub health: f32,
    pub highest_y: f64,
    pub sneaking: bool,
    pub sender: mpsc::UnboundedSender<Vec<u8>>,
    pub loaded_chunks: HashSet<(i32, i32)>,
    pub last_bcast_x: i32,
    pub last_bcast_y: i32,
    pub last_bcast_z: i32,
    pub last_bcast_yaw: u8,
    pub last_bcast_pitch: u8,
    pub last_chunk: (i32, i32),
    pub view_distance: i32,
}

impl Player {
    pub fn held_item(&self) -> i16 {
        self.inventory[36 + self.selected_slot]
    }
}
