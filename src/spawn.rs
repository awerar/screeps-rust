use std::{cmp::Reverse, iter, ops::{Add, Mul}, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{Creep, Part, RoomName, StructureSpawn, find, game, prelude::*};

use crate::{callbacks::Callback, creeps::{CreepConfig, CreepType}, memory::Memory, names::get_new_creep_name};

#[derive(Clone)]
struct Body(Vec<Part>);

impl Body {
    fn scaled(&self, energy: u32, min_parts: Option<usize>) -> Option<Body> {
        let mut counts: Vec<usize> = vec![0; self.0.len()];
        let mut cost = 0;

        let min_parts = min_parts.unwrap_or(self.0.len());

        loop {
            for (i, part )in self.0.iter().enumerate() {
                cost += part.cost();

                if cost > energy {
                    let body = Body(self.0.iter()
                        .zip(counts.into_iter())
                        .flat_map(|(part, count)| vec![part.clone(); count].into_iter())
                        .collect());
                    
                    if body.0.len() > min_parts {
                        return Some(body);
                    } else {
                        return None;
                    }
                }

                counts[i] += 1;
            }
        }
    }

    fn energy_required(&self) -> u32 {
        self.0.iter().map(|part| part.cost()).sum()
    }

    fn time_to_live(&self) -> u32 {
        if self.0.contains(&Part::Claim) { 600 } else { 1500 }
    }

    fn time_to_spawn(&self) -> u32 {
        (self.0.len() * 3) as u32
    }

    fn num(&self, part: Part) -> usize {
        self.0.iter().filter(|p| **p == part).count()
    }
}

impl Mul<usize> for Body {
    type Output = Self;

    fn mul(self, rhs: usize) -> Self::Output {
        Body(self.0.into_iter().flat_map(|part| iter::repeat_n(part, rhs)).collect())
    }
}

impl Add for Body {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Body(self.0.into_iter().chain(rhs.0.into_iter()).collect())
    }
}

impl From<Part> for Body {
    fn from(value: Part) -> Self {
        Body(vec![value])
    }
}

struct CreepPrototype {
    body: Body,
    ty: CreepType,
    config: CreepConfig
}

impl CreepPrototype {
    fn try_from_existing(mem: &Memory, creep: Creep) -> Option<Self> {
        Some(Self {
            body: Body(creep.body().into_iter().map(|part| part.part()).collect()),
            ty: mem.machines.creeps.get(&creep.name())?.get_type(),
            config: mem.creeps.get(&creep.name())?.clone()
        })
    }
}

type TicksLeft = u32;

enum SpawnerStatus {
    Free,
    Blocked,
    Scheduled(CreepPrototype),
    #[expect(unused)]
    Spawning(CreepPrototype, TicksLeft)
}

impl SpawnerStatus {
    fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }
}

struct SpawnerData {
    name: String,
    room: RoomName,
    energy_capacity: u32,
    energy_avaliable: u32,

    status: SpawnerStatus,
}

impl SpawnerData {
    fn try_from(mem: &Memory, spawn: StructureSpawn) -> Option<Self> {
        let room = spawn.room()?;
        let spawning = spawn.spawning()
            .and_then(|spawning| {
                let prototype = game::creeps().get(spawning.name().into())
                .and_then(|creep| CreepPrototype::try_from_existing(mem, creep))?;

                Some((prototype, spawning.remaining_time()))
            });

        Some(Self {
            name: spawn.name(),
            room: room.name(),
            energy_capacity: room.energy_capacity_available(),
            energy_avaliable: room.energy_available(),
            status: spawning.map_or(SpawnerStatus::Free, |(proto, time_left)| SpawnerStatus::Spawning(proto, time_left)),
        })
    }

    fn schedule(&mut self, prototype: CreepPrototype) -> bool {
        if self.is_free() && self.energy_avaliable >= prototype.body.energy_required() {
            self.status = SpawnerStatus::Scheduled(prototype);
            true
        } else { false }
    }

    fn schedule_or_block(&mut self, prototype: CreepPrototype) -> bool {
        if self.is_free() {
            if !self.schedule(prototype) {
                self.status = SpawnerStatus::Blocked;
                false
            } else { true }
        } else { false }
    }

    fn is_free(&self) -> bool {
        self.status.is_free()
    }
}

struct SpawnSchedule {
    spawners: Vec<SpawnerData>,
    already_spawned: Vec<CreepPrototype>
}

impl SpawnSchedule {
    fn new(mem: &Memory) -> Self {
        Self {
            spawners: game::spawns().values()
                .flat_map(|spawn| SpawnerData::try_from(mem, spawn))
                .collect(),
            already_spawned: game::creeps().values()
                .flat_map(|creep| CreepPrototype::try_from_existing(mem, creep))
                .collect()
        }
    }

    fn all_creeps(&self) -> PrototypeIterator<'_, impl Iterator<Item = &'_ CreepPrototype>> {
        PrototypeIterator(
            self.already_spawned.iter().chain(
                self.spawners.iter()
                .flat_map(|spawner| {
                    use SpawnerStatus::*;

                    match &spawner.status {
                        Free | Blocked | Spawning(_, _) => None,
                        Scheduled(proto) => Some(proto)
                    }
                })
            )
        )
    }

    fn spawners(&mut self) -> SpawnerIterator<'_, impl Iterator<Item = &'_ mut SpawnerData>> {
        SpawnerIterator(self.spawners.iter_mut())
    }

    fn execute(self, mem: &mut Memory) {
        for data in self.spawners {
            let Some(spawn) = game::spawns().get(data.name) else { continue; };
            let SpawnerStatus::Scheduled(proto) = data.status else { continue; };

            if let Some(spawning) = spawn.spawning() {
                warn!("Cancelling spawn of {}", spawning.name());
                spawning.cancel().ok();
            }

            let name = format!("{} {}", proto.ty.prefix(), get_new_creep_name());
            info!("Spawning new creep: {name}");

            if let Err(err) = spawn.spawn_creep(&proto.body.0, &name) {
                warn!("Couldn't spawn creep: {}", err);
                continue;
            }

            mem.machines.creeps.insert(name.clone(), proto.ty.default_role());

            let creep_death_time = game::time() + proto.body.time_to_spawn() + proto.body.time_to_live();
            mem.callbacks.schedule(creep_death_time, Callback::CreepCleanup(name));
        }
    }
}

struct PrototypeIterator<'a, T>(T) where T : Iterator<Item = &'a CreepPrototype>;

impl<'a, T> PrototypeIterator<'a, T> where T : Iterator<Item = &'a CreepPrototype> {
    fn filter_home(self, home: RoomName) -> PrototypeIterator<'a, impl Iterator<Item = &'a CreepPrototype>> {
        PrototypeIterator(self.0.filter(move |proto| proto.config.home == home))
    }

    fn filter_type(self, ty: CreepType) -> PrototypeIterator<'a, impl Iterator<Item = &'a CreepPrototype>> {
        PrototypeIterator(self.0.filter(move |proto| proto.ty == ty))
    }

    fn part_count(self, part: Part) -> usize {
        self.0.map(|proto| proto.body.num(part)).sum()
    }
}

struct SpawnerIterator<'a, T>(T) where T : Iterator<Item = &'a mut SpawnerData>;

impl<'a, T> SpawnerIterator<'a, T> where T : Iterator<Item = &'a mut SpawnerData> {
    fn filter_room(self, room: RoomName) -> SpawnerIterator<'a, impl Iterator<Item = &'a mut SpawnerData>> {
        SpawnerIterator(self.0.filter(move |spawner| spawner.room == room))
    }

    fn filter_free(self) -> SpawnerIterator<'a, impl Iterator<Item = &'a mut SpawnerData>> {
        SpawnerIterator(self.0.filter(|spawner| spawner.is_free()))
    }
}

#[expect(unused)]
fn schedule_harvesters(mem: &Memory, schedule: &mut SpawnSchedule) {
    use Part::*;

    for colony in mem.colonies.keys() {
        let Some(room) = game::rooms().get(*colony) else { continue; };

        let mut total_harvester_works: usize = schedule.all_creeps()
            .filter_home(*colony)
            .filter_type(CreepType::Harvester)
            .part_count(Part::Work);

        let target_harvester_works = 5 * room.find(find::SOURCES, None).len();

        for spawner in schedule.spawners().filter_room(*colony).filter_free().0 {
            if total_harvester_works >= target_harvester_works { break; };

            let harvester_moves = (2 + (spawner.energy_capacity % 100) / 50) as usize;
            let harvester_works = (spawner.energy_capacity as usize).saturating_sub(50 * harvester_moves).min(5);
            let body =  Body::from(Move) * harvester_moves as usize + Body::from(Work) * harvester_works as usize;
            let prototype = CreepPrototype { 
                body, 
                ty: CreepType::Harvester,
                config: CreepConfig { home: *colony }
            };

            if spawner.schedule_or_block(prototype) {
                total_harvester_works += harvester_works;
            }
        }
    }
}

const WORKER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry, Work]) });
fn schedule_workers(mem: &Memory, schedule: &mut SpawnSchedule) {
    use Part::*;

    for colony in mem.colonies.keys() {
        let Some(room) = game::rooms().get(*colony) else { continue; };

        let mut total_worker_works: usize = schedule.all_creeps()
                .filter_home(*colony)
                .filter_type(CreepType::Worker)
                .part_count(Part::Work);

        let target_worker_works = 5 * room.find(find::SOURCES, None).len() * 4;

        for spawner in schedule.spawners().filter_room(*colony).filter_free().0 {
            if total_worker_works >= target_worker_works { continue; };

            let Some(body) = WORKER_TEMPLATE.scaled(spawner.energy_capacity, None) else { continue; };
            let num_work = body.num(Work);
            let prototype = CreepPrototype { 
                body, 
                ty: CreepType::Worker,
                config: CreepConfig { home: *colony }
            };

            if spawner.schedule_or_block(prototype) {
                total_worker_works += num_work;
            }
        }
    }
}


const CLAIMER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Claim, Move]) });
fn schedule_claimers(mem: &Memory, schedule: &mut SpawnSchedule) {
    if mem.claim_requests.len() == 0 { return; }

    let claimer_count = schedule.all_creeps().filter_type(CreepType::Claimer).0.count();
    if claimer_count > 0 { return; }

    let Some(spawner) = schedule.spawners().filter_free().0.next() else { return; };

    spawner.schedule_or_block(CreepPrototype { 
        body: CLAIMER_TEMPLATE.clone(), 
        ty: CreepType::Claimer, 
        config: CreepConfig::new(spawner.room)
    });
}

const REMOTE_BUILDER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry, Move, Carry, Move, Work]) });
fn schedule_remote_builders(mem: &mut Memory, schedule: &mut SpawnSchedule) {
    let target_works = mem.remote_build_requests.get_total_work_ticks().div_ceil(750) as usize;
    let mut curr_works = schedule.all_creeps().filter_type(CreepType::RemoteBuilder).part_count(Part::Work);

    let spawners = schedule.spawners()
        .filter_free()
        .0.sorted_by_key(|spawner| Reverse(spawner.energy_capacity));

    for spawner in spawners {
        if curr_works >= target_works { break; }

        let Some(body) = REMOTE_BUILDER_TEMPLATE.scaled(spawner.energy_capacity, Some(6)) else { continue; };
        let num_work = body.num(Part::Work);

        if spawner.schedule(CreepPrototype { 
            body, 
            ty: CreepType::RemoteBuilder, 
            config: CreepConfig::new(spawner.room) 
        }) {
            curr_works += num_work;
        }
    }
}

pub fn do_spawns(mem: &mut Memory) {
    let mut schedule = SpawnSchedule::new(mem);

    //schedule_harvesters(mem, &mut schedule);
    schedule_workers(mem, &mut schedule);
    schedule_claimers(mem, &mut schedule);
    schedule_remote_builders(mem, &mut schedule);

    schedule.execute(mem);
}