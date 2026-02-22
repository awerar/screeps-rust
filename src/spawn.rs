use std::{cmp::Reverse, iter, ops::{Add, Mul}, sync::LazyLock};

use itertools::Itertools;
use log::{debug, info, warn};
use screeps::{Creep, Part, ResourceType, RoomName, StructureSpawn, find, game, prelude::*};

use crate::{callbacks::Callback, creeps::{CreepData, CreepType}, memory::Memory, messages::{CreepMessage, SpawnMessage}, names::get_new_creep_name};

#[derive(Clone)]
struct Body(Vec<Part>);

impl Body {
    fn scaled(&self, energy: u32, min_parts: Option<usize>) -> Body {
        let min_parts = min_parts.unwrap_or(self.0.len());

        let mut counts: Vec<usize> = vec![0; self.0.len()];
        let mut cost = 0;
        let mut part_count = 0;

        loop {
            for (i, part )in self.0.iter().enumerate() {
                cost += part.cost();
                
                if energy < cost && part_count >= min_parts  {
                    return Body(self.0.iter()
                        .zip(counts.into_iter())
                        .flat_map(|(part, count)| vec![*part; count].into_iter())
                        .collect());
                }
                
                counts[i] += 1;
                part_count += 1;
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
    fn try_from_existing(mem: &Memory, creep: &Creep) -> Option<Self> {
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
    fn try_from(mem: &Memory, spawn: &StructureSpawn) -> Option<Self> {
        let room = spawn.room()?;
        let spawning = spawn.spawning()
            .and_then(|spawning| {
                let prototype = game::creeps().get(spawning.name().into())
                .and_then(|creep| CreepPrototype::try_from_existing(mem, &creep))?;

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
            if self.schedule(prototype) { true } else {
                self.status = SpawnerStatus::Blocked;
                false
            }
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
                .filter_map(|spawn| SpawnerData::try_from(mem, &spawn))
                .collect(),
            already_spawned: game::creeps().values()
                .filter_map(|creep| CreepPrototype::try_from_existing(mem, &creep))
                .collect()
        }
    }

    fn all_creeps(&self) -> PrototypeIterator<'_, impl Iterator<Item = &'_ CreepPrototype>> {
        PrototypeIterator(
            self.already_spawned.iter().chain(
                self.spawners.iter()
                .filter_map(|spawner| {
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
                warn!("Couldn't spawn creep: {err}");
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

fn schedule_excavators(mem: &Memory, schedule: &mut SpawnSchedule) {
    use Part::*;

    for colony in mem.colonies.keys() {
        let Some(room) = game::rooms().get(*colony) else { continue; };

        let any_excavator_in_colony = schedule.all_creeps()
            .filter_home(*colony).0
            .find(|proto| matches!(proto.ty, CreepType::Excavator(_)))
            .is_some();

        for source in room.find(find::SOURCES, None) {
            let any_excavator_already = schedule.all_creeps()
                .0.any(|proto| matches!(proto.ty, CreepType::Excavator(excavator_source) if excavator_source == source.id()));
            if any_excavator_already { continue; }

            let Some(spawner) = schedule.spawners().filter_room(room.name()).filter_free().0.next() else { continue; };

            let any_source_constructions = mem.colony(room.name()).unwrap()
                .plan.sources.source_plans
                .get(&source.id())
                .is_some_and(|source_plan| source_plan.get_construction_site().is_some());

            let energy = if any_excavator_in_colony { spawner.energy_capacity } else { spawner.energy_avaliable.max(300) };
            let target_excavator_works = if any_source_constructions { 7 } else { 5 };
            let excavator_works = (energy as usize).saturating_sub(50).min(target_excavator_works);
            
            let body = Body::from(Carry) + Body::from(Work) * excavator_works;

            let prototype = CreepPrototype { 
                body, 
                ty: CreepType::Excavator(source.id()),
                home: *colony
            };

            spawner.schedule_or_block(prototype);
        }
    }
}

// Truck capacity C = 50y energy
// Roundtrip time T = 2x ticks
// Production P = 10 energy per tick
// C/T = P => C = PT => 50y = 20x => y = 0.4x
const TRUCK_SOURCE_CARRY_PER_DIST: f32 = 0.4;

// Napkin math
// Truck capacity C = 50y
// Center radius R = 5 steps
// Roundtrip time T = 2R = 10 ticks
// Consumption P = 20
// C = PT => 50y = 200 => y = 4
// Creep cost = 1.5y * 50 / 1500 = 0.2
const TRUCK_CENTER_CARRY: f32 = 4.0;
const TRUCK_FABRICATOR_CARRY: f32 = 10.0; // TODO: Fix this properly

const TRUCK_CARRY_MARGIN: f32 = 0.25;

static TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry, Carry]) });
static MAX_TRUCK_ENERGY: LazyLock<u32> = LazyLock::new(||  (TRUCK_TEMPLATE.clone() * 10).energy_required());
fn schedule_trucks(mem: &Memory, schedule: &mut SpawnSchedule) {
    use Part::*;

    for (colony, colony_data) in &mem.colonies {
        let total_carry_for_sources = colony_data.plan.sources.source_plans.values()
            .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
            .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
            .sum::<f32>();

        let target_carry = ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize;
        debug!("Target truck carry in {colony}: {target_carry}");
        let mut current_carry = schedule.all_creeps().filter_home(*colony).filter_type(CreepType::Truck).part_count(Carry);

        for spawner in schedule.spawners().filter_free().filter_room(*colony).0 {
            if current_carry >= target_carry { break; }

            let energy = if current_carry == 0 { spawner.energy_avaliable } else { spawner.energy_capacity };
            let body = TRUCK_TEMPLATE.scaled(energy.min(*MAX_TRUCK_ENERGY), Some(2));
            let creep_carry = body.num(Carry);

            let proto = CreepPrototype { ty: CreepType::Truck, home: *colony, body };
            spawner.schedule_or_block(proto);
            current_carry += creep_carry;
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

const TARGET_IDLE_FABRICATOR_WORK_COUNT: usize = 20;
const TARGET_SURPLUS_FABRICATOR_WORK_COUNT: usize = 40;
const BUFFER_ENERGY_SURPLUS_THRESHOLD: u32 = 50_000;
static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Carry, Carry, Move, Work, Carry]) });
fn schedule_fabricators(mem: &mut Memory, schedule: &mut SpawnSchedule) {
    for (colony, colony_data) in &mem.colonies {
        let mut curr_work_count = schedule.all_creeps().filter_home(*colony).filter_type(CreepType::Fabricator).part_count(Part::Work);
        
        let buffer_energy = colony_data.buffer().map_or(0, |buffer| buffer.store().get_used_capacity(Some(ResourceType::Energy)));
        let work_target = if buffer_energy >= BUFFER_ENERGY_SURPLUS_THRESHOLD { TARGET_SURPLUS_FABRICATOR_WORK_COUNT } else { TARGET_IDLE_FABRICATOR_WORK_COUNT };

        let spawners = schedule.spawners()
            .filter_room(*colony)
            .filter_free()
            .0.sorted_by_key(|spawner| Reverse(spawner.energy_capacity));

        for spawner in spawners {
            if curr_work_count >= work_target { break; }

            let body = FABRICATOR_TEMPLATE.scaled(spawner.energy_capacity, None);
            let body_work_count = body.num(Part::Work);

            if spawner.schedule(CreepPrototype { 
                body, 
                ty: CreepType::Fabricator, 
                home: spawner.room
            }) {
                curr_work_count += body_work_count;
            }
        }
    }
}

pub fn do_spawns(mem: &mut Memory) {
    let mut schedule = SpawnSchedule::new(mem);

    schedule_trucks(mem, &mut schedule);
    schedule_tugboats(mem, &mut schedule);
    schedule_excavators(mem, &mut schedule);
    schedule_fabricators(mem, &mut schedule);
    schedule_flagships(mem, &mut schedule);

    schedule.execute(mem);
}