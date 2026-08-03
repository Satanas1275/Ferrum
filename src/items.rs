use crate::packets;
use crate::world::SharedState;

pub const PICKUP_DELAY_TICKS: u16 = 40;

#[derive(Clone)]
pub struct ItemEntity {
    pub entity_id: i32,
    pub item_id: i16,
    pub count: i8,
    pub damage: i16,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub age: u16,
}

pub fn spawn_item_entity(
    state: &SharedState,
    item_id: i16,
    count: i8,
    damage: i16,
    x: f64,
    y: f64,
    z: f64,
    vel_x: i16,
    vel_y: i16,
    vel_z: i16,
) -> i32 {
    let entity_id = state.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let item = ItemEntity {
        entity_id,
        item_id,
        count,
        damage,
        x,
        y,
        z,
        age: 0,
    };

    state.items.lock().unwrap().insert(entity_id, item);

    let spawn_packet = packets::build_spawn_item(entity_id, item_id, x, y, z, vel_x, vel_y, vel_z);
    let metadata_packet = packets::build_item_metadata(entity_id, item_id, count, damage);

    let players = state.players.lock().unwrap();
    for (_, player) in players.iter() {
        let _ = player.sender.send(spawn_packet.clone());
        let _ = player.sender.send(metadata_packet.clone());
    }
    entity_id
}
