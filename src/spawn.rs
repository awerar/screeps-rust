use std::{collections::HashMap, ops::{Deref, DerefMut}, sync::LazyLock};

use log::*;
use screeps::{Part, game, prelude::*};

use crate::{callbacks::Callback, creeps::{CreepRole, get_missing_roles}, memory::Memory, names::get_new_creep_name};

#[derive(Clone)]
pub struct BodyTemplate(Vec<Part>);

impl Deref for BodyTemplate {
    type Target = Vec<Part>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BodyTemplate {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl BodyTemplate {
    pub fn scaled(&self, energy: u32, min_parts: Option<usize>) -> Option<BodyTemplate> {
        let mut counts: Vec<usize> = vec![0; self.len()];
        let mut cost = 0;

        let min_parts = min_parts.unwrap_or(self.len());

        loop {
            for (i, part )in self.iter().enumerate() {
                cost += part.cost();

                if cost > energy {
                    let body = BodyTemplate(self.iter()
                        .zip(counts.into_iter())
                        .flat_map(|(part, count)| vec![part.clone(); count].into_iter())
                        .collect());
                    
                    if body.len() > min_parts {
                        return Some(body);
                    } else {
                        return None;
                    }
                }

                counts[i] += 1;
            }
        }
    }

    pub fn energy_required(&self) -> u32 {
        self.iter().map(|part| part.cost()).sum()
    }

    pub fn time_to_live(&self) -> u32 {
        if self.contains(&Part::Claim) { 600 } else { 1500 }
    }

    pub fn time_to_spawn(&self) -> u32 {
        (self.len() * 3) as u32
    }
}

pub const HARVESTER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| BodyTemplate(vec![Part::Move, Part::Carry, Part::Work]));
pub const CLAIMER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| BodyTemplate(vec![Part::Claim, Part::Move]));

pub fn do_spawns(memory: &mut Memory) {
    let mut room_queues = HashMap::new();

    for spawn in game::spawns().values() {
        if spawn.spawning().is_some() { continue; }

        let room = spawn.room().unwrap();
        let queue = room_queues.entry(room.name()).or_insert_with(|| get_missing_roles(memory, &room).into_iter());

        let Some(role) = queue.next() else { continue; };

        let body = match role {
            CreepRole::Worker(_) => HARVESTER_TEMPLATE.scaled(room.energy_capacity_available(), None),
            CreepRole::Claimer(_) => Some(CLAIMER_TEMPLATE.clone()),
        };

        let Some(body) = body else { continue; };

        if room.energy_available() >= body.energy_required() {
            let prefix = match role {
                CreepRole::Worker(_) => "Worker",
                CreepRole::Claimer(_) => "Claimer",
            };

            let name = format!("{prefix} {}", get_new_creep_name());
            info!("Spawning new creep: {name}");

            if let Err(err) = spawn.spawn_creep(&body, &name) {
                warn!("Couldn't spawn creep: {}", err);
                continue;
            }

            if matches!(role, CreepRole::Claimer(_)) {
                memory.shared.claimer_creep = Some(name.clone());
            }

            memory.creeps.insert(name.clone(), role);

            let creep_death_time = game::time() + body.time_to_spawn() + body.time_to_live();
            memory.shared.callbacks.schedule(creep_death_time, Callback::CreepCleanup(name));
        }
    }
}