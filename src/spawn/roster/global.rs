use std::{cell::RefCell, collections::{HashMap, hash_map}, rc::Rc};

use screeps::RoomName;

use crate::{colony::ColonyView, creeps::{CreepRole, body::Body}, memory::Memory, names::UsedNames, spawn::roster::{ColonyRoster, creeps::GlobalCreeps}, statemachine::{Flow, OrBreak}};

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

    pub fn schedule_selected<S, B, R>(&mut self, select: S, make_body: B, make_role: R, home: Option<RoomName>) -> Flow
    where
        S: for<'a> FnOnce(hash_map::Iter<'a, RoomName, ColonyRoster>) -> Flow<RoomName>,
        B: FnOnce(&ColonyRoster) -> Flow<Body>,
        R: FnOnce(&ColonyRoster) -> CreepRole
    {
        let choice = select(self.rosters.iter())?;
        let home = home.unwrap_or(choice);
        assert!(self.rosters.contains_key(&home), "{home} is not a valid home colony");

        let roster = self.rosters.get_mut(&choice).expect("Room selection should return a valid room");

        let body = make_body(roster)?;
        let role = make_role(roster);
        let (id, proto) = roster.schedule_selected_absolute(ColonyRoster::default_select, body, role, home)?;

        self.rosters.get_mut(&home).expect("Home was validated before spawning").local_creeps.0.insert(id, proto.relative().clone());

        Flow::Continue(())
    }

    pub fn default_select(iter: hash_map::Iter<'_, RoomName, ColonyRoster>) -> Flow<RoomName> {
        iter.filter(|(_, roster)| roster.has_free())
            .max_by_key(|(_, roster)| roster.energy.future_energy())
            .map(|(room, _)| *room)
            .or_break()
    }

    pub fn schedule<B, R>(&mut self, make_body: B, make_role: R, home: Option<RoomName>) -> Flow
    where
        B: FnOnce(&ColonyRoster) -> Flow<Body>,
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
