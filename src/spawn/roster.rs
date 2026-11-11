use std::{cell::RefCell, collections::{HashMap, hash_map}, rc::Rc};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, RoomName, Source, SpawnOptions, Structure, StructureSpawn, action_error_codes::SpawnCreepErrorCode, game};
use thiserror::Error;

use crate::{colony::ColonyView, creeps::{CreepData, CreepRole, excavator}, domain_traits::{CreepId, HasId, ObjectId, ResolvableId}, memory::Memory, names::UsedNames, spawn::{budget::ColonyBudget, energy::{ColonyEnergy, EnergyGroup, EnergyStructure}, prototype::{AbsolutePrototype, Body, RelativePrototype}, roles::RoleSelector}};

pub type SharedUsedNames = Rc<RefCell<UsedNames>>;
pub type SharedColonyBudgets = Rc<RefCell<HashMap<RoomName, ColonyBudget>>>;

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
            any_excavating_excavators: creeps.iter().any(|(creep, proto)| {
                let CreepRole::Excavator(source) = proto.role() else { return false; };
                excavator::is_in_position(creep.resolve().pos(), source, view)
            }),
            excavators:
                view.plan.sources.keys()
                    .filter_map(|id| Some(id.resolve()?.id()))
                    .filter_map(|source| {
                        let Some((excavator, _)) = creeps.0.iter().find(|(_, proto)| matches!(proto.role(), CreepRole::Excavator(source2) if source == *source2)) else {
                            return Some((source, ExcavatorSyndrome::NoExcavator));
                        };

                        if !excavator::is_in_position(excavator.resolve().pos(), &source, view)
                            && !creeps.any_of_role(RoleSelector::TugboatFor(excavator.clone())){
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

    budgets: SharedColonyBudgets,
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

pub enum ScheduleDecision {
    Scheduled(CreepId, AbsolutePrototype),
    WaitingForEnergy,
    WaitingForSpawn
}

#[derive(Debug, Error)]
pub enum ColonyScheduleError {
    #[error(transparent)] SpawnError(#[from] SpawnCreepErrorCode),
    #[error("Not enough energy")] NotEnoughEnergy
}

pub type ColonyScheduleResult = Result<ScheduleDecision, ColonyScheduleError>;

impl ColonyRoster {
    pub fn new(colony: &ColonyView<'_>, mem: &Memory, names: SharedUsedNames, budgets: SharedColonyBudgets, global_creeps: &GlobalCreeps) -> Self {
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

        budgets.borrow_mut().insert(colony.name, ColonyBudget::new(colony, global_creeps));

        Self {
            spawns,
            energy: ColonyEnergy::new(groups),
            names,
            name: colony.name,
            local_creeps,
            budgets,
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

    pub fn full_budget(&self) -> f32 {
        self.budgets.borrow()[&self.name].full()
    }

    pub fn sustainable_budget(&self) -> f32 {
        self.budgets.borrow()[&self.name].sustainable()  
    }

    pub fn future_energy(&self) -> u32 {
        self.energy.future_energy()
    }

    pub fn room(&self) -> RoomName {
        self.name
    }

    // Not meant to be used by user
    fn schedule_selected_absolute<S>(&mut self, select: S, body: Body, role: CreepRole, home: RoomName) -> ColonyScheduleResult
    where
        S: for<'b> FnOnce(ColonySpawnIterator<'b>) -> Option<usize>
    {
        
        let Some(choice) = select(ColonySpawnIterator { index: 0, spawns: &self.spawns }) else { return Ok(ScheduleDecision::WaitingForSpawn) };
        let spawn = self.spawns.get_mut(choice).expect("Spawn selection should return a valid index");
        
        let proto = AbsolutePrototype::new(body, role, home, spawn.spawn.room().unwrap().name());

        assert!(spawn.is_free());

        let cost = proto.body().energy_required();

        if cost > self.energy.future_energy() { return Err(ColonyScheduleError::NotEnoughEnergy) }
        if cost > self.energy.energy() {
            spawn.block();
            self.energy.reserve_future(cost);
            return Ok(ScheduleDecision::WaitingForEnergy)
        }

        let structures = self.energy.allocate(cost);

        let dirs = if let Some(dir) = spawn.source_direction() {
            if let CreepRole::Excavator(source) = proto.role() && spawn.is_source_spawn(source) {
                vec![dir]
            } else {
                Direction::iter().filter(|dir2| **dir2 != dir).copied().collect_vec()
            }
        } else {
            Direction::iter().copied().collect_vec()
        };

        let name = self.names.borrow_mut().generate_new(proto.role());
        spawn.spawn.spawn_creep_with_options(
            proto.body().parts(),
            &name,
            &SpawnOptions::new()
                .energy_structures(structures.into_iter().map_into::<Structure>())
                .directions(&dirs)
        )?;

        for budget in self.budgets.borrow_mut().values_mut() {
            budget.add_creep(&proto);
        }

        let id = game::creeps().get(name).unwrap().id();
        spawn.begin_spawning(id.clone(), CreepData { role: proto.role().clone(), home: proto.home(), spawn_room: self.name }, dirs);

        Ok(ScheduleDecision::Scheduled(id, proto))
    }

    pub fn schedule_selected<S>(&mut self, select: S, body: Body, role: CreepRole) -> ColonyScheduleResult
    where
        S: for<'a> FnOnce(ColonySpawnIterator<'a>) -> Option<usize>
    {
        let home = self.name;
        let result = self.schedule_selected_absolute(select, body, role, home);

        if let Ok(ScheduleDecision::Scheduled(id, proto)) = &result {
            self.local_creeps.0.insert(id.clone(), proto.relative().clone());
        }

        result
    }

    pub fn default_select(iter: ColonySpawnIterator<'_>) -> Option<usize> {
        iter.max_by_key(|(_, spawn)| spawn.is_central()).map(|(ix, _)| ix)
    }

    pub fn schedule(&mut self, body: Body, role: CreepRole) -> ColonyScheduleResult {
        self.schedule_selected(Self::default_select, body, role)
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
    #[error("Unable to create prototype")] NoBody,
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
        let budgets = Rc::new(RefCell::new(HashMap::new()));
        let global_creeps = GlobalCreeps::new(mem);

        Self {
            rosters: mem.colonies.view_all()
                .map(|colony| (colony.name, ColonyRoster::new(&colony, mem, names.clone(), budgets.clone(), &global_creeps)))
                .collect(),
            global_creeps
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&RoomName, &mut ColonyRoster)> {
        self.rosters.iter_mut()
    }

    pub fn get(&self, colony: &ColonyView<'_>) -> &ColonyRoster {
        self.rosters.get(&colony.name).unwrap()
    }

    pub fn global_creeps(&self) -> &GlobalCreeps {
        &self.global_creeps
    }

    pub fn schedule_selected<S, B, R>(&mut self, select: S, make_body: B, make_role: R, home: Option<RoomName>) -> GlobalScheduleResult
    where
        S: for<'a> FnOnce(hash_map::Iter<'a, RoomName, ColonyRoster>) -> Option<RoomName>,
        B: FnOnce(&ColonyRoster) -> Option<Body>,
        R: FnOnce(&ColonyRoster) -> CreepRole
    {
        let Some(choice) = select(self.rosters.iter()) else { return Err(GlobalScheduleError::NoRoom) };
        let roster = self.rosters.get_mut(&choice).expect("Room selection should return a valid room");

        let body = make_body(roster).ok_or(GlobalScheduleError::NoBody)?;
        let result = roster.schedule_selected_absolute(ColonyRoster::default_select, body, make_role(roster), home.unwrap_or(choice));

        if let Ok(ScheduleDecision::Scheduled(id, proto)) = &result {
            let roster = self.rosters.get_mut(&proto.home()).ok_or(GlobalScheduleError::InvalidHome(proto.home()))?;
            roster.local_creeps.0.insert(id.clone(), proto.relative().clone());
        }

        result.map_err(GlobalScheduleError::from)
    }

    pub fn default_select(iter: hash_map::Iter<'_, RoomName, ColonyRoster>) -> Option<RoomName> {
        iter.max_by_key(|(_, roster)| roster.energy.future_energy()).map(|(room, _)| *room)
    }

    pub fn schedule<B, R>(&mut self, make_body: B, make_role: R, home: Option<RoomName>) -> GlobalScheduleResult
    where
        B: FnOnce(&ColonyRoster) -> Option<Body>,
        R: FnOnce(&ColonyRoster) -> CreepRole
    {
        self.schedule_selected(Self::default_select, make_body, make_role, home)
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        for colony_spawns in self.rosters.into_values() {
            colony_spawns.gather_new_creeps(mem);
        }
    }
}
