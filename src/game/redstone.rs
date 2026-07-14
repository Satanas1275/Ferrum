use std::collections::{HashMap, HashSet, VecDeque};
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
        93 => 0,
        94 => 15,
        69 => if (m & 0x08) != 0 { 15 } else { 0 },
        149 | 150 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        77 | 143 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        70 | 72 | 147 | 148 => if m > 0 { 15 } else { 0 },
        152 => 15,
        157 | 28 => {
            if (m & 0x08) != 0 { 15 } else { 0 }
        },
        _ => 0,
    }
}

fn is_power_source(stored: u16) -> bool {
    let block = block_id(stored);
    let m = meta(stored);
    match block {
        75 | 94 | 152 => true,
        69 | 77 | 143 => (m & 0x08) != 0,
        70 | 72 | 147 | 148 => m > 0,
        93 => false,
        149 | 150 | 157 | 28 => (m & 0x08) != 0,
        _ => false,
    }
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
        } else if !matches!(nb, 94 | 149 | 150) && get_block_power(n) > 0 {
            // contact direct avec une vraie source (levier, bouton, plaque...) : pleine force, pas de décrément
            // (repeater/comparator sont EXCLUS ici exprès : ce sont des composants
            // directionnels qui n'alimentent que par leur sortie, pas leurs 4 côtés —
            // ils sont gérés par leurs branches dédiées ci-dessous qui vérifient `facing`)
            power = power.max(get_block_power(n));
        } else if nb == 75 {
            power = power.max(15);
        } else if nb == 94 {
            let f = facing(meta(n));
            if f == (-*dx, -*dz) {
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
                } else if block_id(torch_at) == 94 {
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
    } else if !matches!(block_id(above), 93 | 94 | 149 | 150) && get_block_power(above) > 0 {
        power = power.max(get_block_power(above));
    }

    let below = get_block(world, x, y - 1, z);
    if block_id(below) == 55 {
        power = power.max(meta(below).saturating_sub(1));
    } else if !matches!(block_id(below), 93 | 94 | 149 | 150) && is_power_source(below) {
        // le dust repose directement sur le bloc source (ex: redstone block) : pleine force, pas de décrément
        power = power.max(get_block_power(below));
    }

    if is_opaque(block_id(below)) {
        for (dx, dz) in &[(1,0), (-1,0), (0,1), (0,-1)] {
            // Wire can climb one solid block: from a wire at ground level to
            // wire on top of the adjacent block.  The reverse (descending)
            // case was already covered above, but this upward input was not.
            let lower_wire = get_block(world, x + dx, y - 1, z + dz);
            if block_id(lower_wire) == 55 {
                power = power.max(meta(lower_wire).saturating_sub(1));
            }
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
    if block == 94 { return true; }
    if block == 152 { return true; }
    if matches!(block, 149 | 150) { return (meta(stored) & 0x08) != 0; }
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
        if nb == 94 {
            let f = facing(meta(n));
            if f == (x - nx, z - nz) { return true; }
        } else if (nb == 149 || nb == 150) && (meta(n) & 0x08) != 0 {
            let f = facing(meta(n));
            if f == (x - nx, z - nz) { return true; }
        } else if get_block_power(n) > 0 {
            return true;
        }
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

/// Avant la propagation "croissante" classique, on remet à zéro tout fil de
/// redstone connecté qui pourrait avoir perdu sa source de puissance (ex: un
/// levier qu'on vient d'éteindre).
///
/// Pourquoi c'est nécessaire : le BFS principal ci-dessous ne fait qu'UN SEUL
/// passage par tick, et pour chaque bloc il prend le max entre sa valeur
/// actuelle et celle de ses voisins. C'est correct quand la puissance
/// AUGMENTE (peu importe l'ordre de visite, le max fini toujours par être
/// bon), mais c'est faux quand elle DIMINUE : un fil peut être recalculé
/// AVANT que son voisin (qui dépendait de lui) ait été mis à jour, lire
/// l'ancienne valeur (périmée) de ce voisin, se fixer dessus, et comme
/// chaque bloc n'est visité qu'une fois par tick (`visited`), il ne sera
/// jamais recorrigé -> le fil reste "allumé" pour toujours après extinction
/// de la source. C'est exactement le symptôme "j'éteins le levier et la
/// redstone reste allumée".
///
/// Le fix classique (deux passes) : on repart des points modifiés, on
/// remet à 0 tout le réseau de fils déjà allumé qui leur est connecté, puis
/// on laisse le BFS normal re-propager la vraie puissance depuis les
/// sources encore actives. Comme on ne touche qu'aux fils déjà allumés
/// (meta > 0), cette passe ne fait rien du tout quand on allume un circuit
/// (aucun flicker dans ce cas).
async fn reset_dust_network(state: &SharedState, seeds: &[(i32, i32, i32)]) -> Vec<(i32, i32, i32)> {
    let mut to_visit: VecDeque<(i32, i32, i32)> = VecDeque::new();
    let mut seen: Vec<(i32, i32, i32)> = Vec::new();
    let mut reset_list: Vec<(i32, i32, i32)> = Vec::new();

    for &(x, y, z) in seeds {
        let stored = {
            let world = state.world.lock().await;
            get_block(&world, x, y, z)
        };
        let block = block_id(stored);
        let m = meta(stored);
        // Un fil à 0 est typiquement un fil qui vient juste d'être posé.
        // Il doit seulement calculer sa puissance depuis le réseau voisin ;
        // il ne signale pas une extinction et ne doit donc pas remettre ce
        // réseau à zéro avant ce calcul.
        if !can_reset_dust_from(block, m) {
            continue;
        }
        if block == 55 && m == 0 {
            continue;
        }
        // Un repeater/comparator est un composant DIRECTIONNEL : il ne peut
        // affecter que ce qu'il y a devant lui (sa sortie). Ce qu'il y a
        // derrière (son entrée) est un fil qu'il se contente de LIRE, sans
        // jamais l'influencer. Si on le traitait comme une source
        // omnidirectionnelle ici, chaque fois que le repeater changerait
        // d'état, on remettrait à zéro (inutilement, avec un flicker visible)
        // le fil qui l'alimente — exactement le bug rapporté.
        let mut candidates: Vec<(i32, i32, i32)> = if matches!(block, 93 | 94 | 149 | 150) {
            let (dx, dz) = facing(m);
            vec![(x + dx, y, z + dz)]
        } else {
            vec![(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)]
        };
        // When the changed position is an empty block (a wire was broken),
        // a descending wire may only touch it diagonally.  Include both
        // slope directions so that the reset reaches that branch as well.
        if !matches!(block, 93 | 94 | 149 | 150) {
            for (dx, dz) in [(1,0), (-1,0), (0,1), (0,-1)] {
                candidates.push((x + dx, y - 1, z + dz));
                candidates.push((x + dx, y + 1, z + dz));
            }
        }
        for n in candidates {
            to_visit.push_back(n);
        }
    }

    while let Some((x, y, z)) = to_visit.pop_front() {
        if seen.contains(&(x, y, z)) { continue; }
        seen.push((x, y, z));

        let stored = {
            let world = state.world.lock().await;
            get_block(&world, x, y, z)
        };
        if block_id(stored) != 55 || meta(stored) == 0 { continue; }

        {
            let mut world = state.world.lock().await;
            world.insert((x, y, z), encode(55, 0));
        }
        reset_list.push((x, y, z));

        for n in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
            if !seen.contains(&n) {
                to_visit.push_back(n);
            }
        }
        // Dust on a block top is diagonally connected to the dust at the
        // foot of that block.  The reset must traverse those slopes too, or
        // the upper wire keeps its old power after the source is turned off.
        for (dx, dz) in [(1,0), (-1,0), (0,1), (0,-1)] {
            for dy in [-1, 1] {
                let n = (x + dx, y + dy, z + dz);
                if !seen.contains(&n) {
                    to_visit.push_back(n);
                }
            }
        }
    }

    reset_list
}

pub async fn tick(state: &SharedState) {
    let tick = state.tick_counter.fetch_add(1, Ordering::SeqCst);

    // Pressure plates are sources while a player's feet occupy their block.
    // Store the normal client metadata (0/1), while exposing full redstone
    // power through `get_block_power` above.
    let occupied: HashSet<(i32, i32, i32)> = {
        let players = state.players.lock().await;
        players.values().map(|p| {
            (p.x.floor() as i32, (p.y - 1.0).floor() as i32, p.z.floor() as i32)
        }).collect()
    };
    let plate_changes: Vec<(i32, i32, i32, u16)> = {
        let mut world = state.world.lock().await;
        let positions: Vec<(i32, i32, i32)> = world.iter()
            .filter_map(|(&(x, y, z), &stored)| matches!(block_id(stored), 70 | 72 | 147 | 148).then_some((x, y, z)))
            .collect();
        let mut changes = Vec::new();
        for (x, y, z) in positions {
            let stored = get_block(&world, x, y, z);
            let powered = occupied.contains(&(x, y, z));
            let new_stored = encode(block_id(stored), if powered { 1 } else { 0 });
            if new_stored != stored {
                world.insert((x, y, z), new_stored);
                changes.push((x, y, z, new_stored));
            }
        }
        changes
    };
    for &(x, y, z, stored) in &plate_changes {
        send_block_update(state, x, y, z, stored).await;
        schedule_update(state, x, y, z).await;
    }

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
                // Repartir du repeater lui-même : `reset_dust_network` sait qu'un
                // composant directionnel ne peut remettre à zéro que sa sortie.
                // En ajoutant directement ses six voisins ici, le fil à l'entrée
                // devenait une graine du reset et s'éteignait à chaque bascule.
                let mut queue = state.redstone_queue.lock().await;
                if !queue.contains(&(x, y, z)) {
                    queue.push_back((x, y, z));
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

    // Phase de reset : neutralise les valeurs périmées avant de re-propager
    // (voir doc de reset_dust_network ci-dessus).
    let seeds: Vec<(i32, i32, i32)> = queue.iter().cloned().collect();
    let reset_positions = reset_dust_network(state, &seeds).await;
    let reset_dust: HashSet<(i32, i32, i32)> = reset_positions.iter().copied().collect();
    for &(x, y, z) in &reset_positions {
        send_block_update(state, x, y, z, encode(55, 0)).await;
        if !queue.contains(&(x, y, z)) {
            queue.push_back((x, y, z));
        }
    }

    let mut visited: Vec<(i32, i32, i32)> = Vec::new();
    let mut iterations = 0;
    let mut pending_actuations: Vec<(i32, i32, i32, u16)> = Vec::new();

    while let Some((x, y, z)) = queue.pop_front() {
        if iterations >= MAX_ITER { break; }
        iterations += 1;

        if visited.contains(&(x, y, z)) {
            // A dust node may need another pass when a neighbour was
            // recomputed later in the same tick.  Other components are still
            // handled once to avoid feedback loops.
            let world = state.world.lock().await;
            let is_dust = block_id(get_block(&world, x, y, z)) == 55;
            drop(world);
            if !is_dust { continue; }
        } else {
            visited.push((x, y, z));
        }

        let world = state.world.lock().await;
        let stored = get_block(&world, x, y, z);
        if stored == 0 {
            // La suppression d'un fil est elle aussi un changement de signal :
            // la case vide n'est pas elle-même un composant à recalculer, mais
            // les composants adjacents (notamment un repeater dont c'était
            // l'entrée) doivent relire leur alimentation.
            drop(world);
            for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
                if visited.contains(&(nx, ny, nz)) || queue.contains(&(nx, ny, nz)) {
                    continue;
                }
                let world = state.world.lock().await;
                let neighbor = block_id(get_block(&world, nx, ny, nz));
                drop(world);
                if is_redstone_relevant(neighbor) {
                    queue.push_back((nx, ny, nz));
                }
            }
            continue;
        }
        let block = block_id(stored);
        let m = meta(stored);

        if !is_redstone_relevant(block) {
            // A lamp is a passive six-sided consumer.  Evaluating it when it
            // is placed lets it light immediately from an already-powered
            // adjacent wire, without scheduling/resetting that wire.
            if matches!(block, 123 | 124) {
                let powered = is_block_powered(&world, x, y, z);
                let new_block = if powered { 124 } else { 123 };
                drop(world);
                if new_block != block {
                    let ns = encode(new_block, m);
                    let mut world = state.world.lock().await;
                    world.insert((x, y, z), ns);
                    drop(world);
                    send_block_update(state, x, y, z, ns).await;
                }
            } else {
                drop(world);
            }
            continue;
        }

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
                // l'entrée d'un repeater est du côté OPPOSÉ à sa sortie (facing = direction de sortie)
                let input = get_block(&world, x - dx, y, z - dz);
                let input_power = get_block_power(input);
                let output_on = block == 94;
                // meta = facing (bits 0-1) + délai (bits 2-3), 0-3 -> 1 à 4 ticks.
                // L'état on/off est dans le block id (93/94), PAS dans le meta,
                // sinon ça entre en collision avec le bit haut du délai (bit 3
                // == 0x08 était utilisé pour les deux à la fois auparavant).
                let delay_ticks = ((m >> 2) & 0x03) as u64 + 1;
                let should_be_on = input_power > 0;
                if should_be_on != output_on {
                    let new_block = if should_be_on { 94 } else { 93 };
                    let new_st = encode(new_block, m);
                    schedule_delayed = Some((tick + delay_ticks, new_st));
                    new_stored = None;
                } else {
                    new_stored = None;
                }
            },
            149 | 150 => {
                let (dx, dz) = facing(m);
                // l'entrée d'un comparator est du côté OPPOSÉ à sa sortie (facing = direction de sortie)
                let input = get_block(&world, x - dx, y, z - dz);
                let input_power = get_block_power(input);
                let mode_subtract = (m & 0x04) != 0;
                let output_on = (m & 0x08) != 0;
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

        // Switches/plates are already written by the interaction/occupancy
        // code, so they have no `new_stored` here.  Their queued update is
        // nevertheless a real signal change and must notify consumers.
        let changed = new_stored.is_some()
            || matches!(block, 69 | 70 | 72 | 77 | 143 | 147 | 148 | 152);
        if let Some(ns) = new_stored {
            {
                let mut world = state.world.lock().await;
                world.insert((x, y, z), ns);
            }
            send_block_update(state, x, y, z, ns).await;
            crate::game::connection::notify_neighbors(state, x, y, z).await;
        }

        // A reset writes dust to zero before this loop runs.  Treat it as a
        // change too, so lamps/pistons observe the loss of power.
        if changed || reset_dust.contains(&(x, y, z)) {
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
                                        // Keep the upper-half marker (0x08) and hinge bit while
                                        // changing only the shared open bit.
                                        let top_open = if powered { (meta(top) & 0x0B) | 0x04 } else { meta(top) & 0x0B };
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
            if queue.contains(&(nx, ny, nz)) { continue; }
            let world = state.world.lock().await;
            let nb = block_id(get_block(&world, nx, ny, nz));
            drop(world);
            if visited.contains(&(nx, ny, nz)) && !(changed && nb == 55) { continue; }
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

    // Write actuation changes.  Pistons need a little more than metadata:
    // their head is a real block (34), and the block in front is moved.
    let mut block_updates = pending_actuations.clone();
    {
        let mut world = state.world.lock().await;
        for &(x, y, z, ns) in &pending_actuations {
            world.insert((x, y, z), ns);

            let piston = block_id(ns);
            if !matches!(piston, 29 | 33) {
                continue;
            }

            let piston_meta = meta(ns);
            let (dx, dy, dz) = piston_offset(piston_meta);
            let head = (x + dx, y + dy, z + dz);
            if (piston_meta & 0x08) != 0 {
                let mut chain: Vec<((i32, i32, i32), u16)> = Vec::new();
                let mut cursor = head;
                let mut has_space = false;
                for _ in 0..=12 {
                    let current = get_block(&world, cursor.0, cursor.1, cursor.2);
                    if block_id(current) == 0 {
                        has_space = true;
                        break;
                    }
                    if !piston_pushable(block_id(current)) || chain.len() == 12 {
                        break;
                    }
                    chain.push((cursor, current));
                    cursor = (cursor.0 + dx, cursor.1 + dy, cursor.2 + dz);
                }

                if has_space {
                    // Move from the far end first, so no block is overwritten.
                    for &((bx, by, bz), moved) in chain.iter().rev() {
                        let destination = (bx + dx, by + dy, bz + dz);
                        world.insert(destination, moved);
                        block_updates.push((destination.0, destination.1, destination.2, moved));
                    }
                    let head_meta = (piston_meta & 0x07) | if piston == 29 { 0x08 } else { 0 };
                    let head_block = encode(34, head_meta);
                    world.insert(head, head_block);
                    block_updates.push((head.0, head.1, head.2, head_block));
                } else {
                    // A piston cannot push more than 12 blocks (or an
                    // immovable block), so it remains retracted.
                    let retracted = encode(piston, piston_meta & !0x08);
                    world.insert((x, y, z), retracted);
                    block_updates.push((x, y, z, retracted));
                }
            } else {
                // Retract the head.  A sticky piston pulls back a single
                // pushable block from directly in front of the old head.
                if block_id(get_block(&world, head.0, head.1, head.2)) == 34 {
                    world.insert(head, 0);
                    block_updates.push((head.0, head.1, head.2, 0));
                }
                if piston == 29 {
                    let distant = (head.0 + dx, head.1 + dy, head.2 + dz);
                    let pulled = get_block(&world, distant.0, distant.1, distant.2);
                    if piston_pushable(block_id(pulled)) && block_id(get_block(&world, head.0, head.1, head.2)) == 0 {
                        world.insert(distant, 0);
                        world.insert(head, pulled);
                        block_updates.push((distant.0, distant.1, distant.2, 0));
                        block_updates.push((head.0, head.1, head.2, pulled));
                    }
                }
            }
        }
    }
    for &(x, y, z, ns) in &block_updates {
        send_block_update(state, x, y, z, ns).await;
        crate::game::connection::notify_neighbors(state, x, y, z).await;
        // Lamps, pistons, doors, etc. are passive consumers.  Scheduling the
        // dust around each visual/mechanical state change feeds the reset
        // pass again and makes a lamp oscillate forever.  Rails are the only
        // actuated blocks here that may relay redstone power further.
        if !matches!(block_id(ns), 27 | 28 | 157) {
            continue;
        }
        let mut queue = state.redstone_queue.lock().await;
        for (nx, ny, nz) in [(x-1,y,z),(x+1,y,z),(x,y-1,z),(x,y+1,z),(x,y,z-1),(x,y,z+1)] {
            if !queue.contains(&(nx, ny, nz)) {
                queue.push_back((nx, ny, nz));
            }
        }
    }
}

fn piston_offset(face: u8) -> (i32, i32, i32) {
    match face & 0x07 {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => (0, 0, 0),
    }
}

fn piston_pushable(block: u16) -> bool {
    !matches!(block,
        0 | 7 | 23 | 25 | 29 | 33 | 34 | 36 | 49 | 52 | 54 | 61 | 62 |
        84 | 90 | 116 | 117 | 118 | 119 | 120 | 130 | 137 | 138 | 145 |
        146 | 154 | 158
    )
}

/// Consumers (lamps, pistons, doors...) cannot change the power carried by
/// dust.  They must therefore never initiate a dust reset when placed.
fn can_reset_dust_from(block: u16, m: u8) -> bool {
    matches!(block, 0 | 75 | 76 | 93 | 94 | 149 | 150 | 69 | 70 | 72 |
                    77 | 143 | 147 | 148 | 152 | 157 | 28)
        || (block == 55 && m > 0)
}
