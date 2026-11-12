use std::{cell::RefCell, collections::{HashMap, hash_map}, rc::Rc};

use screeps::RoomName;
use thiserror::Error;

use crate::{colony::ColonyView, creeps::{CreepRole, body::Body}, memory::Memory, names::UsedNames, spawn::roster::{ColonyRoster, ColonyScheduleError, ScheduleDecision, creeps::GlobalCreeps}};

#[derive(Debug, Error)]
pub enum GlobalScheduleError {
    #[error("No valid room")] NoRoom,
    #[error("Unable to create prototype")] NoBody,
    #[error("{0} is not a valid home colony")] InvalidHome(RoomName),
    #[error(transparent)] Roster(#[from] ColonyScheduleError),
}

pub type GlobalScheduleResult = Result<ScheduleDecision, GlobalScheduleError>;

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
