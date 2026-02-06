use std::{collections::{HashMap, VecDeque}, ops::{Deref, DerefMut}, sync::LazyLock};

use log::*;
use screeps::{Part, RoomName, game, prelude::*};

use crate::{callbacks::Callback, creeps::CreepType, memory::Memory, names::get_new_creep_name};

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

struct SpawnQueue {
    pub queues: HashMap<RoomName, VecDeque<CreepType>>,

    colony_roles: HashMap<RoomName, HashMap<CreepType, usize>>,
    total_roles: HashMap<CreepType, usize>
}

impl SpawnQueue {
    fn new(mem: &Memory) -> Self {
        let colony_queues: HashMap<_, _> = mem.colonies.keys().cloned().map(|colony| (colony, VecDeque::new())).collect();
        let mut colony_roles: HashMap<_, _> = mem.colonies.keys().cloned().map(|colony| (colony, HashMap::new())).collect();
        let mut total_roles = HashMap::new();

        for (creep_name, creep) in game::creeps().entries() {
            let Some(role) = mem.machines.creeps.get(&creep_name) else {  continue; };
            let creep_type = role.get_type();

            *total_roles.entry(creep_type).or_default() += 1;

            let Some(home) = mem.creep(&creep).map(|config| config.home) else { continue; };
            *colony_roles.entry(home).or_default().entry(creep_type).or_default() += 1;
        }

        SpawnQueue { queues: colony_queues, colony_roles, total_roles }
    }

    fn queue_many_in_colony(&mut self, colony: RoomName, ty: CreepType, count: usize) {
        self.queues.entry(colony).or_default().extend((0..count).map(|_| ty));
        *self.colony_roles.entry(colony).or_default().entry(ty).or_default() += count;
        *self.total_roles.entry(ty).or_default() += count;
    }

    fn queue_in_colony(&mut self, colony: RoomName, ty: CreepType) {
        self.queue_many_in_colony(colony, ty, 1);
    }

    fn queue_missing_in_colony(&mut self, colony: RoomName, ty: CreepType, target: usize) {
        let current = self.colony_roles.get(&colony)
            .and_then(|roles| roles.get(&ty).cloned()).unwrap_or(0);


        self.queue_many_in_colony(colony, ty, target.saturating_sub(current));
    }

    fn queue_distributed(&mut self, ty: CreepType) -> Result<(), ()> {
        let (colony, _) = self.queues.iter().min_by_key(|(_, queue)| queue.len()).ok_or(())?;
        self.queue_in_colony(*colony, ty);
        Ok(())
    }

    fn queue_many_distributed(&mut self, ty: CreepType, count: usize) -> Result<(), ()> {
        for _ in 0..count {
            self.queue_distributed(ty)?;
        }
        
        Ok(())
    }

    fn queue_missing_distributed(&mut self, ty: CreepType, target: usize) -> Result<(), ()> {
        let current = self.total_roles.get(&ty).cloned().unwrap_or(0);
        let missing = target.saturating_sub(current);

        self.queue_many_distributed(ty, missing)
    }
}

fn get_current_spawn_queue(mem: &mut Memory) -> HashMap<RoomName, VecDeque<CreepType>> {
    use CreepType::*;

    let mut queue = SpawnQueue::new(mem);

    for colony in mem.colonies.keys().cloned().collect::<Vec<_>>() {
        let target_harvesters = mem.source_assignments(colony).map(|assignments| assignments.max_creeps()).unwrap_or(0);
        queue.queue_missing_in_colony(colony, Worker, target_harvesters);
    }

    queue.queue_missing_distributed(Claimer, (mem.claim_requests.len() > 0).then_some(1).unwrap_or(0)).ok();


    let target_remote_builders = mem.remote_build_requests.get_total_work_ticks().div_ceil(500) as usize;
    queue.queue_missing_distributed(RemoteBuilder, target_remote_builders).ok();

    queue.queues
}

pub const HARVESTER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| { use Part::*; BodyTemplate(vec![Move, Carry, Work]) });
pub const CLAIMER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| { use Part::*; BodyTemplate(vec![Claim, Move]) });
pub const REMOTE_BUILDER_TEMPLATE: LazyLock<BodyTemplate> = LazyLock::new(|| { use Part::*; BodyTemplate(vec![Move, Carry, Move, Carry, Move, Work]) });

pub fn do_spawns(mem: &mut Memory) {
    let mut colony_queues = get_current_spawn_queue(mem);

    for spawn in game::spawns().values() {
        if spawn.spawning().is_some() { continue; }

        let room = spawn.room().unwrap();
        let queue = colony_queues.entry(room.name()).or_default();
        //debug!("Spawn queue for {}: {queue:?}", room.name());
        
        let Some(ty) = queue.front() else { continue; };

        let body = match ty {
            CreepType::Worker => HARVESTER_TEMPLATE.scaled(room.energy_capacity_available(), None),
            CreepType::Claimer => Some(CLAIMER_TEMPLATE.clone()),
            CreepType::RemoteBuilder => REMOTE_BUILDER_TEMPLATE.scaled(room.energy_capacity_available(), Some(6))
        };

        let Some(body) = body else { continue; };

        if room.energy_available() >= body.energy_required() {
            let name = format!("{} {}", ty.prefix(), get_new_creep_name());
            info!("Spawning new creep: {name}");

            if let Err(err) = spawn.spawn_creep(&body, &name) {
                warn!("Couldn't spawn creep: {}", err);
                continue;
            }

            mem.machines.creeps.insert(name.clone(), ty.default_role());

            let creep_death_time = game::time() + body.time_to_spawn() + body.time_to_live();
            mem.callbacks.schedule(creep_death_time, Callback::CreepCleanup(name));
        }
    }
}