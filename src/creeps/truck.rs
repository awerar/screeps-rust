use screeps::{Creep, HasId, HasPosition, HasStore, MaybeHasId, ObjectId, Position, ResourceType, RoomName, SharedCreepProperties, Store, Structure, StructureObject, Transferable, Withdrawable, action_error_codes::{TransferErrorCode, WithdrawErrorCode}};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::{colony::planning::planned_ref::{ConstructionType, OptionalPlannedStructureRef, PlannedStructureRef, StructureRefReq, PlannedStructureRefs}, memory::Memory, statemachine::StateMachine, tasks::MultiTasksQueue};

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
        providers.extend(
            plan.sources.0.values()
            .filter_map(|source_plan| source_plan.container.resolve())
            .map(|container| (ProviderId::new(container.clone()), container.store()))
        );

        let providers = providers.into_iter()
            .map(|(provider, store)| (provider, store.get_used_capacity(Some(ResourceType::Energy))));

        self.providers.set_tasks(providers);

        let mut consumers = Vec::new();
        consumers.extend(plan.center.spawn.resolve().map(|spawn| (ConsumerId::new(spawn.clone()), spawn.store())));
        consumers.extend(
            plan.center.extensions.iter()
                .filter_map(|extension| extension.resolve())
                .map(|extension| (ConsumerId::new(extension.clone()), extension.store()))
        );
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

impl<T: ProviderReqs + StructureRefReq> OptionalPlannedStructureRef<T> {
    fn resolve_provider(&self) -> Option<(ProviderId, Store)> {
        let structure = self.resolve()?;
        let store = structure.store();
       Some((ProviderId::new(structure), store))
    }
}

impl<T: ProviderReqs + StructureRefReq> PlannedStructureRefs<T> {
    fn resolve_providers(&self) -> Option<(ProviderId, Store)> {
        self.0.iter().cloned().map(Into::into).filter_map(OptionalPlannedStructureRef::resolve_provider)
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

impl<T: ConsumerReqs + StructureRefReq> OptionalPlannedStructureRef<T> {
    fn resolve_consumer(&self) -> Option<(ConsumerId, Store)> {
        let structure = self.resolve()?;
        let store = structure.store();
       Some((ConsumerId::new(structure), store))
    }
}