use std::{cell::RefCell, collections::{HashMap, hash_map}, rc::Rc};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, RoomName, Source, SpawnOptions, Structure, StructureSpawn, action_error_codes::SpawnCreepErrorCode, game};
use thiserror::Error;

use crate::{colony::{ColonyView, plan::refs::ResolvableStructureRef}, creeps::{CreepData, CreepRole, excavator::ExcavatorCreep}, domain_traits::{CreepId, HasId, ObjectId, ResolvableId}, memory::Memory, names::UsedNames, spawn::{energy::{ColonyEnergy, EnergyGroup, EnergyStructure}, prototype::{AbsolutePrototype, Prototype, RelativePrototype}}};

pub type SharedUsedNames = Rc<RefCell<UsedNames>>;

enum ExcavatorSyndrome {
    NoExcavator,
    NoTugboatFor(CreepId)
}

pub struct ColonySyndrome {
    any_trucks: bool,
    any_excavating_excavators: bool,
    excavators: HashMap<ObjectId<Source>, ExcavatorSyndrome>
}

impl ColonySyndrome {
    fn new(creeps: &ColonyCreeps, view: &ColonyView<'_>) -> Self {
        Self {
            any_trucks: creeps.0.values().any(|proto| matches!(proto.role(), CreepRole::Truck(_))),
            any_excavating_excavators: creeps.0.values().any(|proto| matches!(proto.role(), CreepRole::Excavator(ExcavatorCreep::Mining, _))),
            excavators:
                view.plan.sources.keys()
                    .filter_map(|id| Some(id.resolve()?.id()))
                    .filter_map(|source| {
                        let Some((excavator, proto)) = creeps.0.iter().find(|(_, proto)| matches!(proto.role(), CreepRole::Excavator(_, source2) if source == *source2)) else {
                            return Some((source, ExcavatorSyndrome::NoExcavator));
                        };

                        let CreepRole::Excavator(state, _) = proto.role() else { unreachable!() };

                        if matches!(state, ExcavatorCreep::Going)
                            && creeps.0.values().all(|proto| !matches!(proto.role(), CreepRole::Tugboat(tugged, _) if *excavator == *tugged)) {
                                return Some((source, ExcavatorSyndrome::NoTugboatFor(excavator.clone())));
                        }

                        None
                    }).collect()
        }
    }

    pub fn any_problems(&self) -> bool {
        !self.any_trucks || !self.any_excavating_excavators || !self.excavators.is_empty()
    }

    pub fn tugged_order(&self) -> Option<Vec<Creep>> {
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

#[derive(Deref)]
pub struct ColonyCreeps(HashMap<CreepId, RelativePrototype>);

impl ColonyCreeps {
    pub fn new(colony: RoomName, mem: &Memory) -> Self {
        Self(
            mem.creeps.iter()
                .filter(|(_, data)| data.home == colony)
                .map(|(id, data)| (id.clone(), RelativePrototype::from_creep(id, data)))
                .collect()
        )
    }
}

enum SpawnState {
    Free,
    Blocked,
    Spawning(CreepId, CreepData, Vec<Direction>)
}

pub enum ColonySpawnType {
    Central,
    Source(ObjectId<Source>, Direction)
}

pub struct ColonySpawn {
    pub spawn: StructureSpawn,
    state: SpawnState,
    ty: ColonySpawnType
}

impl ColonySpawn {
    pub fn new(spawn: StructureSpawn, ty: ColonySpawnType) -> Self {
        Self {
            state: if spawn.spawning().is_some() { SpawnState::Blocked } else { SpawnState::Free },
            spawn,
            ty
        }
    }

    pub fn is_free(&self) -> bool {
        matches!(self.state, SpawnState::Free)
    }

    pub fn is_central(&self) -> bool {
        matches!(self.ty, ColonySpawnType::Central)
    }

    pub fn is_source_spawn(&self, source: &ObjectId<Source>) -> bool {
        matches!(self.ty, ColonySpawnType::Source(source2, _) if *source == source2)
    }

    pub fn source_direction(&self) -> Option<Direction> {
        if let ColonySpawnType::Source(_, dir) = &self.ty { 
            Some(*dir) 
        } else { 
            None 
        }
    }

    pub fn block(&mut self) {
        self.state = SpawnState::Blocked;
    }

    pub fn begin_spawning(&mut self, id: CreepId, data: CreepData, dirs: Vec<Direction>) {
        self.state = SpawnState::Spawning(id, data, dirs);
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        if let SpawnState::Spawning(id, creep_data, dirs) = self.state  {
            mem.creeps.insert(id.clone(), creep_data);
            mem.movement.spawning_directions.insert(id, dirs);
        }
    }
}

pub struct ColonyRoster {
    name: RoomName,

    spawns: Vec<ColonySpawn>,
    energy: ColonyEnergy,
    local_creeps: ColonyCreeps,

    syndrome: ColonySyndrome,

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

pub struct SpawnInfo {
    pub spawn: StructureSpawn,
    pub energy: u32,
    pub future_energy: u32
}

pub enum ScheduleDecision {
    Scheduled(CreepId, AbsolutePrototype),
    WaitingForEnergy,
    WaitingForSpawn
}

#[derive(Debug, Error)]
pub enum ColonyScheduleError {
    #[error(transparent)] SpawnError(#[from] SpawnCreepErrorCode),
    #[error("No prototype")] NoPrototype,
    #[error("Not enough energy")] NotEnoughEnergy
}

pub type ColonyScheduleResult = Result<ScheduleDecision, ColonyScheduleError>;

impl ColonyRoster {
    pub fn new(colony: &ColonyView<'_>, mem: &Memory, names: SharedUsedNames) -> Self {
        let local_creeps = ColonyCreeps::new(colony.name, mem);
        let syndrome = ColonySyndrome::new(&local_creeps, colony);

        let mut groups = Vec::new();

        groups.push(EnergyGroup::new(
            colony.plan.center.spawn.resolve()
                .map(|spawn| EnergyStructure::Spawn(spawn.id()))
                .into_iter()
                .chain(
                    colony.plan.center.extensions.resolve().into_iter()
                    .sorted_by_cached_key(|extension| extension.pos().get_range_to(colony.center))
                    .map(|ext| EnergyStructure::Extension(ext.id()))
                ).collect(),
            syndrome.any_excavating_excavators && syndrome.any_trucks
        ));

        for (source, source_plan) in &colony.plan.sources {
            let Some(source) = source.resolve().map(|src| src.id()) else { continue; };

            groups.push(EnergyGroup::new(
                source_plan.spawn.resolve()
                    .map(|spawn| EnergyStructure::Spawn(spawn.id()))
                    .into_iter()
                    .chain(
                        source_plan.extensions.resolve().into_iter()
                            .map(|ext| EnergyStructure::Extension(ext.id())))
                    .collect(),
                !syndrome.excavators.contains_key(&source)
            ));
        }

        let mut spawns = Vec::new();

        spawns.extend(
            colony.plan.center.spawn.resolve()
                .map(|spawn| {
                    ColonySpawn::new(
                        spawn,
                        ColonySpawnType::Central 
                    )
                })
        );
        
        spawns.extend(
            colony.plan.sources.iter()
                .filter_map(|(source, plan)| {
                    Some(ColonySpawn::new(
                        plan.spawn.resolve()?,
                        ColonySpawnType::Source(source.resolve()?.id(), plan.spawn_direction)
                    ))
                })
        );

        Self {
            spawns,
            energy: ColonyEnergy::new(groups),
            names,
            name: colony.name,
            local_creeps,
            syndrome
        }
    }

    fn free_spawns(&self) -> impl Iterator<Item = &ColonySpawn> {
        self.spawns.iter().filter(|spawn| spawn.is_free())
    }

    pub fn has_free(&self) -> bool {
        self.free_spawns().next().is_some()
    }

    pub fn local_creeps(&self) -> &ColonyCreeps {
        &self.local_creeps
    }

    pub fn syndrome(&self) -> &ColonySyndrome {
        &self.syndrome
    }

    // Not meant to be used by user
    fn schedule_selected_absolute<S, P>(&mut self, select: S, make_proto: P) -> ColonyScheduleResult
    where
        S: for<'b> FnOnce(ColonySpawnIterator<'b>) -> Option<usize>,
        P: FnOnce(SpawnInfo) -> Option<AbsolutePrototype>
    {
        let Some(choice) = select(ColonySpawnIterator { index: 0, spawns: &self.spawns }) else { return Ok(ScheduleDecision::WaitingForSpawn) };
        let spawn = self.spawns.get_mut(choice).expect("Spawn selection should return a valid index");
        let spawn_info = SpawnInfo {
            spawn: spawn.spawn.clone(),
            energy: self.energy.energy(),
            future_energy: self.energy.future_energy()
        };

        let proto = make_proto(spawn_info).ok_or(ColonyScheduleError::NoPrototype)?;

        assert!(spawn.is_free());

        let cost = proto.body().energy_required();

        if cost > self.energy.future_energy() { return Err(ColonyScheduleError::NotEnoughEnergy) }
        if cost > self.energy.energy() {
            spawn.block();
            self.energy.reserve_future(cost);
            return Ok(ScheduleDecision::WaitingForEnergy)
        }

        let structures = self.energy.allocate(cost);

        let name = self.names.borrow_mut().generate_new(proto.role());
        spawn.spawn.spawn_creep_with_options(
            proto.body().parts(),
            &name,
            &SpawnOptions::new()
                .energy_structures(structures.into_iter().map_into::<Structure>())
        )?;

        let dirs = if let Some(dir) = spawn.source_direction() {
            if let CreepRole::Excavator(_, source) = proto.role() && spawn.is_source_spawn(source) {
                vec![dir]
            } else {
                Direction::iter().filter(|dir2| **dir2 != dir).copied().collect_vec()
            }
        } else {
            Direction::iter().copied().collect_vec()
        };

        let id = game::creeps().get(name).unwrap().id();
        spawn.begin_spawning(id.clone(), CreepData { role: proto.role().clone(), home: proto.home() }, dirs);

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
            self.local_creeps.0.insert(id.clone(), proto.relative().clone());
        }

        result
    }

    pub fn default_select(iter: ColonySpawnIterator<'_>) -> Option<usize> {
        iter.max_by_key(|(_, spawn)| spawn.is_central()).map(|(ix, _)| ix)
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
pub enum GlobalScheduleError {
    #[error("No valid room")] NoRoom,
    #[error("{0} is not a valid home colony")] InvalidHome(RoomName),
    #[error(transparent)] Roster(#[from] ColonyScheduleError),
}

pub type GlobalScheduleResult = Result<ScheduleDecision, GlobalScheduleError>;

#[derive(Deref)]
pub struct GlobalCreeps(HashMap<CreepId, AbsolutePrototype>);

impl GlobalCreeps {
    fn new(mem: &Memory) -> Self {
        Self(mem.creeps.iter()
            .map(|(id, data)| (id.clone(), AbsolutePrototype::from_creep(id, data)))
            .collect())
    }
}

pub struct Rosters {
    rosters: HashMap<RoomName, ColonyRoster>,
    global_creeps: GlobalCreeps
}

impl Rosters {
    pub fn new(mem: &Memory) -> Self {
        let names = Rc::new(RefCell::new(UsedNames::new()));

        Self {
            rosters: mem.colonies.view_all()
                .map(|colony| (colony.name, ColonyRoster::new(&colony, mem, names.clone())))
                .collect(),
            global_creeps: GlobalCreeps::new(mem)
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&RoomName, &mut ColonyRoster)> {
        self.rosters.iter_mut()
    }

    pub fn get(&self, room: RoomName) -> Option<&ColonyRoster> {
        self.rosters.get(&room)
    }

    pub fn global_creeps(&self) -> &GlobalCreeps {
        &self.global_creeps
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
            self.rosters.get_mut(&proto.home())
                .ok_or(GlobalScheduleError::InvalidHome(proto.home()))?
                .local_creeps.0.insert(id.clone(), proto.relative().clone());
        }

        result.map_err(GlobalScheduleError::from)
    }

    pub fn default_select(iter: hash_map::Iter<'_, RoomName, ColonyRoster>) -> Option<RoomName> {
        iter.max_by_key(|(_, roster)| roster.energy.future_energy()).map(|(room, _)| *room)
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
