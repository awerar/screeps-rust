use screeps::{Creep, HasId, HasPosition, HasStore, MaybeHasId, ObjectId, Position, ResourceType, RoomName, SharedCreepProperties, Store, Structure, StructureObject, Transferable, Withdrawable, action_error_codes::{TransferErrorCode, WithdrawErrorCode}};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::{colony::planning::planned_ref::{ConstructionType, OptionalPlannedStructureRef, PlannedStructureRef, PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}, memory::Memory, statemachine::StateMachine, tasks::MultiTasksQueue};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
pub enum TruckCreep {
    #[default] Idle,
    CollectingFrom(ProviderId),
    ProvidingTo(ConsumerId)
}

impl StateMachine<Creep> for TruckCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        /*let coordinator = mem.truck_coordinators.entry(mem.creep(creep).unwrap().home).or_default();

        match self {
            Self::Idle => {
                let used_amount = creep.store().get_used_capacity(Some(ResourceType::Energy));
                let free_amount = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;

                if used_amount == 0 {
                    Ok(coordinator.providers.assign_task_to(creep, free_amount).map_or(TruckCreep::Idle, TruckCreep::CollectingFrom))
                } else {
                    Ok(coordinator.consumers.assign_task_to(creep, used_amount).map_or(TruckCreep::Idle, TruckCreep::ProvidingTo))
                }
            },
            Self::CollectingFrom(provider) => {
                coordinator.providers.heartbeat(creep);

                let Some(target) = provider.resolve_pos() else { return Err(()) };

                if !creep.pos().is_near_to(target) {
                    mem.movement.smart_move_creep_to(creep, target).ok();
                    return Ok(self.clone());
                }

                coordinator.providers.finish(creep);
                provider.withdraw(creep, ResourceType::Energy, None).map(|()| Self::Idle)
            },
            Self::ProvidingTo(consumer) => {
                coordinator.consumers.heartbeat(creep);

                let Some(target) = consumer.resolve_pos() else { return Err(()) };

                if !creep.pos().is_near_to(target) {
                    mem.movement.smart_move_creep_to(creep, target).ok();
                    return Ok(self.clone());
                }

                coordinator.consumers.finish(creep);
                consumer.transfer(creep, ResourceType::Energy, None).map(|()| Self::Idle)
            },
        }*/

        todo!()
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TruckTaskCoordinator {
    providers: MultiTasksQueue<ProviderId>,
    consumers: MultiTasksQueue<ConsumerId>
}

impl TruckTaskCoordinator {
    fn update(&mut self, room: RoomName, mem: &Memory) {
        let plan = &mem.colony(room).unwrap().plan;

        let mut providers = Vec::new();
        providers.extend(plan.center.link.resolve_provider());
        providers.extend(plan.sources.source_containers.resolve_providers());

        self.providers.set_tasks(
            providers.into_iter()
                .map(|(provider, store)| (provider, store.get_used_capacity(Some(ResourceType::Energy))))
        );

        let mut consumers = Vec::new();
        consumers.extend(plan.center.spawn.resolve_consumer());
        consumers.extend(plan.center.extensions.resolve_consumers());
        consumers.extend(plan.center.towers.resolve_consumers());
        consumers.extend(plan.center.terminal.resolve_consumer());
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ProviderId(ObjectId<Structure>);
pub trait ProviderReqs = Withdrawable + HasStore + Into<Structure>;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ConsumerId(ObjectId<Structure>);
pub trait ConsumerReqs = Transferable + HasStore + Into<Structure>;

impl ProviderId {
    pub fn new<T: ProviderReqs>(t: T) -> Self {
        Self(t.into().id())
    }

    pub fn withdraw(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        let withdraw_result = creep.withdraw(structure.as_withdrawable().unwrap(), ty, amount);

        if matches!(withdraw_result, Ok(_) | Err(WithdrawErrorCode::Full)) { Ok(()) } else { Err(()) }
    }

    pub fn store(&self) -> Result<Store, ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        structure.as_has_store().map(HasStore::store).ok_or(())
    }

    pub fn resolve_pos(&self) -> Option<Position> {
        self.0.resolve().map(|structure| structure.pos())
    }
}

trait ResolvableProviderRef { fn resolve_provider(&self) -> Option<(ProviderId, Store)>; }
impl<R: ResolvableStructureRef> ResolvableProviderRef for R where R::Structure : ProviderReqs + StructureRefReq {
    fn resolve_provider(&self) -> Option<(ProviderId, Store)> {
        let structure = self.resolve()?;
        let store = structure.store();
       Some((ProviderId::new(structure), store))
    }
}

impl<T : ProviderReqs + StructureRefReq> PlannedStructureRefs<T> {
    fn resolve_providers(&self) -> Vec<(ProviderId, Store)> {
        self.0.iter().filter_map(|r| r.resolve_provider()).collect()
    }
}

impl ConsumerId {
    pub fn new<T: ConsumerReqs>(t: T) -> Self {
        Self(t.into().id())
    }

    pub fn transfer(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        let withdraw_result = creep.transfer(structure.as_transferable().unwrap(), ty, amount);

        if matches!(withdraw_result, Ok(_) | Err(TransferErrorCode::Full)) { Ok(()) } else { Err(()) }
    }

    pub fn store(&self) -> Result<Store, ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        structure.as_has_store().map(HasStore::store).ok_or(())
    }

    pub fn resolve_pos(&self) -> Option<Position> {
        self.0.resolve().map(|structure| structure.pos())
    }
}

trait ResolvableConsumerRef { fn resolve_consumer(&self) -> Option<(ConsumerId, Store)>; }
impl<R: ResolvableStructureRef> ResolvableConsumerRef for R where R::Structure : ConsumerReqs + StructureRefReq {
    fn resolve_consumer(&self) -> Option<(ConsumerId, Store)> {
        let structure = self.resolve()?;
        let store = structure.store();
       Some((ConsumerId::new(structure), store))
    }
}

impl<T : ConsumerReqs + StructureRefReq> PlannedStructureRefs<T> {
    fn resolve_consumers(&self) -> Vec<(ConsumerId, Store)> {
        self.0.iter().filter_map(|r| r.resolve_consumer()).collect()
    }
}