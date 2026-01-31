use std::{collections::HashMap, ops::{Deref, DerefMut}, sync::LazyLock};

use log::*;
use screeps::{Part, game, prelude::*};

use crate::{creeps::{Role, get_missing_roles}, memory::Memory, names::get_new_creep_name};

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
}

pub const HARVESTER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| BodyTemplate(vec![Part::Carry, Part::Move, Part::Work]));
pub const CLAIMER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| BodyTemplate(vec![Part::Claim, Part::Move]));

pub fn do_spawns(memory: &mut Memory) {
    let mut room_queues = HashMap::new();

    for spawn in game::spawns().values() {
        if spawn.spawning().is_some() { continue; }

        let room = spawn.room().unwrap();
        let queue = room_queues.entry(room.name()).or_insert_with(|| get_missing_roles(memory, &room).into_iter());

        let Some(role) = queue.next() else { continue; };

        let body = match role {
            Role::Worker(_) => HARVESTER_TEMPLATE.scaled(room.energy_capacity_available(), None),
            Role::Claimer(_) => Some(CLAIMER_TEMPLATE.clone()),
        };

        let Some(body) = body else { continue; };

        if room.energy_available() >= body.energy_required() {
            let prefix = match role {
                Role::Worker(_) => "Worker",
                Role::Claimer(_) => "Claimer",
            };

            let name = format!("{prefix} {}", get_new_creep_name());
            info!("Spawning new creep: {name}");

            if let Err(err) = spawn.spawn_creep(&body, &name) {
                warn!("Couldn't spawn creep: {}", err);
                continue;
            }

            if matches!(role, Role::Claimer(_)) {
                memory.claimer_creep = Some(name.clone());
            }

            memory.creeps.insert(name, role);
        }
    }
}