use std::{cell::RefCell, collections::{HashMap, hash_map}, iter, ops::{Add, Mul}, rc::Rc, sync::LazyLock};

use derive_deref::Deref;
use itertools::Itertools;
use log::warn;
use screeps::{Creep, HasPosition, MAX_CREEP_SIZE, Part, RoomName, SPAWN_ENERGY_CAPACITY, Source, SpawnOptions, Structure, StructureExtension, StructureSpawn, action_error_codes::SpawnCreepErrorCode, game};
use thiserror::Error;

use crate::{colony::{ColonyView, planning::{plan::SourcePlan, planned_ref::ResolvableStructureRef}, steps::ColonyStep}, creeps::{CreepData, CreepRole, excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::{ImportTruckState, TruckCreep}, }, domain_traits::{CreepId, EnergyStoreAccessors, HasId, HasName, ObjectId, ResolvableId}, logging::LogResultErr, memory::Memory, names::UsedNames};

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

#[derive(Clone)]
struct RelativePrototype {
    body: Body,
    role: CreepRole,
}

impl RelativePrototype {
    fn new(body: Body, role: CreepRole) -> Self {
        Self { body, role }
    }

    fn from_creep(id: &CreepId, data: &CreepData) -> Self {
        Self {
            body: Body(id.resolve().body().into_iter().map(|part| part.part()).collect()),
            role: data.role.clone(),
        }
    }

    fn with_home(self, home: RoomName) -> AbsolutePrototype {
        AbsolutePrototype { proto: self, home }
    }
}

#[derive(Clone)]
struct AbsolutePrototype {
    proto: RelativePrototype,
    home: RoomName
}

impl AbsolutePrototype {
    fn new(body: Body, role: CreepRole, home: RoomName) -> Self {
        Self { proto: RelativePrototype { body, role }, home }
    }

    fn from_creep(id: &CreepId, data: &CreepData) -> Self {
        Self {
            proto: RelativePrototype::from_creep(id, data),
            home: data.home
        }
    }
    
    pub fn body(&self) -> &Body {
        &self.proto.body
    }

    pub fn role(&self) -> &CreepRole {
        &self.proto.role
    }
}

enum Prototype {
    Absolute(AbsolutePrototype),
    Relative(RelativePrototype)
}

impl Prototype {
    pub fn relative(body: Body, role: CreepRole) -> Self {
        Prototype::Relative(RelativePrototype::new(body, role))
    }

    pub fn absolute(body: Body, role: CreepRole, home: RoomName) -> Self {
        Prototype::Absolute(AbsolutePrototype::new(body, role, home))
    }

    pub fn with_default_home(self, home: RoomName) -> AbsolutePrototype {
        match self {
            Prototype::Absolute(proto) => proto,
            Prototype::Relative(proto) => AbsolutePrototype { proto, home },
        }
    }
}

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
                        let Some((excavator, proto)) = creeps.0.iter().find(|(_, proto)| matches!(&proto.role, CreepRole::Excavator(_, source2) if source == *source2)) else {
                            return Some((source, ExcavatorSyndrome::NoExcavator));
                        };

                        let CreepRole::Excavator(state, _) = &proto.role else { unreachable!() };

                        if matches!(state, ExcavatorCreep::Going)
                            && creeps.0.values().all(|proto| !matches!(&proto.role, CreepRole::Tugboat(tugged, _) if *excavator == *tugged)) {
                                return Some((source, ExcavatorSyndrome::NoTugboatFor(excavator.clone())));
                        }

                        None
                    }).collect()
        }
    }

    fn any_problems(&self) -> bool {
        !self.any_trucks || !self.any_excavating_excavators || !self.excavators.is_empty()
    }

    fn tugged_order(&self) -> Option<Vec<Creep>> {
        if self.any_problems() {
            Some(
                if self.any_trucks {
                    self.excavators.values().filter_map(|excavator| {
                        match excavator {
                            ExcavatorSyndrome::NoExcavator => None,
                            ExcavatorSyndrome::NoTugboatFor(creep ) => Some(creep.resolve())
                        }
                    }).collect()
                } else { 
                    Vec::new()
                }
            )
        } else { 
            None 
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
            EnergyPoolType::RefilledTo(capacity) => (*capacity).max(self.current),
        }
    }

    pub fn is_refilled(&self) -> bool {
        matches!(&self.ty, EnergyPoolType::RefilledTo(_))
    }
}

enum SpawnState {
    Free,
    Blocked,
    Spawning(CreepId, CreepData)
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

    fn gather_new_creeps(self, mem: &mut Memory) {
        if let SpawnState::Spawning(id, creep_data) = self.state  {
            mem.creeps.insert(id, creep_data);
        }
    }
}

struct ExtensionGroup {
    extensions_left: Vec<StructureExtension>,
    energy: EnergyPool,
    central: bool
}

impl ExtensionGroup {
    pub fn new(extensions: Vec<StructureExtension>, refilled: bool, central: bool) -> Self {
        Self { 
            energy: EnergyPool::new(
                extensions.iter().map(EnergyStoreAccessors::used_energy_capacity).sum::<u32>(),
                EnergyPoolType::refilled_if(
                    refilled, 
                    extensions.iter().map(EnergyStoreAccessors::energy_capacity).sum::<u32>()
                )
            ),
            extensions_left: extensions.into_iter()
                .filter(|extension| extension.used_energy_capacity() > 0)
                .rev()
                .collect_vec(),
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
        assert!(self.energy() >= amount);

        let mut extensions = Vec::new();
        for group in self.0.iter_mut().sorted_by_key(|group| !group.energy.is_refilled()) {
            if amount == 0 { break; }
            
            let group_amount = amount.min(group.energy.current);
            amount -= group_amount;

            extensions.extend(group.allocate(group_amount));
        }

        extensions
    }

    fn reserve_future(&mut self, mut amount: u32) {
        assert!(self.future_energy() >= amount);

        for group in self.0.iter_mut().sorted_by_key(|group| !group.energy.is_refilled()) {
            if amount == 0 { return }

            let group_amount = amount.min(group.energy.future_energy());
            amount -= group_amount;

            group.reserve_future(group_amount);
        }
    }
}

enum RoleSelector {
    Excavator,
    SourceExcavator(ObjectId<Source>),
    Truck,
    ImportTruck,
    Flagship,
    Tugboat,
    TugboatFor(CreepId),
    Fabricator
}

impl RoleSelector {
    fn matches(&self, role: &CreepRole) -> bool {
        match self {
            RoleSelector::Excavator => matches!(role, CreepRole::Excavator(_, _)),
            RoleSelector::SourceExcavator(source) => matches!(role, CreepRole::Excavator(_, source2) if *source2 == *source),
            RoleSelector::Truck => matches!(role, CreepRole::Truck(_)),
            RoleSelector::ImportTruck => matches!(role, CreepRole::ImportTruck(_)),
            RoleSelector::Flagship => matches!(role, CreepRole::Flagship(_)),
            RoleSelector::Tugboat => matches!(role, CreepRole::Tugboat(_, _)),
            RoleSelector::TugboatFor(tugged) => matches!(role, CreepRole::Tugboat(tugged2, _) if *tugged2 == *tugged),
            RoleSelector::Fabricator => matches!(role, CreepRole::Fabricator(_)),
        }
    }
}

#[derive(Deref)]
struct ColonyCreeps(HashMap<CreepId, RelativePrototype>);

impl ColonyCreeps {
    pub fn new(colony: RoomName, mem: &Memory) -> Self {
        Self(
            mem.creeps.iter()
                .filter(|(_, data)| data.home == colony)
                .map(|(id, data)| (id.clone(), RelativePrototype::from_creep(id, data)))
                .collect()
        )
    }

    fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = &RelativePrototype> {
        self.0.values().filter(move |proto| role.matches(&proto.role))
    }

    fn part_count(&self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|proto| proto.body.part_count(part)).sum()
    }
}

struct ColonyRoster {
    name: RoomName,

    spawns: Vec<ColonySpawn>,
    extensions: ColonyExtensions,
    local_creeps: ColonyCreeps,

    syndrome: ColonySyndrome,

    names: SharedUsedNames
}

struct ColonySpawnIterator<'a> {
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

struct SpawnInfo {
    spawn: StructureSpawn,
    energy: u32,
    future_energy: u32
}

enum ScheduleDecision {
    Scheduled(CreepId, AbsolutePrototype),
    WaitingForEnergy
}

#[derive(Debug, Error)]
enum ColonyScheduleError {
    #[error(transparent)] SpawnError(#[from] SpawnCreepErrorCode),
    #[error("No valid spawn")] NoSpawn,
    #[error("No prototype")] NoPrototype,
    #[error("Not enough energy")] NotEnoughEnergy
}

type ColonyScheduleResult = Result<ScheduleDecision, ColonyScheduleError>;

impl ColonyRoster {
    pub fn new(colony: &ColonyView<'_>, mem: &Memory, names: SharedUsedNames) -> Self {
        let local_creeps = ColonyCreeps::new(colony.name, mem);
        let syndrome = ColonySyndrome::new(&local_creeps, colony);

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
            name: colony.name,
            local_creeps,
            syndrome
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

    // Not meant to be used by user
    fn schedule_selected_absolute<S, P>(&mut self, select: S, make_proto: P) -> ColonyScheduleResult
    where
        S: for<'b> FnOnce(ColonySpawnIterator<'b>) -> Option<usize>,
        P: FnOnce(SpawnInfo) -> Option<AbsolutePrototype>
    {
        let Some(choice) = select(ColonySpawnIterator { index: 0, spawns: &self.spawns }) else { return Err(ColonyScheduleError::NoSpawn) };
        let spawn = self.spawns.get_mut(choice).expect("Spawn selection should return a valid index");
        let spawn_info = SpawnInfo {
            spawn: spawn.spawn.clone(),
            energy: self.extensions.energy() + spawn.energy.current,
            future_energy: self.extensions.future_energy() + spawn.energy.future_energy()
        };

        let proto = make_proto(spawn_info).ok_or(ColonyScheduleError::NoPrototype)?;

        assert!(spawn.is_free());

        let cost = proto.body().energy_required();
        let spawn_cost = cost.min(spawn.energy.current);
        let extension_cost = cost - spawn_cost;

        if cost > spawn.energy.future_energy() + self.extensions.future_energy() { return Err(ColonyScheduleError::NotEnoughEnergy) }
        if cost > spawn.energy.current + self.extensions.energy() { 
            spawn.state = SpawnState::Blocked;
            self.extensions.reserve_future(extension_cost);
            return Ok(ScheduleDecision::WaitingForEnergy)
        }

        spawn.energy.current -= spawn_cost;
        let extensions = self.extensions.allocate(extension_cost);
        let energy_structures = vec![Structure::from(spawn.spawn.clone())].into_iter()
            .chain(extensions.into_iter().map(Structure::from));

        let name = self.names.borrow_mut().generate_new(proto.role());
        spawn.spawn.spawn_creep_with_options(
            &proto.body().0, 
            &name, 
            &SpawnOptions::new().energy_structures(energy_structures)
        )?;

        let id = game::creeps().get(name).unwrap().id();
        spawn.state = SpawnState::Spawning(id.clone(), CreepData { role: proto.role().clone(), home: proto.home });
        
        Ok(ScheduleDecision::Scheduled(id, proto))
    }

    pub fn schedule_selected<S, P>(&mut self, select: S, make_proto: P) -> ColonyScheduleResult
    where
        S: for<'a> FnOnce(ColonySpawnIterator<'a>) -> Option<usize>,
        P: FnOnce(SpawnInfo) -> Option<RelativePrototype>
    {
        let home = self.name;
        let result = self.schedule_selected_absolute(select, |spawn| make_proto(spawn).map(|creep| creep.with_home(home)));

        if let Ok(ScheduleDecision::Scheduled(id, proto)) = &result {
            self.local_creeps.0.insert(id.clone(), proto.proto.clone());
        }

        result
    }

    pub fn default_select(iter: ColonySpawnIterator<'_>) -> Option<usize> {
        iter.max_by_key(|(_, spawn)| {
            (spawn.energy.future_energy(), spawn.energy.current, matches!(spawn.ty, ColonySpawnType::Central))
        }).map(|(ix, _)| ix)
    }

    pub fn schedule<P>(&mut self, make_proto: P) -> ColonyScheduleResult
    where
        P: FnOnce(SpawnInfo) -> Option<RelativePrototype>
    {
        self.schedule_selected(Self::default_select, make_proto)
    }

    fn gather_new_creeps(self, mem: &mut Memory) {
        for spawn in self.spawns {
            spawn.gather_new_creeps(mem);
        }
    }
}

#[derive(Debug, Error)]
enum GlobalScheduleError {
    #[error("No valid room")] NoRoom,
    #[error("{0} is not a valid home colony")] InvalidHome(RoomName),
    #[error(transparent)] Roster(#[from] ColonyScheduleError),
}

type GlobalScheduleResult = Result<ScheduleDecision, GlobalScheduleError>;

struct GlobalCreeps(HashMap<CreepId, AbsolutePrototype>);

impl GlobalCreeps {
    fn new(mem: &Memory) -> Self {
        Self(mem.creeps.iter()
            .map(|(id, data)| (id.clone(), AbsolutePrototype::from_creep(id, data)))
            .collect())
    }

    fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = &AbsolutePrototype> {
        self.0.values().filter(move |proto| role.matches(proto.role()))
    }

    fn part_count(&self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|proto| proto.body().part_count(part)).sum()
    }
}

struct Rosters {
    rosters: HashMap<RoomName, ColonyRoster>,
    global_creeps: GlobalCreeps
}

impl Rosters {
    fn new(mem: &Memory) -> Self {
        let names = Rc::new(RefCell::new(UsedNames::new()));

        Self {
            rosters: mem.colonies.view_all()
                .map(|colony| (colony.name, ColonyRoster::new(&colony, mem, names.clone())))
                .collect(),
            global_creeps: GlobalCreeps::new(mem)
        }
    }

    pub fn schedule_selected<S, P>(&mut self, select: S, make_proto: P) -> GlobalScheduleResult
    where
        S: for<'a> FnOnce(hash_map::Iter<'a, RoomName, ColonyRoster>) -> Option<RoomName>,
        P: FnOnce(SpawnInfo) -> Option<Prototype>
    {
        let Some(choice) = select(self.rosters.iter()) else { return Err(GlobalScheduleError::NoRoom) };
        let roster = self.rosters.get_mut(&choice).expect("Room selection should return a valid room");

        let result = roster.schedule_selected_absolute(
            ColonyRoster::default_select, 
            |info| make_proto(info).map(|proto| proto.with_default_home(choice))
        );

        if let Ok(ScheduleDecision::Scheduled(id, proto)) = &result {
            self.rosters.get_mut(&proto.home)
                .ok_or(GlobalScheduleError::InvalidHome(proto.home))?
                .local_creeps.0.insert(id.clone(), proto.proto.clone());
        }

        result.map_err(GlobalScheduleError::from)
    }

    pub fn default_select(iter: hash_map::Iter<'_, RoomName, ColonyRoster>) -> Option<RoomName> {
        iter.max_by_key(|(_, roster)| roster.max_future_spawnable_energy())
            .map(|(room, _)| *room)
    }

    pub fn schedule<P>(&mut self, make_proto: P) -> GlobalScheduleResult
    where
        P: FnOnce(SpawnInfo) -> Option<Prototype>
    {
        self.schedule_selected(Self::default_select, make_proto)
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        for colony_spawns in self.rosters.into_values() {
            colony_spawns.gather_new_creeps(mem);
        }
    }
}

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() { 7 } else { 5 };
    let excavator_works = energy.saturating_sub(Part::Carry.cost()).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::from(Part::Carry) + Body::from(Part::Work) * (excavator_works as usize)
}

fn schedule_excavators(roster: &mut ColonyRoster, view: &ColonyView<'_>) {
    for (source, source_plan) in &view.plan.sources {
        let Some(source) = source.resolve() else { continue; };
        if !roster.has_free() { continue; }

        if roster.local_creeps.of_role(RoleSelector::SourceExcavator(source.id())).next().is_some() { continue; }

        roster.schedule(|info| {
            Some(RelativePrototype { 
                body: get_excavator_body(info.future_energy, source_plan), 
                role: CreepRole::Excavator(ExcavatorCreep::default(), source.id())
            })
        }).log_err();
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

fn schedule_trucks(roster: &mut ColonyRoster, colony: &ColonyView<'_>) {
    let total_carry_for_sources = colony.plan.sources.values()
        .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
        .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
        .sum::<f32>();

    let target_carry = if roster.syndrome.any_problems() { 
        1
    } else {
        ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize
    };

    while roster.has_free() {
        if roster.local_creeps.part_count(RoleSelector::Truck, Part::Carry) >= target_carry { break; }

        roster.schedule(|info| {
            Some(RelativePrototype {
                body: get_truck_body(info.future_energy),
                role: CreepRole::Truck(TruckCreep::default())
            })
        }).log_err();
    }
}

static IMPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Move, Carry]) });
fn schedule_import_trucks(rosters: &mut Rosters, mem: &mut Memory) {
    for colony in mem.colonies.view_all() {
        if !matches!(colony.step, ColonyStep::BuildSpawn) { continue; }

        let roster = rosters.rosters.get(&colony.name).unwrap();
        if roster.local_creeps.part_count(RoleSelector::ImportTruck, Part::Carry) > 100 { 
            continue; 
        }

        rosters.schedule(|info| {
            Some(Prototype::absolute(
                IMPORT_TRUCK_TEMPLATE.scaled(info.future_energy, None), 
                CreepRole::ImportTruck(ImportTruckState::default()), 
                colony.name
            ))
        }).log_err();
    }
}

static FLAGSHIP_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Claim, Move]) });
fn schedule_flagships(rosters: &mut Rosters, mem: &mut Memory) {
    let coordinator = &mut mem.flagship_coordinator;
    if coordinator.rooms.is_empty() { return; }

    if rosters.global_creeps.of_role(RoleSelector::Flagship).count() > 0 { return; }

    rosters.schedule(|_| {
        Some(Prototype::relative(  
            FLAGSHIP_TEMPLATE.clone(), 
            CreepRole::Flagship(FlagshipCreep::default())
        ))
    }).log_err();
}

fn get_tugboat_body(energy: u32, tugged: &Creep) -> Body {
    let tugged_body = Body::from(tugged);
    let target_tugboat_move_parts = tugged_body.0.len().saturating_sub(2 * tugged_body.part_count(Part::Move));

    let tugged_empty_carry = tugged.store().get_free_capacity(None).div_floor(50) as usize;
    let target_tugboat_move_parts = target_tugboat_move_parts.saturating_sub(tugged_empty_carry);

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

fn schedule_tugboats(roster: &mut ColonyRoster, tugboat_requests: &TugboatRequests) {
    let tugged = roster.syndrome.tugged_order()
        .unwrap_or_else(|| {
            tugboat_requests.0.iter()
            .filter(|tugged| roster.local_creeps.contains_key(&tugged.id()))
            .cloned()
            .collect_vec()
        });

    for tugged in tugged {
        if !roster.has_free() { continue; }
        if roster.local_creeps.of_role(RoleSelector::TugboatFor(tugged.id())).next().is_some() { continue; }

        roster.schedule_selected(
            |iter| {
                iter.min_by_key(|(_, spawn)| spawn.spawn.pos().get_range_to(tugged.pos()))
                    .map(|(ix, _)| ix)
            },
            |info| {
                Some(RelativePrototype { 
                    body: get_tugboat_body(info.future_energy, &tugged), 
                    role: CreepRole::Tugboat(tugged.id(), info.spawn.id()) 
                })
            }
        ).log_err();
    }
}

const TARGET_IDLE_FABRICATOR_WORK_COUNT: usize = 20;
const TARGET_SURPLUS_FABRICATOR_WORK_COUNT: usize = 40;
const BUFFER_ENERGY_SURPLUS_THRESHOLD: u32 = 50_000;
static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body(vec![Carry, Carry, Move, Work, Carry]) });
fn schedule_fabricators(roster: &mut ColonyRoster, colony: &ColonyView<'_>) {
    if roster.syndrome.any_problems() { return }

    let buffer_energy = colony.buffer.map_or(0, |buffer| buffer.used_energy_capacity());
    let work_target = if buffer_energy >= BUFFER_ENERGY_SURPLUS_THRESHOLD { TARGET_SURPLUS_FABRICATOR_WORK_COUNT } else { TARGET_IDLE_FABRICATOR_WORK_COUNT };

    while roster.has_free() {
        if roster.local_creeps.part_count(RoleSelector::Fabricator, Part::Work) >= work_target { break; }

        roster.schedule(|info| {
            Some(RelativePrototype { 
                body: FABRICATOR_TEMPLATE.scaled(info.future_energy, None), 
                role: CreepRole::Fabricator(FabricatorCreep::default()) 
            })
        }).log_err();
    }
}

fn schedule_remote_fabricators(rosters: &mut Rosters, mem: &mut Memory) {
    for colony in mem.colonies.view_all() {
        if !matches!(colony.step, ColonyStep::BuildSpawn) { continue; }

        let roster = rosters.rosters.get(&colony.name).unwrap();
        if roster.local_creeps.of_role(RoleSelector::Fabricator).next().is_some() { continue; }

        rosters.schedule(|info| {
            Some(Prototype::absolute(
                FABRICATOR_TEMPLATE.scaled(info.future_energy, None), 
                CreepRole::Fabricator(FabricatorCreep::default()), 
                colony.name
            ))
        }).log_err();
    }
}

#[expect(clippy::needless_pass_by_value)]
pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut rosters = Rosters::new(mem);

    for (colony, roster) in &mut rosters.rosters {
        let view = mem.colonies.view(*colony).unwrap();

        schedule_excavators(roster, &view);
        schedule_tugboats(roster, &tugboat_requests);
        schedule_trucks(roster, &view);
        schedule_fabricators(roster, &view);
    }

    schedule_remote_fabricators(&mut rosters, mem);
    schedule_flagships(&mut rosters, mem);
    schedule_import_trucks(&mut rosters, mem);

    rosters.gather_new_creeps(mem);
}