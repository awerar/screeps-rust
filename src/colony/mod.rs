use std::fmt::Display;

use derive_where::derive_where;
use screeps::{HasPosition, Position, Room, Store, Structure, StructureContainer, StructureController, StructureStorage};

use crate::{check::{Check, CheckFrom}, colony::{plan::ColonyPlan, steps::ColonyStep}, domain_traits::{HasId, HasStore, ObjectId, ResolvableId, Transferable, Withdrawable}, ids::{CheckState, Checked, Unchecked}};

mod lifecycle;
pub mod plan;
mod planner;
mod registry;
pub mod steps;

pub use lifecycle::update_colonies;
pub use registry::{Colonies, Colony, ColonyCoordinators, ColonyName};

pub struct ColonyView<'mem> {
    pub plan: &'mem ColonyPlan,
    pub step: ColonyStep,
    pub name: ColonyName,
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
    pub fn new(name: ColonyName, plan: &'mem ColonyPlan, step: ColonyStep) -> Self {
        let buffer = plan.center.storage.resolve().map(|x| x.id()).map(ColonyBuffer::Storage)
            .or_else(|| plan.center.container_storage.resolve().map(|x| x.id()).map(ColonyBuffer::Container));

        ColonyView {
            plan,
            step,
            name,
            controller: name.controller(),
            room: name.room(),
            buffer,
            center: plan.center.pos
        }
    }
}

impl Colony {
    pub fn view(&self) -> ColonyView<'_> {
        ColonyView::new(self.name(), &self.plan, self.step)
    }

    pub fn split(&mut self) -> (ColonyView<'_>, &mut ColonyCoordinators) {
        let name = self.name();
        let Colony { plan, step, coordinators, .. } = self;
        (ColonyView::new(name, plan, *step), coordinators)
    }
}

impl Colonies {
    pub fn view(&self, name: ColonyName) -> ColonyView<'_> {
        self.get(name).view()
    }

    pub fn view_all(&self) -> impl Iterator<Item = ColonyView<'_>> {
        self.iter().map(Colony::view)
    }

    pub fn view_mut(&mut self, name: ColonyName) -> (ColonyView<'_>, &mut ColonyCoordinators) {
        self.get_mut(name).split()
    }

    pub fn view_all_mut(&mut self) -> impl Iterator<Item = (ColonyView<'_>, &mut ColonyCoordinators)> {
        self.iter_mut().map(Colony::split)
    }

    pub fn view_mut_disjoint<const N: usize>(&mut self, names: [ColonyName; N]) -> [(ColonyView<'_>, &mut ColonyCoordinators); N] {
        self.get_disjoint_mut(names).map(Colony::split)
    }

    pub fn coordinators(&self, name: ColonyName) -> &ColonyCoordinators {
        &self.get(name).coordinators
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

impl From<ColonyBuffer> for Structure {
    fn from(buffer: ColonyBuffer) -> Self {
        match buffer {
            ColonyBuffer::Container(id) => id.resolve().into(),
            ColonyBuffer::Storage(id) => id.resolve().into(),
        }
    }
}