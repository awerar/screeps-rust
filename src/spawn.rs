use std::{iter, ops::{Add, Mul}, sync::LazyLock};

use log::{info, warn};
use screeps::{Creep, MAX_CREEP_SIZE, Part, RoomName, StructureSpawn, find, game};

use crate::{colony::{planning::plan::SourcePlan, steps::ColonyStep}, commands::{Command, pop_command}, creeps::{CreepData, CreepRole, excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::{ImportTruckState, TruckCreep}}, domain_traits::{CreepId, EnergyStoreAccessors, HasId, HasName}, memory::Memory, names::{UsedNames, generate_new_creep_name}};

#[derive(Clone)]
pub struct Body(Vec<Part>);

impl Body {
    fn scaled(&self, energy: u32, min_parts: Option<usize>) -> Body {
        let min_parts = min_parts.unwrap_or(self.0.len());

        let mut counts: Vec<usize> = vec![0; self.0.len()];
        let mut cost = 0;
        let mut part_count = 0;

        loop {
            for (i, part )in self.0.iter().enumerate() {
                cost += part.cost();
                
                if part_count >= MAX_CREEP_SIZE || (energy < cost && part_count as usize >= min_parts)  {
                    return Body(self.0.iter()
                        .zip(counts)
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

    pub fn num(&self, part: Part) -> usize {
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
    role: CreepRole,
    home: RoomName
}

impl CreepPrototype {
    fn try_from_existing(mem: &Memory, creep: &Creep) -> Option<Self> {
        let creep_data = mem.creeps.get(&creep.id())?;

        Some(Self {
            body: Body(creep.body().into_iter().map(|part| part.part()).collect()),
            role: creep_data.role.clone(),
            home: creep_data.home
        })
    }
}

type TicksLeft = u32;

enum SpawnerStatus {
    Free,
    Blocked,
    Scheduled { name: String, proto: CreepPrototype },
    #[expect(unused)]
    Spawning(CreepPrototype, TicksLeft)
}

impl SpawnerStatus {
    fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }
}

struct SpawnerData {
    structure: StructureSpawn,
    room: RoomName,
    energy_capacity: u32,
    energy_avaliable: u32,

    status: SpawnerStatus,
}

impl SpawnerData {
    fn from(mem: &Memory, spawn: &StructureSpawn) -> Self {
        let room = spawn.room().unwrap();
        let spawning = spawn.spawning()
            .and_then(|spawning| {
                let creep = game::creeps().get(spawning.name().into())?;
                let prototype = CreepPrototype::try_from_existing(mem, &creep)?;

                Some((prototype, spawning.remaining_time()))
            });

        let energy_capacity = if room.find(find::MY_CREEPS, None).len() >= 2 {
            room.energy_capacity_available()
        } else {
            room.energy_available()
        };

        Self {
            structure: spawn.clone(),
            room: room.name(),
            energy_capacity,
            energy_avaliable: room.energy_available(),
            status: spawning.map_or(SpawnerStatus::Free, |(proto, time_left)| SpawnerStatus::Spawning(proto, time_left)),
        }
    }

    fn schedule(&mut self, used_names: &mut UsedNames, prototype: CreepPrototype) -> Option<CreepId> {
        if self.is_free() && self.energy_avaliable >= prototype.body.energy_required() {
            if pop_command(Command::DebugSpawn) { info!("Scheduling creep {:?}", prototype.role) }

            let name = generate_new_creep_name(prototype.role.prefix(), used_names);
            self.status = SpawnerStatus::Scheduled { name: name.clone(), proto: prototype };
            
            Some(CreepId::Name(name))
        } else {
            if pop_command(Command::DebugSpawn) { info!("Unable to schedule creep {:?}", prototype.role) }
            None
        }
    }

    fn schedule_or_block(&mut self, used_names: &mut UsedNames, prototype: CreepPrototype) -> Option<CreepId> {
        if self.is_free() {
            if let Some(handle) = self.schedule(used_names, prototype) { 
                Some(handle) 
            } else {
                self.status = SpawnerStatus::Blocked;
                None
            }
        } else { None }
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
                .map(|spawn| SpawnerData::from(mem, &spawn))
                .collect(),
            already_spawned: game::creeps().values()
                .filter_map(|creep| CreepPrototype::try_from_existing(mem, &creep))
                .collect()
        }
    }

    fn all_creeps(&self) -> impl Iterator<Item = &'_ CreepPrototype> {
        self.already_spawned.iter().chain(
            self.spawners.iter()
            .filter_map(|spawner| {
                use SpawnerStatus::*;

                match &spawner.status {
                    Free | Blocked | Spawning(_, _) => None,
                    Scheduled { proto, .. } => Some(proto)
                }
            })
        )
    }

    fn spawners(&mut self) -> impl Iterator<Item = &'_ mut SpawnerData> {
        self.spawners.iter_mut()
    }

    fn execute(self, mem: &mut Memory) {
        for data in self.spawners {
            let SpawnerStatus::Scheduled { proto, name } = data.status else { continue; };

            if let Some(spawning) = data.structure.spawning() {
                warn!("Cancelling spawn of {}", spawning.name());
                spawning.cancel().ok();
            }

            info!("Spawning new creep: {name}");

            if let Err(err) = data.structure.spawn_creep(&proto.body.0, &name) {
                warn!("Couldn't spawn creep: {err}");
                continue;
            }

            let creep_data = CreepData::new(proto.home, proto.role);
            mem.creeps.insert(CreepId::Name(name.clone()), creep_data);
        }
    }
}

trait CreepPrototypeIteratorExt<'a>: Iterator<Item = &'a CreepPrototype> + Sized {
    fn filter_home(self, home: RoomName) -> impl Iterator<Item = &'a CreepPrototype> {
        self.filter(move |proto| proto.home == home)
    }

    fn filter_role(self, f: impl Fn(&CreepRole) -> bool) -> impl Iterator<Item = &'a CreepPrototype> {
        self.filter(move |proto| f(&proto.role))
    }

    fn part_count(self, part: Part) -> usize {
        self.map(|proto| proto.body.num(part)).sum()
    }
}

impl<'a, T: Iterator<Item = &'a CreepPrototype> + Sized> CreepPrototypeIteratorExt<'a> for T { }

trait SpawnerDataIteratorExt<'a>: Iterator<Item = &'a mut SpawnerData> + Sized {
    fn filter_room(self, room: RoomName) -> impl Iterator<Item = &'a mut SpawnerData> {
        self.filter(move |spawner| spawner.room == room)
    }

    fn filter_free(self) -> impl Iterator<Item = &'a mut SpawnerData> {
        self.filter(|spawner| spawner.is_free())
    }
}

impl<'a, T: Iterator<Item = &'a mut SpawnerData> + Sized> SpawnerDataIteratorExt<'a> for T { }

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() { 7 } else { 5 };
    let excavator_works = energy.saturating_sub(50).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::from(Part::Carry) + Body::from(Part::Work) * (excavator_works as usize)
}

fn schedule_excavators(mem: &Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        for (source, source_plan) in &colony.plan.sources {
            let Some(source) = source.resolve() else { continue; };

            let any_excavator_already = schedule.all_creeps()
                .any(|proto| matches!(&proto.role, CreepRole::Excavator(_, excavator_source) if *excavator_source == source.id()));
            if any_excavator_already { continue; }

            let Some(spawner) = schedule.spawners().filter_room(colony.name).filter_free().next() else { continue; };

            let prototype = CreepPrototype { 
                body: get_excavator_body(spawner.energy_capacity, source_plan), 
                role: CreepRole::Excavator(ExcavatorCreep::default(), source.id()),
                home: colony.name
            };

            spawner.schedule_or_block(used_names, prototype);
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
fn get_truck_body(energy: u32) -> Body {
    TRUCK_TEMPLATE.scaled(energy.min(*MAX_TRUCK_ENERGY), Some(2))
}

fn schedule_trucks(mem: &Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    use Part::*;

    for colony in mem.colonies.view_all() {
        let total_carry_for_sources = colony.plan.sources.values()
            .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
            .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
            .sum::<f32>();

        let target_carry = ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize;

        loop {
            let current_carry = schedule.all_creeps().filter_home(colony.name).filter_role(|role| matches!(role, CreepRole::Truck(_))).part_count(Carry);
            if current_carry >= target_carry { break; }

            let Some(spawner) = schedule.spawners().filter_free().filter_room(colony.name).next() else { break };
            if spawner.schedule_or_block(used_names, CreepPrototype { 
                role: CreepRole::Truck(TruckCreep::default()), 
                home: colony.name, 
                body: get_truck_body(spawner.energy_capacity)
            }).is_none() { break }
        }
    }
}

static IMPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry]) });
fn schedule_import_trucks(mem: &mut Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        if !matches!(colony.step, ColonyStep::BuildSpawn) { continue; }
        if schedule.all_creeps().filter_home(colony.name).filter_role(|role| matches!(role, CreepRole::ImportTruck(_))).part_count(Part::Carry) > 100 { continue; }

        let Some(spawn) = schedule.spawners().filter_free().next() else { continue; };
        spawn.schedule(used_names, CreepPrototype { 
            body: IMPORT_TRUCK_TEMPLATE.scaled(spawn.energy_capacity, None), 
            role: CreepRole::ImportTruck(ImportTruckState::default()), 
            home: colony.name
        });
    }
}

static FLAGSHIP_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Claim, Move]) });
fn schedule_flagships(mem: &mut Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    let coordinator = &mut mem.flagship_coordinator;
    if coordinator.rooms.is_empty() { return; }

    let flagship_count = schedule.all_creeps().filter_role(|role| matches!(role, CreepRole::Flagship(_))).count();
    if flagship_count > 0 { return; }

    let Some(spawner) = schedule.spawners().filter_free().next() else { return; };

    spawner.schedule_or_block(used_names, CreepPrototype { 
        body: FLAGSHIP_TEMPLATE.clone(), 
        role: CreepRole::Flagship(FlagshipCreep::default()), 
        home: spawner.room
    });
}

fn get_tugboat_body(energy: u32, tugged: &Creep) -> Body {
    let tugged_body = Body::from(tugged);
    let target_tugboat_move_parts = tugged_body.0.len().saturating_sub(2 * tugged_body.num(Part::Move));
    let tugged_empty_carry = tugged.store().get_free_capacity(None).div_floor(50) as usize;
    let target_tugboat_move_parts = target_tugboat_move_parts - tugged_empty_carry;

    if target_tugboat_move_parts == 0 {
        warn!("Creep {} has requested tugboat, but doesn't actually benefit from it", tugged.name());
    }

    Body::from(Part::Move) * target_tugboat_move_parts.clamp(0, (energy / 50) as usize)
}

pub struct TugboatRequests(Vec<Creep>);
impl TugboatRequests {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn add_request_for(&mut self, tugged: Creep) {
        self.0.push(tugged);
    }
}

fn schedule_tugboats(mem: &mut Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames, tugboat_requests: TugboatRequests) {
    for tugged in tugboat_requests.0 {
        let already_exists = schedule.all_creeps()
            .any(|proto| matches!(&proto.role, CreepRole::Tugboat(tugged2, _) if *tugged2 == tugged.id()));
        if already_exists { continue; }

        let Some(home) = mem.creeps.get(&tugged.id()).map(|data| data.home) else { continue; };
        let Some(spawner) = schedule.spawners().filter_free().filter_room(home).next() else { continue; };

        spawner.schedule_or_block(used_names, CreepPrototype { 
            body: get_tugboat_body(spawner.energy_capacity, &tugged),
            role: CreepRole::Tugboat(tugged.id(), spawner.structure.id()), 
            home 
        });
    }
}

const TARGET_IDLE_FABRICATOR_WORK_COUNT: usize = 20;
const TARGET_SURPLUS_FABRICATOR_WORK_COUNT: usize = 40;
const BUFFER_ENERGY_SURPLUS_THRESHOLD: u32 = 50_000;
static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Carry, Carry, Move, Work, Carry]) });
fn schedule_fabricators(mem: &mut Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        let buffer_energy = colony.buffer.map_or(0, |buffer| buffer.used_energy_capacity());
        let work_target = if buffer_energy >= BUFFER_ENERGY_SURPLUS_THRESHOLD { TARGET_SURPLUS_FABRICATOR_WORK_COUNT } else { TARGET_IDLE_FABRICATOR_WORK_COUNT };

        loop {
            let curr_work_count = schedule.all_creeps().filter_home(colony.name).filter_role(|role| matches!(role, CreepRole::Fabricator(_))).part_count(Part::Work);
            if curr_work_count >= work_target { break; }

            let Some(spawner) = schedule.spawners().filter_room(colony.name).filter_free().next() else { break; };
            let body = FABRICATOR_TEMPLATE.scaled(spawner.energy_capacity, None);

            if spawner.schedule(used_names, CreepPrototype { 
                body, 
                role: CreepRole::Fabricator(FabricatorCreep::default()), 
                home: spawner.room
            }).is_none() { break; }
        }
    }
}

fn schedule_remote_fabricators(mem: &mut Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        if !matches!(colony.step, ColonyStep::BuildSpawn) { continue; }
        if schedule.all_creeps().filter_home(colony.name).filter_role(|role| matches!(role, CreepRole::Fabricator(_))).next().is_some() { continue; }

        let Some(spawn) = schedule.spawners().filter_free().next() else { continue; };
        spawn.schedule(used_names, CreepPrototype { 
            body: FABRICATOR_TEMPLATE.scaled(spawn.energy_capacity, None), 
            role: CreepRole::Fabricator(FabricatorCreep::default()), 
            home: colony.name
        });
    }
}

fn schedule_recovery(mem: &mut Memory, schedule: &mut SpawnSchedule, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        let buffered_energy = colony.buffer.map_or(0, |buffer| buffer.used_energy_capacity());
        let excavator_count = schedule.all_creeps().filter_home(colony.name)
            .filter(|proto| matches!(proto.role, CreepRole::Excavator(_, _)))
            .count();

        if buffered_energy == 0 && excavator_count == 0 {
            let Some(spawn) = schedule.spawners().filter_free().filter_room(colony.name).next() else { continue; };
            let Some((source, source_plan)) = colony.plan.sources.iter().next() else { continue; };
            let Some(source) = source.resolve() else { continue; };

            spawn.schedule_or_block(used_names, CreepPrototype { 
                body: get_excavator_body(spawn.energy_avaliable.max(300), source_plan), 
                role: CreepRole::Excavator(ExcavatorCreep::default(), source.id()), 
                home: colony.name
            });
        }

        let truck_count = schedule.all_creeps().filter_home(colony.name).filter_role(|role| matches!(role, CreepRole::Truck(_))).count();
        if truck_count == 0 {
            let Some(spawn) = schedule.spawners().filter_free().filter_room(colony.name).next() else { continue; };
            spawn.schedule_or_block(used_names, CreepPrototype { 
                body: get_truck_body(spawn.energy_avaliable.max(300)), 
                role: CreepRole::Truck(TruckCreep::default()), 
                home: colony.name
            });
        }
    }
}

pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut schedule = SpawnSchedule::new(mem);
    let mut used_names: UsedNames = game::creeps().keys().collect();

    schedule_tugboats(mem, &mut schedule, &mut used_names, tugboat_requests);
    schedule_recovery(mem, &mut schedule, &mut used_names);

    schedule_excavators(mem, &mut schedule, &mut used_names);
    schedule_trucks(mem, &mut schedule, &mut used_names);
    schedule_fabricators(mem, &mut schedule, &mut used_names);
    schedule_remote_fabricators(mem, &mut schedule, &mut used_names);
    schedule_flagships(mem, &mut schedule, &mut used_names);
    schedule_import_trucks(mem, &mut schedule, &mut used_names);

    schedule.execute(mem);
}