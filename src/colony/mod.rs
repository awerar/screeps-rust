use std::{collections::{HashMap, HashSet, hash_map}, fmt::Display};

use derive_where::derive_where;
use js_sys::JsString;
use screeps::{HasPosition, OwnedStructureProperties, Position, Room, RoomName, Store, StructureContainer, StructureController, StructureStorage, find, game};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use tap::Tap;

use crate::{check::{Check, CheckFrom}, colony::{planning::{plan::ColonyPlan, planned_ref::ResolvableStructureRef}, steps::ColonyStep}, commands::{Command, handle_commands, pop_command}, domain_traits::{HasId, HasStore, ObjectId, ResolvableId, Transferable, Withdrawable}, ids::{CheckState, Checked, Unchecked}, memory::Memory, statemachine::step, visuals::{RoomDrawerType, draw_in_room_replaced}};

pub mod planning;
pub mod steps;

#[derive(Serialize, Deserialize, Default)]
pub struct Colonies(HashMap<RoomName, (ColonyPlan, ColonyStep)>);

pub struct ColonyView<'mem> {
    pub plan: &'mem ColonyPlan,
    pub step: ColonyStep,
    pub name: RoomName,
    pub room: Room,
    pub controller: StructureController,
    pub buffer: Option<ColonyBuffer>,
    pub center: Position
}

impl Display for ColonyView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl<'mem> ColonyView<'mem> {
    pub fn new(room: Room, plan: &'mem ColonyPlan, step: ColonyStep) -> Self {
        let buffer = plan.center.storage.resolve().map(|x| x.id()).map(ColonyBuffer::Storage)
            .or_else(|| plan.center.container_storage.resolve().map(|x| x.id()).map(ColonyBuffer::Container));

        ColonyView { 
            plan, 
            step, 
            name: room.name(), 
            controller: room.controller().expect("Every colony should have a controller"), 
            room, 
            buffer,
            center: plan.center.pos
        }
    }
}

impl Colonies {
    pub fn view(&self, name: RoomName) -> Option<ColonyView<'_>> {
        let (plan, step) = self.0.get(&name)?;
        Some(ColonyView::new(game::rooms().get(name)?, plan, *step))
    }

    pub fn view_all(&self) -> impl Iterator<Item = ColonyView<'_>> {
        self.0.keys().filter_map(|name| self.view(*name))
    }

    pub fn rooms(&self) -> impl Iterator<Item = RoomName> {
        self.0.keys().copied()
    }
}

#[derive_where(Debug, Serialize, Deserialize, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord; ObjectId<StructureContainer, S>, ObjectId<StructureStorage, S>)]
pub enum ColonyBuffer<S: CheckState = Checked> {
    Container(ObjectId<StructureContainer, S>),
    Storage(ObjectId<StructureStorage, S>)
}

trait ColonyBufferStructure: HasStore + Withdrawable + Transferable + HasPosition {}
impl ColonyBufferStructure for StructureContainer {}
impl ColonyBufferStructure for StructureStorage {}

impl ColonyBuffer {
    fn with<R>(&self, f: impl FnOnce(&dyn ColonyBufferStructure) -> R) -> R {
        match self {
            Self::Container(id) => f(&id.resolve()),
            Self::Storage(id) => f(&id.resolve())
        }
    }

    pub fn resolve_storage(&self) -> Option<StructureStorage> {
        match self {
            ColonyBuffer::Container(_) => None,
            ColonyBuffer::Storage(id) => Some(id.resolve()),
        }
    }
}

impl CheckFrom for ColonyBuffer {
    type Unchecked = ColonyBuffer<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(match uc {
            ColonyBuffer::Container(container) => 
                Self::Container(container.check()?),
            ColonyBuffer::Storage(storage) => 
                Self::Storage(storage.check()?),
        })
    }
}

impl HasStore for ColonyBuffer {
    fn store(&self) -> Store {
        self.with(|s| s.store())
    }
}

impl Withdrawable for ColonyBuffer {
    fn withdraw_to(&self, creep: &screeps::Creep, ty: screeps::ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::WithdrawErrorCode> {
        self.with(|s| s.withdraw_to(creep, ty, amount))
    }
}

impl Transferable for ColonyBuffer {
    fn transfer_from(&self, creep: &screeps::Creep, ty: screeps::ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::TransferErrorCode> {
        self.with(|s| s.transfer_from(creep, ty, amount))
    }
}

impl HasPosition for ColonyBuffer {
    fn pos(&self) -> Position {
        self.with(|s| s.pos())
    }
}

pub fn update_colonies(mem: &mut Memory) {
    info!("Updating rooms...");

    handle_commands(|command| {
        let Command::ResetColony { room: name } = command else { return false; };
        let Ok(name) = RoomName::new(name) else { return true; };
        mem.colonies.0.remove(&name);
        true
    });

    let prev_colonies: HashSet<_> = mem.colonies.0.keys().copied().collect();
    let curr_colonies: HashSet<_> = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name).collect();

    if curr_colonies.len() == 1 {
        let room = curr_colonies.iter().next().unwrap();
        let room = game::rooms().get(*room).unwrap();
        let controller = room.controller().unwrap();

        if controller.level() == 1 && controller.progress().unwrap() == 0
            && room.find(find::MY_CREEPS, None).is_empty() 
            && room.find(find::MY_STRUCTURES, None).len() == 2
            && room.find(find::MY_CONSTRUCTION_SITES, None).is_empty()
            && room.find(find::FLAGS, None).is_empty() {
                let spawn = room.find(find::MY_SPAWNS, None).into_iter().next().unwrap();
                spawn.pos().create_flag(Some(&JsString::from("Center")), None, None).ok();
            }
    }

    let lost_colonies = prev_colonies.difference(&curr_colonies);
    for room in lost_colonies {
        mem.colonies.0.remove(room);
        mem.truck_coordinators.remove(room);
        mem.fabricator_coordinators.remove(room);
        warn!("Lost colony {room}");
    }

    for name in curr_colonies {
        let room = game::rooms().get(name).unwrap();

        if let hash_map::Entry::Vacant(e) = mem.colonies.0.entry(name) {
            let plan = ColonyPlan::create_for(&room);
            let Ok(plan) = plan else {
                let Err(err) = plan else { unreachable!() };
                warn!("Unable to create plan for {name}: {err}");
                continue;
            };

            let diff = plan.diff_with(&room);
            if !diff.compatible() {
                if pop_command(Command::MigrateColony { room: name.to_string() }) {
                    info!("Migrating {name}");
                    diff.migrate(name);
                } else {
                    diff.draw(name);

                    let plan_clone = plan.clone();
                    draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
                    warn!("Plan for {name} is not compatible with current layout");
                    continue;
                }
            }

            let plan = plan.tap_mut(|plan| plan.adapt_build_times_to(&room));

            e.insert((plan, ColonyStep::default()));
        }

        if pop_command(Command::ResetColonyStep { room: name.to_string() }) {
            mem.colonies.0.get_mut(&name).unwrap().1 = ColonyStep::default();
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: false }) {
            let plan_clone = mem.colonies.0.get(&name).unwrap().0.clone();
            draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: true }) {
            let plan_clone = mem.colonies.0.get(&name).unwrap().0.clone();
            plan_clone.draw_progression(name);
        }


        let (plan, stp) = mem.colonies.0.get_mut(&name).unwrap();
        let view = ColonyView::new(room.clone(), plan, *stp);
        step(stp, |stp| stp.update(&room, &view));

        info!("{name} is at step {stp:?}");
    }
}