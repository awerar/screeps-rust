use std::{cell::RefCell, collections::HashMap, iter, ops::{Add, Mul}, rc::Rc};

use itertools::Itertools;
use screeps::{Creep, HasPosition, MAX_CREEP_SIZE, Part, RoomName, SPAWN_ENERGY_CAPACITY, Source, SpawnOptions, StructureExtension, StructureSpawn};

use crate::{colony::{Colonies, ColonyView, planning::planned_ref::ResolvableStructureRef}, creeps::{CreepData, CreepRole, excavator::ExcavatorCreep, }, domain_traits::{CreepId, EnergyStoreAccessors, HasId, ObjectId, ResolvableId}, memory::Memory, names::UsedNames};

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

    pub fn part_count(&self, part: Part) -> usize {
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

pub type SharedUsedNames = Rc<RefCell<UsedNames>>;

struct RelativeCreepPrototype {
    body: Body,
    role: CreepRole,
}

impl RelativeCreepPrototype {
    fn new(id: &CreepId, data: &CreepData) -> Self {
        Self {
            body: Body(id.resolve().body().into_iter().map(|part| part.part()).collect()),
            role: data.role.clone(),
        }
    }

    fn part_count(&self, part: Part) -> usize {
        self.body.part_count(part)
    }
}

struct AbsoluteCreepPrototype {
    proto: RelativeCreepPrototype,
    home: RoomName
}

impl AbsoluteCreepPrototype {
    pub fn body(&self) -> &Body {
        &self.proto.body
    }

    pub fn role(&self) -> &CreepRole {
        &self.proto.role
    }

    fn part_count(&self, part: Part) -> usize {
        self.body().part_count(part)
    }
}

impl AbsoluteCreepPrototype {
    fn new(id: &CreepId, data: &CreepData) -> Self {
        Self {
            proto: RelativeCreepPrototype::new(id, data),
            home: data.home
        }
    }
}

enum CreepPrototype {
    Absolute(AbsoluteCreepPrototype),
    Relative(RelativeCreepPrototype)
}

impl CreepPrototype {
    pub fn with_default_home(self, home: RoomName) -> AbsoluteCreepPrototype {
        match self {
            CreepPrototype::Absolute(proto) => proto,
            CreepPrototype::Relative(proto) => AbsoluteCreepPrototype { proto, home },
        }
    }
}

/*impl SpawnerData {
    fn from(mem: &Memory, spawn: &StructureSpawn, creeps: &[CreepPrototype]) -> Self {
        let room = spawn.room().unwrap();
        let spawning = spawn.spawning()
            .and_then(|spawning| {
                let creep = game::creeps().get(spawning.name().into())?;
                let prototype = CreepPrototype::try_from_existing(mem, &creep)?;

                Some((prototype, spawning.remaining_time()))
            });

        let mut future_energy: u32 = room.energy_available().max(300);

        if let Some(colony) = mem.colonies.view(room.name()) {
            let excavated_sources: HashSet<_> = creeps.iter()
                .filter_home(room.name())
                .filter_map(|proto| {
                    let CreepRole::Excavator(_, source) = &proto.role else { return None };
                    Some(*source)
                }).collect();

            let any_excavators = !excavated_sources.is_empty();
            let any_importers = creeps.iter().filter_home(room.name()).any(|proto| matches!(proto.role, CreepRole::ImportTruck(_)));
            let any_trucks = creeps.iter().filter_home(room.name()).any(|proto| matches!(proto.role, CreepRole::Truck(_)));

            for source in &excavated_sources {
                let source_plan = colony.plan.sources.get(&source.screeps_id()).unwrap();
                future_energy += source_plan.extensions.resolve().iter().map(EnergyStoreAccessors::free_energy_capacity).sum::<u32>();
            }

            let future_energy_income = if (any_trucks && any_excavators) || any_importers { 
                    u32::MAX 
                } else if any_trucks {
                    colony.buffer.map_or(0, |buffer| buffer.used_energy_capacity()) 
                } else { 
                    0 
                };

            let mut center_free_capacity = colony.plan.center.extensions.resolve().iter().map(EnergyStoreAccessors::free_energy_capacity).sum::<u32>();
            center_free_capacity += spawn.free_energy_capacity();

            future_energy += center_free_capacity.min(future_energy_income);
        }

        Self {
            structure: spawn.clone(),
            room: room.name(),
            future_energy,
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
}*/

enum ExcavatorSyndrome {
    NoExcavator,
    NoTugboatFor(CreepId)
}

struct ColonySyndrome {
    any_trucks: bool,
    any_excavating_excavators: bool,
    excavators: HashMap<ObjectId<Source>, ExcavatorSyndrome>
}

impl ColonySyndrome {
    fn new(creeps: &ColonyCreeps, view: &ColonyView<'_>) -> Self {
        Self { 
            any_trucks: creeps.0.values().any(|proto| matches!(proto.role, CreepRole::Truck(_))), 
            any_excavating_excavators: creeps.0.values().any(|proto| matches!(&proto.role, CreepRole::Excavator(ExcavatorCreep::Mining, _))), 
            excavators: 
                view.plan.sources.keys()
                    .filter_map(|id| Some(id.resolve()?.id()))
                    .filter_map(|source| {
                        let Some((excavator, _)) = creeps.0.iter().find(|(_, proto)| matches!(&proto.role, CreepRole::Excavator(_, source2) if source == *source2)) else {
                            return Some((source, ExcavatorSyndrome::NoExcavator));
                        };

                        if creeps.0.values().all(|proto| !matches!(&proto.role, CreepRole::Tugboat(tugged, _) if *excavator == *tugged)) {
                            return Some((source, ExcavatorSyndrome::NoTugboatFor(excavator.clone())));
                        }

                        None
                    }).collect()
        }
    }
}

enum EnergyPoolType {
    Finite,
    RefilledTo(u32)
}

impl EnergyPoolType {
    fn refilled_if(cond: bool, capacity: u32) -> Self {
        if cond { Self::RefilledTo(capacity) }
        else { EnergyPoolType::Finite }
    }
}

struct EnergyPool {
    pub current: u32,
    ty: EnergyPoolType
}

impl EnergyPool {
    pub fn new(current: u32, ty: EnergyPoolType) -> Self {
        Self { current, ty }
    }

    pub fn future_energy(&self) -> u32 {
        match &self.ty {
            EnergyPoolType::Finite => self.current,
            EnergyPoolType::RefilledTo(capacity) => *capacity,
        }
    }

    pub fn is_refilled(&self) -> bool {
        matches!(&self.ty, EnergyPoolType::RefilledTo(_))
    }
}

struct Spawning {
    name: String,
    proto: AbsoluteCreepPrototype,
    extensions: Vec<StructureExtension>
}

enum SpawnState {
    Free,
    Blocked,
    Spawning(Spawning)
}

enum ColonySpawnType {
    Central,
    Source(ObjectId<Source>)
}

struct ColonySpawn {
    spawn: StructureSpawn,
    state: SpawnState,
    ty: ColonySpawnType,

    energy: EnergyPool,
}

impl ColonySpawn {
    pub fn new(spawn: StructureSpawn, ty: ColonySpawnType, gets_energy: bool) -> Self {
        Self { 
            energy: EnergyPool::new(
                spawn.used_energy_capacity(),
                EnergyPoolType::refilled_if(gets_energy, SPAWN_ENERGY_CAPACITY)
            ), 
            state: if spawn.spawning().is_some() { SpawnState::Blocked } else { SpawnState::Free }, 
            spawn,
            ty
        }
    }

    pub fn is_free(&self) -> bool {
        matches!(self.state, SpawnState::Free)
    }

    fn execute(self) -> Result<(), anyhow::Error> {
        let SpawnState::Spawning(spawning) = self.state else { return Ok(()) };
        self.spawn.spawn_creep_with_options(
            &spawning.proto.body().0, 
            &spawning.name, 
            &SpawnOptions::new().energy_structures(spawning.extensions)
        ).map_err(anyhow::Error::new)
    }
}

struct ExtensionGroup {
    extensions_left: Vec<StructureExtension>,
    energy: EnergyPool,
    central: bool
}

impl ExtensionGroup {
    pub fn new(extensions: Vec<StructureExtension>, refilled: bool, central: bool) -> Self {
        let extensions = extensions.into_iter()
            .filter(|extension| extension.used_energy_capacity() > 0)
            .collect_vec();

        Self { 
            energy: EnergyPool::new(
                extensions.iter().map(EnergyStoreAccessors::used_energy_capacity).sum::<u32>(),
                EnergyPoolType::refilled_if(
                    refilled, 
                    extensions.iter().map(EnergyStoreAccessors::energy_capacity).sum::<u32>()
                )
            ),
            extensions_left: extensions,
            central
        }
    }

    pub fn allocate(&mut self, mut amount: u32) -> Vec<StructureExtension> {
        assert!(self.energy.current >= amount);

        let mut extensions = Vec::new();
        while amount > 0 {
            let extension = self.extensions_left.pop().unwrap();
            amount = amount.saturating_sub(extension.used_energy_capacity());
            self.energy.current -= extension.used_energy_capacity();

            extensions.push(extension);
        }

        extensions
    }

    pub fn reserve_future(&mut self, amount: u32) {
        assert!(self.energy.future_energy() >= amount);

        if !self.energy.is_refilled() {
            self.allocate(amount);
        }
    }
}

struct ColonyExtensions(Vec<ExtensionGroup>);

impl ColonyExtensions {
    fn energy(&self) -> u32 {
        self.0.iter().map(|group| group.energy.current).sum()
    }

    fn future_energy(&self) -> u32 {
        self.0.iter().map(|group| group.energy.future_energy()).sum()
    }

    fn allocate(&mut self, mut amount: u32) -> Vec<StructureExtension> {
        assert!(self.energy() <= amount);

        let mut extensions = Vec::new();
        for group in self.0.iter_mut().sorted_by_key(|group| !group.energy.is_refilled()) {
            if amount == 0 { break; }
            
            let group_amount = amount.min(group.energy.current);
            amount -= group_amount;

            extensions.extend(group.allocate(amount));
        }

        extensions
    }

    fn reserve_future(&mut self, mut amount: u32) {
        assert!(self.future_energy() <= amount);

        for group in self.0.iter_mut().sorted_by_key(|group| !group.energy.is_refilled()) {
            if amount == 0 { return }

            let group_amount = amount.min(group.energy.future_energy());
            amount -= group_amount;

            group.reserve_future(amount);
        }
    }
}

struct ColonySpawns {
    name: RoomName,

    spawns: Vec<ColonySpawn>,
    extensions: ColonyExtensions,

    names: SharedUsedNames
}

pub struct ColonySpawnIterator<'a> {
    index: usize,
    spawns: &'a [ColonySpawn]
}

impl<'a> Iterator for ColonySpawnIterator<'a> {
    type Item = (usize, &'a ColonySpawn);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(spawn) = self.spawns.get(self.index) {
            let index = self.index;
            self.index += 1;

            if !spawn.is_free() { continue; }

            return Some((index, spawn))
        }

        None
    }
}

enum ScheduleResult {
    NoValidSpawn,
    Scheduled(CreepId),
    WaitingForEnergy,
    NotEnoughEnergy
}

impl ColonySpawns {
    pub fn new(colony: &ColonyView<'_>, syndrome: &ColonySyndrome, names: SharedUsedNames) -> Self {
        let mut extensions = Vec::new();

        extensions.push(ExtensionGroup::new(
            colony.plan.center.extensions.resolve().into_iter()
                .sorted_by_cached_key(|extension| extension.pos().get_range_to(colony.center))
                .collect(), 
            syndrome.any_excavating_excavators && syndrome.any_trucks,
            true
        ));

        for (source, source_plan) in &colony.plan.sources {
            let Some(source) = source.resolve().map(|src| src.id()) else { continue; };

            extensions.push(ExtensionGroup::new(
                source_plan.extensions.resolve(),
                !syndrome.excavators.contains_key(&source),
                false
            ));
        }

        Self { 
            spawns: 
                colony.plan.center.spawn.resolve().map(|spawn| {
                    ColonySpawn::new(
                        spawn,
                        ColonySpawnType::Central,
                        syndrome.any_trucks && syndrome.any_excavating_excavators)
                }).into_iter().collect(),
            extensions: ColonyExtensions(extensions),
            names,
            name: colony.name
        }
    }

    fn free_spawns(&self) -> impl Iterator<Item = &ColonySpawn> {
        self.spawns.iter().filter(|spawn| spawn.is_free())
    }

    pub fn max_spawnable_energy(&self) -> u32 {
        self.free_spawns()
            .map(|spawn| spawn.energy.current)
            .max()
            .map_or(
                0, 
                |spawn_energy| spawn_energy + self.extensions.energy()
            )
    }

    pub fn max_future_spawnable_energy(&self) -> u32 {
        self.free_spawns()
            .map(|spawn| spawn.energy.future_energy())
            .max()
            .map_or(
                0, 
                |spawn_energy| spawn_energy + self.extensions.future_energy()
            )
    }

    pub fn has_free(&self) -> bool {
        self.free_spawns().next().is_some()
    }

    pub fn schedule_selected<S, P>(&mut self, select: S, make_proto: P) -> ScheduleResult 
    where
        S: for<'a> FnOnce(ColonySpawnIterator<'a>) -> Option<usize>,
        P: FnOnce(&ColonySpawn) -> CreepPrototype
    {
        let Some(choice) = select(ColonySpawnIterator { index: 0, spawns: &self.spawns }) else { return ScheduleResult::NoValidSpawn };
        let spawn = self.spawns.get_mut(choice).expect("Spawn selection should return a valid index");
        let proto = make_proto(spawn).with_default_home(self.name);

        assert!(spawn.is_free());

        let cost = proto.body().energy_required();
        let spawn_cost = cost.min(spawn.energy.current);
        let extension_cost = cost - spawn_cost;

        if cost > spawn.energy.future_energy() + self.extensions.future_energy() { return ScheduleResult::NotEnoughEnergy }
        if cost > spawn.energy.current + self.extensions.energy() { 
            spawn.state = SpawnState::Blocked;
            self.extensions.reserve_future(extension_cost);
            return ScheduleResult::WaitingForEnergy 
        }

        spawn.energy.current -= spawn_cost;
        let extensions = self.extensions.allocate(extension_cost);

        let name = self.names.borrow_mut().generate_new(proto.role());
        spawn.state = SpawnState::Spawning(Spawning { name: name.clone(), proto, extensions });

        ScheduleResult::Scheduled(CreepId::Name(name))
    }

    pub fn schedule<P>(&mut self, make_proto: P) -> ScheduleResult
    where
        P: FnOnce(&ColonySpawn) -> CreepPrototype
    {
        self.schedule_selected(
            |iter|{
                iter.max_by_key(|(_, spawn)| {
                    (spawn.energy.future_energy(), spawn.energy.current, matches!(spawn.ty, ColonySpawnType::Central))
                }).map(|(ix, _)| ix)
            }, 
            make_proto
        )
    }

    fn execute(self) -> Result<(), anyhow::Error> {
        for spawn in self.spawns {
            spawn.execute()?;
        }

        Ok(())
    }
}

struct Spawns(HashMap<RoomName, ColonySpawns>);

impl Spawns {
    fn new(colonies: &Colonies, syndrome: &ColonySyndrome, names: SharedUsedNames) -> Self {
        Self(
            colonies.view_all()
                .map(|colony| (colony.name, ColonySpawns::new(&colony, syndrome, names.clone())))
                .collect()
        )
    }

    pub fn execute(self) -> Result<(), anyhow::Error> {
        for colony_spawns in self.0.into_values() {
            colony_spawns.execute()?;
        }

        Ok(())
    }
}

struct ColonyCreeps(HashMap<CreepId, RelativeCreepPrototype>);

impl ColonyCreeps {
    pub fn new(colony: RoomName, mem: &Memory) -> Self {
        Self(
            mem.creeps.iter()
                .filter(|(_, data)| data.home == colony)
                .map(|(id, data)| (id.clone(), RelativeCreepPrototype::new(id, data)))
                .collect()
        )
    }
}

struct Creeps(HashMap<RoomName, ColonyCreeps>);

impl Creeps {
    pub fn new(mem: &Memory) -> Self {
        Self(
            mem.colonies.rooms()
                .map(|colony| (colony, ColonyCreeps::new(colony, mem)))
                .collect()
        )
    }
}

/*impl SpawnSchedule {
    fn new(mem: &Memory) -> Self {
        let creeps = game::creeps().values()
            .filter_map(|creep| CreepPrototype::try_from_existing(mem, &creep))
            .collect_vec();

        Self {
            spawners: game::spawns().values()
                .map(|spawn| SpawnerData::from(mem, &spawn, &creeps))
                .collect(),
            already_spawned: creeps
        }
    }

    fn all_creeps(&self) -> impl Iterator<Item = &'_ CreepPrototype> {
        self.already_spawned.iter().chain(
            self.spawners.iter()
            .filter_map(|spawner| {
                match &spawner.status {
                    Free | Blocked | Spawning(_, _) => None,
                    Scheduled { proto, .. } => Some(proto)
                }
            })
        )
    }

    fn schedule_or_block(&mut self, body: Body, role: CreepRole, room: Option<RoomName>, used_names: &mut UsedNames) -> Option<CreepId> {
        self.spawners.iter().filter_map(f)
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
}*/

/*
trait CreepPrototypeIteratorExt<'a>: Iterator<Item = &'a AbsoluteCreepPrototype> + Sized {
    fn filter_home(self, home: RoomName) -> impl Iterator<Item = &'a AbsoluteCreepPrototype> {
        self.filter(move |proto| proto.home == home)
    }

    fn filter_role(self, f: impl Fn(&CreepRole) -> bool) -> impl Iterator<Item = &'a AbsoluteCreepPrototype> {
        self.filter(move |proto| f(&proto.role))
    }

    fn part_count(self, part: Part) -> usize {
        self.map(|proto| proto.body.num(part)).sum()
    }
}

impl<'a, T: Iterator<Item = &'a AbsoluteCreepPrototype> + Sized> CreepPrototypeIteratorExt<'a> for T { }

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() { 7 } else { 5 };
    let excavator_works = energy.saturating_sub(50).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::from(Part::Carry) + Body::from(Part::Work) * (excavator_works as usize)
}

fn schedule_excavators(schedule: &mut ColonySpawnSchedule<'_, '_>, used_names: &mut UsedNames) {
    for (source, source_plan) in &schedule.view().plan.sources {
        let Some(source) = source.resolve() else { continue; };

        let any_excavator_already = schedule.all_creeps()
            .any(|proto| matches!(&proto.role, CreepRole::Excavator(_, excavator_source) if *excavator_source == source.id()));
        if any_excavator_already { continue; }

        let Some(spawner) = schedule.spawners().filter_free().next() else { continue; };

        let prototype = AbsoluteCreepPrototype { 
            body: get_excavator_body(spawner.future_energy, source_plan), 
            role: CreepRole::Excavator(ExcavatorCreep::default(), source.id()),
            home: schedule.view().name
        };

        spawner.schedule_or_block(used_names, prototype);
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

fn schedule_trucks(schedule: &mut ColonySpawnSchedule<'_, '_>, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        let total_carry_for_sources = colony.plan.sources.values()
            .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
            .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
            .sum::<f32>();

        let target_carry = ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize;

        loop {
            let current_carry = schedule.all_creeps().filter_role(|role| matches!(role, CreepRole::Truck(_))).part_count(Part::Carry);
            if current_carry >= target_carry { break; }

            let Some(spawner) = schedule.spawners().filter_free().next() else { break };
            if spawner.schedule_or_block(used_names, AbsoluteCreepPrototype { 
                role: CreepRole::Truck(TruckCreep::default()), 
                home: colony.name, 
                body: get_truck_body(spawner.future_energy)
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
        spawn.schedule(used_names, AbsoluteCreepPrototype { 
            body: IMPORT_TRUCK_TEMPLATE.scaled(spawn.future_energy, None), 
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

    spawner.schedule_or_block(used_names, AbsoluteCreepPrototype { 
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

fn schedule_tugboats(schedule: &mut ColonySpawnSchedule<'_, '_>, used_names: &mut UsedNames, tugboat_requests: &TugboatRequests) {
    for tugged in &tugboat_requests.0 {
        let already_exists = schedule.all_creeps()
            .any(|proto| matches!(&proto.role, CreepRole::Tugboat(tugged2, _) if *tugged2 == tugged.id()));
        if already_exists { continue; }

        let Some(spawner) = schedule.spawners().filter_free().next() else { continue; };

        spawner.schedule_or_block(used_names, AbsoluteCreepPrototype { 
            body: get_tugboat_body(spawner.future_energy, tugged),
            role: CreepRole::Tugboat(tugged.id(), spawner.structure.id()), 
            home: schedule.view().name
        });
    }
}

const TARGET_IDLE_FABRICATOR_WORK_COUNT: usize = 20;
const TARGET_SURPLUS_FABRICATOR_WORK_COUNT: usize = 40;
const BUFFER_ENERGY_SURPLUS_THRESHOLD: u32 = 50_000;
static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Carry, Carry, Move, Work, Carry]) });
fn schedule_fabricators(schedule: &mut ColonySpawnSchedule<'_, '_>, used_names: &mut UsedNames) {
    for colony in mem.colonies.view_all() {
        let buffer_energy = colony.buffer.map_or(0, |buffer| buffer.used_energy_capacity());
        let work_target = if buffer_energy >= BUFFER_ENERGY_SURPLUS_THRESHOLD { TARGET_SURPLUS_FABRICATOR_WORK_COUNT } else { TARGET_IDLE_FABRICATOR_WORK_COUNT };

        loop {
            let curr_work_count = schedule.all_creeps().filter_role(|role| matches!(role, CreepRole::Fabricator(_))).part_count(Part::Work);
            if curr_work_count >= work_target { break; }

            let Some(spawner) = schedule.spawners().filter_free().next() else { break; };
            let body = FABRICATOR_TEMPLATE.scaled(spawner.future_energy, None);

            if spawner.schedule(used_names, AbsoluteCreepPrototype { 
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
        spawn.schedule(used_names, AbsoluteCreepPrototype { 
            body: FABRICATOR_TEMPLATE.scaled(spawn.future_energy, None), 
            role: CreepRole::Fabricator(FabricatorCreep::default()), 
            home: colony.name
        });
    }
}

#[expect(clippy::needless_pass_by_value)]
pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut schedule = SpawnSchedule::new(mem);
    let mut used_names: UsedNames = game::creeps().keys().collect();

    for view in mem.colonies.view_all() {
        let mut schedule = ColonySpawnSchedule { view, schedule: &mut schedule };

        schedule_tugboats(&mut schedule, &mut used_names, &tugboat_requests);
        schedule_excavators(&mut schedule, &mut used_names);
        schedule_trucks(&mut schedule, &mut used_names);
        schedule_fabricators(&mut schedule, &mut used_names);
    }

    schedule_remote_fabricators(mem, &mut schedule, &mut used_names);
    schedule_flagships(mem, &mut schedule, &mut used_names);
    schedule_import_trucks(mem, &mut schedule, &mut used_names);

    schedule.execute(mem);
}*/