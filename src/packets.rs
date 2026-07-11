use crate::net::writing::{build_packet, write_string, write_varint};
use crate::player::Player;

pub use crate::net::writing::build_packet as build_packet_id;

pub fn build_player_list_item(username: &str, online: bool) -> Vec<u8> {
    let mut content = write_string(username);
    content.push(if online { 1 } else { 0 });
    content.extend((0i16).to_be_bytes());
    build_packet(0x38, &mut content)
}

pub fn build_chat(json: &str) -> Vec<u8> {
    let mut content = write_string(json);
    build_packet(0x02, &mut content)
}

pub fn build_disconnect(reason: &str) -> Vec<u8> {
    let mut content = write_string(reason);
    build_packet(0x40, &mut content)
}

pub fn build_game_mode_change(mode: u8) -> Vec<u8> {
    let mut content = Vec::new();
    content.push(3u8);
    content.extend((mode as f32).to_be_bytes());
    build_packet(0x2B, &mut content)
}

pub fn build_player_position_look(x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(x.to_be_bytes());
    content.extend((y + 1.63).to_be_bytes());
    content.extend(z.to_be_bytes());
    content.extend(yaw.to_be_bytes());
    content.extend(pitch.to_be_bytes());
    content.push(1u8);
    build_packet(0x08, &mut content)
}

pub fn build_spawn_player(j: &Player) -> Vec<u8> {
    let mut content = write_varint(j.entity_id);
    content.extend(write_string(&j.uuid));
    content.extend(write_string(&j.username));
    content.extend(((j.x * 32.0) as i32).to_be_bytes());
    content.extend(((j.y * 32.0) as i32).to_be_bytes());
    content.extend(((j.z * 32.0) as i32).to_be_bytes());
    content.push((j.yaw / 360.0 * 256.0) as i32 as u8);
    content.push((j.pitch / 360.0 * 256.0) as i32 as u8);

    let current_item = j.held_item();
    let current_item = if current_item < 0 { 0 } else { current_item };
    content.extend(current_item.to_be_bytes());

    content.extend(&[0x00, 0x00, 0x00, 0x7F]);
    build_packet(0x0C, &mut content)
}

pub fn equipment_snapshot(j: &Player) -> [(i16, i16); 5] {
    [
        (0, j.held_item()),
        (1, j.inventory[8]),
        (2, j.inventory[7]),
        (3, j.inventory[6]),
        (4, j.inventory[5]),
    ]
}

pub fn build_equipment_packets(entity_id: i32, j: &Player) -> Vec<Vec<u8>> {
    equipment_snapshot(j)
    .into_iter()
    .map(|(slot, item)| build_entity_equipment(entity_id, slot, item))
    .collect()
}

pub fn build_transaction_confirmation(window_id: u8, action_number: i16, accepted: bool) -> Vec<u8> {
    let mut content = Vec::new();
    content.push(window_id);
    content.extend(action_number.to_be_bytes());
    content.push(if accepted { 1 } else { 0 });
    build_packet(0x32, &mut content)
}

pub fn build_entity_equipment(entity_id: i32, slot: i16, item_id: i16) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.extend(slot.to_be_bytes());
    content.extend(item_id.to_be_bytes());
    if item_id >= 0 {
        content.push(1u8);
        content.extend((0i16).to_be_bytes());
        content.extend((-1i16).to_be_bytes());
    }
    build_packet(0x04, &mut content)
}

pub fn build_entity_head_look(j: &Player) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(j.entity_id.to_be_bytes());
    content.push((j.yaw / 360.0 * 256.0) as i32 as u8);
    build_packet(0x19, &mut content)
}

pub fn build_entity_teleport(j: &Player) -> Vec<u8> {
    build_entity_teleport_pos(j.entity_id, j.x, j.y, j.z, j.yaw, j.pitch)
}

pub fn build_entity_teleport_pos(entity_id: i32, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.extend(((x * 32.0) as i32).to_be_bytes());
    content.extend(((y * 32.0) as i32).to_be_bytes());
    content.extend(((z * 32.0) as i32).to_be_bytes());
    content.push((yaw / 360.0 * 256.0) as i32 as u8);
    content.push((pitch / 360.0 * 256.0) as i32 as u8);
    build_packet(0x18, &mut content)
}

pub fn build_destroy_entity(entity_id: i32) -> Vec<u8> {
    let mut content = vec![1u8];
    content.extend(entity_id.to_be_bytes());
    build_packet(0x13, &mut content)
}

pub fn build_block_change(x: i32, y: u8, z: i32, stored: u16) -> Vec<u8> {
    let block = stored & 0xFFF;
    let metadata = ((stored >> 12) & 0x0F) as u8;
    let mut content = Vec::new();
    content.extend(x.to_be_bytes());
    content.push(y);
    content.extend(z.to_be_bytes());
    content.extend(write_varint(block as i32));
    content.push(metadata);
    build_packet(0x23, &mut content)
}

pub fn block_to_item(stored: u16) -> i16 {
    let block_id = stored & 0xFFF;
    match block_id {
        1 => 4,
        2 => 3,
        3 => 3,
        7 => -1,
        12 => 12,
        13 => 13,
        16 => 263,
        55 => 331,  // redstone wire (block) -> item "Redstone" (331), pas d'item id 55
        56 => 264,
        59 => 296,  // ble en culture -> item ble (approximation, pas de gestion des graines)
        60 => 3,    // farmland casse -> dirt
        64 => 324,
        71 => 330,
        83 => 338,  // canne a sucre (block) -> item canne a sucre
        90 => -1,   // portail : jamais de drop
        92 => -1,   // cake (block) : pas de drop en vanilla
        _ => block_id as i16,
    }
}

pub fn build_spawn_item(entity_id: i32, _item_id: i16, x: f64, y: f64, z: f64, vel_x: i16, vel_y: i16, vel_z: i16) -> Vec<u8> {
    let mut content = write_varint(entity_id);
    content.push(2u8);
    content.extend(((x * 32.0) as i32).to_be_bytes());
    content.extend(((y * 32.0) as i32).to_be_bytes());
    content.extend(((z * 32.0) as i32).to_be_bytes());
    content.push(0u8);
    content.push(0u8);
    content.extend((1i32).to_be_bytes());
    content.extend(vel_x.to_be_bytes());
    content.extend(vel_y.to_be_bytes());
    content.extend(vel_z.to_be_bytes());
    build_packet(0x0E, &mut content)
}

pub fn build_item_metadata(entity_id: i32, item_id: i16, count: i8, damage: i16) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.push(0xAA);
    content.extend(item_id.to_be_bytes());
    content.push(count as u8);
    content.extend(damage.to_be_bytes());
    content.extend((-1i16).to_be_bytes());
    content.push(0x7F);
    build_packet(0x1C, &mut content)
}

pub fn build_collect_item(collected_id: i32, collector_id: i32) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(collected_id.to_be_bytes());
    content.extend(collector_id.to_be_bytes());
    build_packet(0x0D, &mut content)
}

pub fn build_entity_metadata_flags(entity_id: i32, sneaking: bool) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.push(0x00);
    content.push(if sneaking { 0x02 } else { 0x00 });
    content.push(0x7F);
    build_packet(0x1C, &mut content)
}

pub fn build_update_health(health: f32, food: i16, saturation: f32) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(health.to_be_bytes());
    content.extend(food.to_be_bytes());
    content.extend(saturation.to_be_bytes());
    build_packet(0x06, &mut content)
}

pub fn build_entity_status(entity_id: i32, status: u8) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.push(status);
    build_packet(0x1A, &mut content)
}

pub fn build_animation(entity_id: i32, animation: u8) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(entity_id.to_be_bytes());
    content.push(animation);
    build_packet(0x28, &mut content)
}

pub fn build_respawn(dimension: i32, difficulty: u8, gamemode: u8) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(dimension.to_be_bytes());
    content.push(difficulty);
    content.push(gamemode);
    content.extend(write_string("flat"));
    build_packet(0x07, &mut content)
}

pub fn build_set_slot(window_id: u8, slot: i16, item_id: i16, count: i8, damage: i16) -> Vec<u8> {
    let mut content = Vec::new();
    content.push(window_id);
    content.extend(slot.to_be_bytes());
    content.extend(item_id.to_be_bytes());
    if item_id >= 0 {
        content.push(count as u8);
        content.extend(damage.to_be_bytes());
        content.extend((-1i16).to_be_bytes());
    }
    build_packet(0x2F, &mut content)
}
