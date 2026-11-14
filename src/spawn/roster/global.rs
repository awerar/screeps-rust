use std::{cell::RefCell, collections::HashMap, rc::Rc};

use anyhow::anyhow;

use crate::{colony::ColonyName, creeps::{CreepRole, body::Body}, memory::Memory, names::UsedNames, spawn::roster::{ColonyRoster, creeps::GlobalCreeps}, statemachine::{Flow, OrBreak}};

pub struct Rosters {
    rosters: HashMap<ColonyName, ColonyRoster>,
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

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut ColonyRoster> {
        self.rosters.values_mut()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ColonyRoster> {
        self.rosters.values()
    }

    pub fn get(&self, name: ColonyName) -> &ColonyRoster {
        self.rosters.get(&name).expect("Rosters should contain one roster for each colony")
    }

    pub fn get_mut(&mut self, name: ColonyName) -> &mut ColonyRoster {
        self.rosters.get_mut(&name).expect("Rosters should contain one roster for each colony")
    }

    pub fn global_creeps(&self) -> &GlobalCreeps {
        &self.global_creeps
    }

    pub fn schedule_selected(&mut self, spawn: ColonyName, body: Body, role: CreepRole, home: ColonyName) -> Flow {
        if !self.get(spawn).is_avaliable() { return Flow::Fail(anyhow!("Can only select avaliable rosters")) }

        let roster = self.get_mut(spawn); 

        let (id, proto) = roster.schedule_selected_absolute(ColonyRoster::default_select, body, role, home)?;
        roster.local_creeps.0.insert(id, proto.relative().clone());

        Flow::Continue(())
    }

    pub fn schedule(&mut self, body: Body, role: CreepRole, home: Option<ColonyName>) -> Flow {
        let spawn = self.iter()
            .filter(|roster| roster.has_free())
            .max_by_key(|roster| roster.energy.future_energy())
            .map(|roster| roster.room())
            .or_break()?;

        self.schedule_selected(spawn, body, role, home.unwrap_or(spawn))
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        for colony_spawns in self.rosters.into_values() {
            colony_spawns.gather_new_creeps(mem);
        }
    }
}