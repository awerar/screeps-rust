use std::{cmp::Reverse, iter, ops::{Add, Mul}, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{Creep, Part, RoomName, StructureSpawn, find, game, prelude::*};

use crate::{callbacks::Callback, creeps::{CreepData, CreepType}, memory::Memory, messages::{CreepMessage, SpawnMessage}, names::get_new_creep_name};

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
                        .flat_map(|(part, count)| vec![*part; count].into_iter())
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
        Body(self.0.into_iter().chain(rhs.0).collect())
    }
}

impl From<Part> for Body {
    fn from(value: Part) -> Self {
        Body(vec![value])
    }
}

impl From<&Creep> for Body {
    fn from(value: &Creep) -> Self {
        Body(value.body().into_iter().map(|bodypart| bodypart.part()).collect())
    }
}

struct CreepPrototype {
    body: Body,
    ty: CreepType,
    home: RoomName
}

impl CreepPrototype {
    fn try_from_existing(mem: &Memory, creep: Creep) -> Option<Self> {
        let creep_data = mem.creeps.get(&creep.name())?;

        Some(Self {
            body: Body(creep.body().into_iter().map(|part| part.part()).collect()),
            ty: creep_data.role.get_type(),
            home: creep_data.home
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

        let energy_capacity = if room.find(find::MY_CREEPS, None).len() >= 2 {
            room.energy_capacity_available()
        } else {
            room.energy_available()
        };

        Some(Self {
            name: spawn.name(),
            room: room.name(),
            energy_capacity,
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

            if let CreepType::Tugboat(tugged) = proto.ty {
                if let Some(tugged) = tugged.resolve() {
                    mem.messages.creep(&tugged).send(CreepMessage::AssignedTugBoat(name.clone()));
                }
            }

            let creep_data = CreepData::new(spawn.room().unwrap().name(), proto.ty.default_role());
            mem.creeps.insert(name.clone(), creep_data);

            let creep_death_time = game::time() + proto.body.time_to_spawn() + proto.body.time_to_live();
            mem.callbacks.schedule(creep_death_time, Callback::CreepCleanup(name));
        }
    }
}

struct PrototypeIterator<'a, T>(T) where T : Iterator<Item = &'a CreepPrototype>;

impl<'a, T> PrototypeIterator<'a, T> where T : Iterator<Item = &'a CreepPrototype> {
    fn filter_home(self, home: RoomName) -> PrototypeIterator<'a, impl Iterator<Item = &'a CreepPrototype>> {
        PrototypeIterator(self.0.filter(move |proto| proto.home == home))
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
fn schedule_excavators(mem: &Memory, schedule: &mut SpawnSchedule) {
    use Part::*;

    for colony in mem.colonies.keys() {
        let Some(room) = game::rooms().get(*colony) else { continue; };

        for source in room.find(find::SOURCES, None) {
            let any_excavator_already = schedule.all_creeps()
                .0.any(|proto| matches!(proto.ty, CreepType::Excavator(excavator_source) if excavator_source == source.id()));
            if any_excavator_already { continue; }

            let Some(spawner) = schedule.spawners().filter_room(room.name()).filter_free().0.next() else { continue; };

            let excavator_carry = 2 - ((spawner.energy_capacity % 100) / 50) as usize;

            let any_source_constructions = mem.colony(room.name()).unwrap()
                .plan.sources.0
                .get(&source.id())
                .is_some_and(|source_plan| source_plan.get_construction_site().is_some());

            let target_excavator_works = if any_source_constructions { 7 } else { 5 };
            let excavator_works = (spawner.energy_capacity as usize).saturating_sub(50 * excavator_carry).min(target_excavator_works);
            
            let body =  Body::from(Carry) * excavator_carry + Body::from(Work) * excavator_works;
            let prototype = CreepPrototype { 
                body, 
                ty: CreepType::Excavator(source.id()),
                home: *colony
            };

            spawner.schedule_or_block(prototype);
        }
    }
}

static WORKER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry, Work]) });
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
                home: *colony
            };

            if spawner.schedule_or_block(prototype) {
                total_worker_works += num_work;
            }
        }
    }
}


static FLAGSHIP_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Claim, Move]) });
fn schedule_flagships(mem: &Memory, schedule: &mut SpawnSchedule) {
    if mem.claim_requests.is_empty() { return; }

    let flagship_count = schedule.all_creeps().filter_type(CreepType::Flagship).0.count();
    if flagship_count > 0 { return; }

    let Some(spawner) = schedule.spawners().filter_free().0.next() else { return; };

    spawner.schedule_or_block(CreepPrototype { 
        body: FLAGSHIP_TEMPLATE.clone(), 
        ty: CreepType::Flagship, 
        home: spawner.room
    });
}

static REMOTE_BUILDER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry, Move, Carry, Move, Work]) });
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
            home: spawner.room
        }) {
            curr_works += num_work;
        }
    }
}

fn schedule_tugboats(mem: &mut Memory, schedule: &mut SpawnSchedule) {
    for msg in mem.messages.spawn.read_all() {
        #[expect(irrefutable_let_patterns)]
        let SpawnMessage::SpawnTugboatFor(tugged_id) = msg else { continue; };
        let Some(tugged) = tugged_id.resolve() else { continue; };
        let Some(home) = mem.creep(&tugged).map(|data| data.home) else { continue; };

        let Some(spawner) = schedule.spawners().filter_free().filter_room(home).0.next() else { continue; };

        let tugged_body = Body::from(&tugged);
        let target_tugboat_move_parts = tugged_body.0.len().saturating_sub(2 * tugged_body.num(Part::Move));

        if target_tugboat_move_parts == 0 {
            warn!("Creep {} has requested tugboat, but doesn't actually benefit from it", tugged.name());
        }

        spawner.schedule_or_block(CreepPrototype { 
            body: Body::from(Part::Move) * target_tugboat_move_parts.clamp(0, (spawner.energy_capacity / 50) as usize), 
            ty: CreepType::Tugboat(tugged_id), 
            home 
        });
    }
}

pub fn do_spawns(mem: &mut Memory) {
    let mut schedule = SpawnSchedule::new(mem);

    schedule_tugboats(mem, &mut schedule);
    //schedule_excavators(mem, &mut schedule);
    schedule_workers(mem, &mut schedule);
    schedule_flagships(mem, &mut schedule);
    schedule_remote_builders(mem, &mut schedule);

    schedule.execute(mem);
}