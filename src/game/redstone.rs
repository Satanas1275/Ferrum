use std::collections::{HashMap, VecDeque};
use std::sync::atomic::Ordering;

use crate::packets;
use crate::world::{get_block, SharedState};

fn block_id(stored: u16) -> u16 { stored & 0xFFF }
fn meta(stored: u16) -> u8 { ((stored >> 12) & 0x0F) as u8 }
fn encode(block: u16, m: u8) -> u16 { block | ((m as u16) << 12) }

fn is_opaque(block: u16) -> bool {
    !matches!(block,
              0 | 6 | 8 | 9 | 10 | 11 | 26 | 27 | 28 | 30 | 31 | 32 |
              37 | 38 | 39 | 40 | 50 | 51 | 52 | 53 | 55 | 59 | 63 | 64 |
              65 | 66 | 68 | 69 | 70 | 71 | 72 | 75 | 76 | 77 | 78 | 83 |
              90 | 92 | 93 | 94 | 96 | 104 | 105 | 106 | 107 | 108 | 109 |
              111 | 114 | 115 | 119 | 122 | 127 | 128 | 131 | 132 | 134 |
              135 | 136 | 141 | 142 | 143 | 147 | 148 | 149 | 150 | 151 |
              156 | 157 | 163 | 164 | 167 | 171 | 175 | 176 | 177 |
              180 | 183 | 184 | 185 | 186 | 203
    )
}

fn get_block_power(stored: u16) -> u8 {
    let block = block_id(stored);
    let m = meta(stored);
    match block {
        55 => m,
        75 => 15,
        76 => 0,
        93 | 94 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        149 | 150 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        69 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        77 | 143 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        70 | 72 | 147 | 148 => m,
        152 => 15,
        157 | 28 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        _ => 0,
    }
}

fn is_power_source(stored: u16) -> bool {
    let block = block_id(stored);
    matches!(block,
             69 | 70 | 72 | 75 | 77 | 143 | 147 | 148 | 152
    ) || (matches!(block, 93 | 94 | 149 | 150 | 157 | 28) && (meta(stored) & 0x08) != 0)
}

fn facing(rot: u8) -> (i32, i32) {
    match rot & 0x03 {
        0 => (0, -1),
        1 => (1, 0),
        2 => (0, 1),
        3 => (-1, 0),
        _ => (0, 0),
    }
}

fn dust_power_input(world: &HashMap<(i32, i32, i32), u16>, x: i32, y: i32, z: i32) -> u8 {
    let mut power: u8 = 0;

    for (dx, dz) in &[(1,0), (-1,0), (0,1), (0,-1)] {
        let n = get_block(world, x + dx, y, z + dz);
        let nb = block_id(n);
        if nb == 55 {
            // fil-à-fil : on perd 1 point de force par bloc traversé
            power = power.max(meta(n).saturating_sub(1));
        } else if get_block_power(n) > 0 {
            // contact direct avec une vraie source (levier, bouton, plaque...) : pleine force, pas de décrément
            power = power.max(get_block_power(n));
        } else if nb == 75 {
            power = power.max(15);
        } else if nb == 93 || nb == 94 {
            let f = facing(meta(n));
            if f == (-*dx, -*dz) && (meta(n) & 0x08) != 0 {
                power = power.max(15);
            }
        } else if nb == 149 || nb == 150 {
            let f = facing(meta(n));
            if f == (-*dx, -*dz) && (meta(n) & 0x08) != 0 {
                power = power.max(15);
            }
        } else if is_opaque(nb) {
            // dust on top of opaque neighbor → this dust gets power from it (un hop en plus, donc -1)
            let above = get_block(world, x + dx, y + 1, z + dz);
            if block_id(above) == 55 {
                power = power.max(meta(above).saturating_sub(1));
            }
            // redstone torch attached to the SIDE of opaque neighbor
            // we check all 4 wall positions around the opaque block
            for (tdx, tdz) in &[(1,0), (-1,0), (0,1), (0,-1)] {
                let torch_at = get_block(world, x + dx + tdx, y, z + dz + tdz);
                if block_id(torch_at) == 75 {
                    // torch direction: support points away from torch toward opaque block
                    let tm = meta(torch_at);
                    let (sx, sz) = match tm & 0x07 {
                        1 => (-1, 0), 2 => (1, 0), 3 => (0, -1), 4 => (0, 1),
                        _ => (0, 0),
                    };
                    // does the torch's support point toward the opaque block?
                    if (x + dx + tdx + sx, z + dz + tdz + sz) == (x + dx, z + dz) {
                        power = power.max(15);
                    }
                } else if matches!(block_id(torch_at), 93 | 94) && (meta(torch_at) & 0x08) != 0 {
                    let tf = facing(meta(torch_at));
                    // does the repeater point toward the opaque block?
                    if (tdx + tf.0, tdz + tf.1) == (0, 0) {
                        power = power.max(15);
                    }
                }
            }
        }
    }

    let above = get_block(world, x, y + 1, z);
    if block_id(above) == 55 {
        power = power.max(meta(above).saturating_sub(1));
    } else if get_block_power(above) > 0 {
        power = power.max(get_block_power(above));
    }

    let below = get_block(world, x, y - 1, z);
    if block_id(below) == 55 {
        power = power.max(meta(below).saturating_sub(1));
    } else if is_power_source(below) {
        // le dust repose directement sur le bloc source (ex: redstone block) : pleine force, pas de décrément
        power = power.max(get_block_power(below));
    }

    if is_opaque(block_id(below)) {
        for (dx, dz) in &[(1,0), (-1,0), (0,1), (0,-1)] {
            let ground = get_block(world, x + dx, y - 1, z + dz);
            if is_strongly_powered(world, x + dx, y - 1, z + dz, ground) {
                power = power.max(15);
            }
        }
    }

    power
}

fn is_strongly_powered(world: &HashMap<(i32, i32, i32), u16>, x: i32, y: i32, z: i32, stored: u16) -> bool {
    let block = block_id(stored);
    if block == 75 { return true; }
    if block == 152 { return true; }
    if matches!(block, 93 | 94 | 149 | 150) { return (meta(stored) & 0x08) != 0; }
    if is_opaque(block) {
        for (dx, dz) in &[(1,0), (-1,0), (0,1), (0,-1)] {
            let n = get_block(world, x + dx, y, z + dz);
            match block_id(n) {
                55 => { /* le fil alimente ce bloc faiblement seulement — ça ne compte pas comme source forte */ },
                75 => { return true; },
                _ => { if get_block_power(n) > 0 { return true; } },
            }
        }
        return false;
    }
    false
}

fn is_block_powered(world: &HashMap<(i32, i32, i32), u16>, x: i32, y: i32, z: i32) -> bool {
    // check all 6 neighbors for direct power sources or strong power
    for &(nx, ny, nz) in &[(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
        let n = get_block(world, nx, ny, nz);
        let nb = block_id(n);
        if nb == 55 && meta(n) > 0 { return true; }
        if nb == 75 { return true; }
        if get_block_power(n) > 0 { return true; }
        if is_strongly_powered(world, nx, ny, nz, n) { return true; }
        // also check diagonal ground neighbors for strong power (torch on wall of adjacent block)
        if nb == 0 || is_opaque(nb) {
            for (dx, dz) in &[(1,0), (-1,0), (0,1), (0,-1)] {
                let diagonal = get_block(world, nx + dx, ny, nz + dz);
                if is_strongly_powered(world, nx + dx, ny, nz + dz, diagonal) {
                    return true;
                }
            }
        }
    }
    false
}

fn is_redstone_relevant(block: u16) -> bool {
    matches!(block,
             55 | 75 | 76 | 93 | 94 | 149 | 150 |
             69 | 70 | 72 | 77 | 143 | 147 | 148 | 152 |
             27 | 28 | 66 | 157
    )
}

async fn send_block_update(state: &SharedState, x: i32, y: i32, z: i32, stored: u16) {
    let pkt = packets::build_block_change(x, y as u8, z, stored);
    let players = state.players.lock().await;
    for (_, p) in players.iter() {
        let _ = p.sender.send(pkt.clone());
    }
}

pub async fn schedule_update(state: &SharedState, x: i32, y: i32, z: i32) {
    let mut queue = state.redstone_queue.lock().await;
    if !queue.contains(&(x, y, z)) {
        queue.push_back((x, y, z));
    }
}

/// Check if a block is powered by any adjacent redstone component
async fn is_block_powered_by_neighbor(state: &SharedState, x: i32, y: i32, z: i32) -> bool {
    let world = state.world.lock().await;
    let result = is_block_powered(&world, x, y, z);
    drop(world);
    result
}

pub async fn tick(state: &SharedState) {
    let tick = state.tick_counter.fetch_add(1, Ordering::SeqCst);

    // process delayed events (repeaters)
    {
        let mut delayed = state.redstone_delayed.lock().await;
        while let Some(front) = delayed.front() {
            if front.0 <= tick {
                let (_, x, y, z, stored) = delayed.pop_front().unwrap();
                let pkt = packets::build_block_change(x, y as u8, z, stored);
                {
                    let mut world = state.world.lock().await;
                    world.insert((x, y, z), stored);
                }
                let players = state.players.lock().await;
                for (_, p) in players.iter() {
                    let _ = p.sender.send(pkt.clone());
                }
                drop(players);
                crate::game::connection::notify_neighbors(state, x, y, z).await;
                let mut queue = state.redstone_queue.lock().await;
                for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
                    if !queue.contains(&(nx, ny, nz)) {
                        queue.push_back((nx, ny, nz));
                    }
                }
            } else {
                break;
            }
        }
    }

    const MAX_ITER: usize = 50_000;
    let mut queue: VecDeque<(i32, i32, i32)>;

    {
        let mut shared = state.redstone_queue.lock().await;
        queue = shared.drain(..).collect();
    }

    let mut visited: Vec<(i32, i32, i32)> = Vec::new();
    let mut iterations = 0;
    let mut pending_actuations: Vec<(i32, i32, i32, u16)> = Vec::new();

    while let Some((x, y, z)) = queue.pop_front() {
        if iterations >= MAX_ITER { break; }
        iterations += 1;

        if visited.contains(&(x, y, z)) { continue; }
        visited.push((x, y, z));

        let world = state.world.lock().await;
        let stored = get_block(&world, x, y, z);
        if stored == 0 { drop(world); continue; }
        let block = block_id(stored);
        let m = meta(stored);

        if !is_redstone_relevant(block) { drop(world); continue; }

        let mut schedule_delayed: Option<(u64, u16)> = None;
        let new_stored: Option<u16>;

        match block {
            55 => {
                let new_power = dust_power_input(&world, x, y, z);
                new_stored = if new_power != m { Some(encode(block, new_power)) } else { None };
            },
            75 | 76 => {
                let (support_x, support_y, support_z) = if m == 5 {
                    (x, y - 1, z)
                } else {
                    let (dx, dz) = match m & 0x07 {
                        1 => (-1, 0), 2 => (1, 0), 3 => (0, -1), 4 => (0, 1),
                        _ => (0, 0),
                    };
                    (x + dx, y, z + dz)
                };
                let support_air = block_id(get_block(&world, support_x, support_y, support_z)) == 0;
                let base_powered = if support_air {
                    true
                } else {
                    is_block_powered(&world, support_x, support_y, support_z)
                };
                let should_be_active = !base_powered && is_opaque(block_id(get_block(&world, support_x, support_y, support_z)));
                let is_active = block == 75;
                new_stored = if should_be_active != is_active {
                    Some(encode(if should_be_active { 75 } else { 76 }, m))
                } else { None };
            },
            93 | 94 => {
                let (dx, dz) = facing(m);
                let input = get_block(&world, x + dx, y, z + dz);
                let input_power = get_block_power(input);
                let output_on = (m & 0x08) != 0;
                let delay_ticks = ((m >> 2) & 0x03) as u64 + 1;
                let should_be_on = input_power > 0;
                if should_be_on != output_on {
                    let new_m = (m & 0x07) | if should_be_on { 0x08 } else { 0 };
                    let new_st = encode(block, new_m);
                    schedule_delayed = Some((tick + delay_ticks, new_st));
                    new_stored = None;
                } else {
                    new_stored = None;
                }
            },
            149 | 150 => {
                let (dx, dz) = facing(m);
                let input = get_block(&world, x + dx, y, z + dz);
                let input_power = get_block_power(input);
                let mode_subtract = (m & 0x04) != 0;
                let output_on = (m & 0x08) != 0;
                let _rear_power = get_block_power(get_block(&world, x - dx, y, z - dz));
                let side_a = get_block_power(get_block(&world, x - dz, y, z + dx));
                let side_b = get_block_power(get_block(&world, x + dz, y, z - dx));
                let side_power = side_a.max(side_b);
                let result = if mode_subtract {
                    input_power.saturating_sub(side_power)
                } else {
                    if side_power > input_power { 0 } else { input_power }
                };
                let should_be_on = result > 0;
                new_stored = if should_be_on != output_on {
                    Some(encode(block, (m & 0x07) | if should_be_on { 0x08 } else { 0 }))
                } else { None };
            },
            _ => { new_stored = None; },
        }
        drop(world);

        if let Some(ns) = new_stored {
            {
                let mut world = state.world.lock().await;
                world.insert((x, y, z), ns);
            }
            send_block_update(state, x, y, z, ns).await;
            crate::game::connection::notify_neighbors(state, x, y, z).await;

            // collect actuations (batch write to avoid redundant work)
            for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
                let world = state.world.lock().await;
                let neighbor = get_block(&world, nx, ny, nz);
                let nb = block_id(neighbor);
                let nm = meta(neighbor);
                drop(world);
                if matches!(nb, 64 | 71 | 96 | 107 | 167 | 183 | 184 | 185 | 186 | 123 | 124 | 29 | 33 | 27 | 28 | 157 | 25) {
                    let powered = is_block_powered_by_neighbor(state, nx, ny, nz).await;
                    match nb {
                        64 | 71 | 96 | 107 | 167 | 183 | 184 | 185 | 186 => {
                            let open = (nm & 0x04) != 0;
                            if powered != open {
                                let new_nm = if powered { nm | 0x04 } else { nm & !0x04 };
                                pending_actuations.push((nx, ny, nz, encode(nb, new_nm)));
                                if (nb == 64 || nb == 71) && (nm & 0x08) == 0 && ny < 255 {
                                    let world = state.world.lock().await;
                                    let top = get_block(&world, nx, ny + 1, nz);
                                    drop(world);
                                    if block_id(top) == nb {
                                        let top_open = if powered { (meta(top) & 0x07) | 0x04 } else { meta(top) & 0x07 };
                                        pending_actuations.push((nx, ny + 1, nz, encode(nb, top_open)));
                                    }
                                }
                            }
                        },
                        123 => { if powered { pending_actuations.push((nx, ny, nz, encode(124, 0))); } },
                        124 => { if !powered { pending_actuations.push((nx, ny, nz, encode(123, 0))); } },
                        29 | 33 => {
                            let extended = (nm & 0x08) != 0;
                            if powered != extended {
                                pending_actuations.push((nx, ny, nz, encode(nb, if powered { nm | 0x08 } else { nm & !0x08 })));
                            }
                        },
                        27 | 28 | 157 => {
                            let active = (nm & 0x08) != 0;
                            if powered != active {
                                pending_actuations.push((nx, ny, nz, encode(nb, if powered { nm | 0x08 } else { nm & !0x08 })));
                            }
                        },
                        25 => { if powered { /* note block sound */ } },
                        _ => {},
                    }
                }
            }
        }

        if let Some((target_tick, ns)) = schedule_delayed {
            let mut delayed = state.redstone_delayed.lock().await;
            delayed.push_back((target_tick, x, y, z, ns));
        }

        // propagate to direct neighbors
        for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
            if visited.contains(&(nx, ny, nz)) || queue.contains(&(nx, ny, nz)) { continue; }
            let world = state.world.lock().await;
            let nb = block_id(get_block(&world, nx, ny, nz));
            drop(world);
            if is_redstone_relevant(nb) {
                queue.push_back((nx, ny, nz));
            }
        }

        // propagate through opaque blocks: a redstone component on the far side
        // of an adjacent opaque block (e.g. torch on top of stone, lever on wall)
        // needs to be notified of power changes in the opaque block
        for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
            if visited.contains(&(nx, ny, nz)) || queue.contains(&(nx, ny, nz)) { continue; }
            let world = state.world.lock().await;
            let nb = block_id(get_block(&world, nx, ny, nz));
            drop(world);
            if !is_opaque(nb) { continue; }
            // check all 6 sides of the opaque block for attached redstone
            for (tx, ty, tz) in [(nx-1,ny,nz),(nx+1,ny,nz),(nx,ny-1,nz),(nx,ny+1,nz),(nx,ny,nz-1),(nx,ny,nz+1)] {
                if tx == x && ty == y && tz == z { continue; }
                if visited.contains(&(tx, ty, tz)) || queue.contains(&(tx, ty, tz)) { continue; }
                let world = state.world.lock().await;
                let tb = block_id(get_block(&world, tx, ty, tz));
                drop(world);
                if is_redstone_relevant(tb) {
                    queue.push_back((tx, ty, tz));
                }
            }
        }
    }

    // write actuation changes
    {
        let mut world = state.world.lock().await;
        for &(x, y, z, ns) in &pending_actuations {
            world.insert((x, y, z), ns);
        }
    }
    for &(x, y, z, ns) in &pending_actuations {
        send_block_update(state, x, y, z, ns).await;
        crate::game::connection::notify_neighbors(state, x, y, z).await;
        let mut queue = state.redstone_queue.lock().await;
        for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
            if !queue.contains(&(nx, ny, nz)) {
                queue.push_back((nx, ny, nz));
            }
        }
    }
}
