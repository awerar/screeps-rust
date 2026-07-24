use std::{collections::HashMap, fmt::Display};

use derive_where::derive_where;
use screeps::{HasPosition, Position, Room, RoomName, Store, StructureContainer, StructureController, StructureStorage, game};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, CheckFrom}, colony::{plan::{ColonyPlan, refs::ResolvableStructureRef}, steps::ColonyStep}, domain_traits::{HasId, HasStore, ObjectId, ResolvableId, Transferable, Withdrawable}, ids::{CheckState, Checked, Unchecked}};

mod lifecycle;
pub mod plan;
mod planner;
pub mod steps;

pub use lifecycle::update_colonies;

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
