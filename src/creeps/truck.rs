use itertools::Itertools;
use screeps::{Creep, HasPosition, MaybeHasId, Position, Resource, ResourceType, Room, Ruin, SharedCreepProperties, Structure, Tombstone};
use serde::{Deserialize, Serialize};

use crate::{colony::planning::plan::ColonyPlan, creeps::truck::truck_stop::{Consumer, ConsumerTasks, GetResourceUsed, Provider, ProviderData, ProviderTasks, ResolveConsumer, ResolveProvider, TruckStop}, memory::Memory, statemachine::{StateMachine, Transition}, tasks::TaskServer};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum TruckCreep {
    #[default] Idle,
    Performing(TruckTask),
    StoringAway,
    FillingUpFor(TruckStop<Structure, Consumer>)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TruckTask {
    CollectingFrom(ProviderType),
    ProvidingTo(TruckStop<Structure, Consumer>)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderType {
    Ruin(TruckStop<Ruin, Provider>),
    Resource(TruckStop<Resource, Provider>),
    Tombstone(TruckStop<Tombstone, Provider>),
    Structure(TruckStop<Structure, Provider>)
}

impl StateMachine<Creep> for TruckCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Transition<Self>, ()> {
        use Transition::*;

        let buffer = mem.creep_home(creep).ok_or(())?.buffer().ok_or(())?;
        let coordinator = mem.truck_coordinators.entry(mem.creep(creep).unwrap().home).or_default();

        let buffer_energy = buffer.store().get_used_capacity(Some(ResourceType::Energy));
        let buffer_free_capacity = buffer.store().get_free_capacity(Some(ResourceType::Energy));

        match self {
            Self::Idle => {
                if creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                    let consumer = coordinator.assign_consumer(creep);
                    if let Some(consumer) = consumer { return Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer)))) }

                    if buffer_free_capacity >= 0 { return Ok(Continue(Self::StoringAway)) }
                } else {
                    let push_provider = coordinator.assign_push_provider(creep);
                    if let Some(provider) = push_provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }

                    if buffer_energy > 0 {
                        let consumer = coordinator.assign_consumer(creep);
                        if let Some(consumer) = consumer { return Ok(Continue(Self::FillingUpFor(consumer))) }
                    }

                    let provider = coordinator.assign_provider(creep);
                    if let Some(provider) = provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }
                }

                Ok(Stay)
            },
            Self::Performing(task) => {
                if !task.still_valid() {
                    coordinator.finish(creep, task, false);
                    return Ok(Continue(Self::Idle))
                }

                coordinator.heartbeat(creep, task);

                if creep.pos().is_near_to(task.pos()) {
                    task.perform(creep)?;
                    coordinator.finish(creep, task, true);
                    Ok(Break(Self::Idle))
                } else {
                    mem.movement.smart_move_creep_to(creep, task.pos()).ok();
                    Ok(Stay)
                }
            },
            Self::FillingUpFor(consumer) => {
                if buffer_energy == 0 {
                    coordinator.consumers.finish_task(creep.try_id().unwrap(), consumer, false);
                    return Ok(Continue(Self::Idle))
                }

                coordinator.consumers.heartbeat_task(creep, consumer);

                if creep.pos().is_near_to(buffer.pos()) {
                    creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None).ok();
                    Ok(Break(Self::Performing(TruckTask::ProvidingTo(consumer.clone()))))
                } else {
                    mem.movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(Stay)
                }
            },
            Self::StoringAway => {
                if creep.pos().is_near_to(buffer.pos()) {
                    creep.transfer(buffer.transferable(), ResourceType::Energy, None).ok();
                    Ok(Break(Self::Idle))
                } else {
                    mem.movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(Stay)
                }
            },
        }
    }
}

impl TruckTask {
    fn pos(&self) -> Position {
        match self {
            TruckTask::CollectingFrom(provider) => provider.pos(),
            TruckTask::ProvidingTo(consumer) => consumer.pos()
        }
    }

    fn perform(&self, creep: &Creep) -> Result<(), ()> {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.withdraw(creep, ResourceType::Energy, None),
            TruckTask::ProvidingTo(consumer) => 
                consumer.transfer(creep, ResourceType::Energy, None)
        }   
    }

    fn still_valid(&self) -> bool {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.get_resource_used(ResourceType::Energy).is_some_and(|amount| amount > 0),
            TruckTask::ProvidingTo(consumer) =>
                consumer.get_resource_free(ResourceType::Energy).is_some_and(|amount| amount > 0)
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TruckTaskCoordinator {
    providers: TaskServer<ProviderType, ProviderData>,
    consumers: TaskServer<TruckStop<Structure, Consumer>, u32>
}

impl TruckTaskCoordinator {
    pub fn update(&mut self, plan: &ColonyPlan, room: Room) {
        self.consumers.handle_timeouts();
        self.providers.handle_timeouts();

        let mut providers = Vec::new();
        providers.extend(plan.center.link.resolve_provider().tasks(3, Some(800), None));
        providers.extend(plan.sources.source_containers.resolve_providers().tasks(2, Some(1500), None));
        providers.extend(plan.center.terminal.resolve_provider().tasks(1, None, Some(10_000)));
        self.providers.set_tasks(providers);

        let mut consumers = Vec::new();
        consumers.extend(plan.center.spawn.resolve_consumer().tasks(4, None));
        consumers.extend(plan.center.extensions.resolve_consumers().tasks(3, None));
        consumers.extend(plan.center.towers.resolve_consumers().tasks(2, None));
        consumers.extend(plan.center.terminal.resolve_consumer().tasks(1, Some(2_000)));
        self.consumers.set_tasks(consumers);
    }

    fn heartbeat(&mut self, creep: &Creep, task: &TruckTask) -> bool {
        match task {
            TruckTask::CollectingFrom(task) => self.providers.heartbeat_task(creep, task),
            TruckTask::ProvidingTo(task) => self.consumers.heartbeat_task(creep, task)
        }
    }

    fn finish(&mut self, creep: &Creep, task: &TruckTask, success: bool) {
        match task {
            TruckTask::CollectingFrom(task) => 
                self.providers.finish_task(creep.try_id().unwrap(), task, success),
            TruckTask::ProvidingTo(task) => 
                self.consumers.finish_task(creep.try_id().unwrap(), task, success)
        }
    }

    fn assign_push_provider(&mut self, creep: &Creep) -> Option<ProviderType> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .filter(|(_, amount, data)| data.push_amount.is_some_and(|push_amount| *amount >= push_amount))
                .max_by_key(|(_, amount, data)| (data.priority, *amount))
                .map(|(provider, _, _)| provider)
        })
    }

    fn assign_provider(&mut self, creep: &Creep) -> Option<ProviderType> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .max_by_key(|(_, amount, data)| ((*amount).min(creep_capacity), data.priority))
                .map(|(provider, _, _)| provider)
        })
    }

    fn assign_consumer(&mut self, creep: &Creep) -> Option<TruckStop<Structure, Consumer>> {
        let creep_energy = creep.store().get_used_capacity(Some(ResourceType::Energy));
        self.consumers.assign_task(creep, creep_energy, |tasks| {
            tasks.into_iter()
                .max_set_by_key(|(_, _, priority)| *priority)
                .into_iter()
                .map(|(consumer, _, _)| consumer)
                .min_by_key(|consumer| consumer.pos().get_range_to(creep.pos()))
        })
    }
}


mod truck_stop {
    use std::{hash::Hash, marker::PhantomData};

    use screeps::{Creep, HasId, HasPosition, HasStore, MaybeHasId, ObjectId, Position, Resource, ResourceType, Ruin, SharedCreepProperties, Store, Structure, StructureObject, Tombstone, Withdrawable, action_error_codes::{TransferErrorCode, WithdrawErrorCode}};
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::JsCast;

    use crate::{colony::planning::planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}, creeps::truck::ProviderType, tasks::TaskAmount};

    pub trait TruckStopType {}

    #[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Consumer { }
    impl TruckStopType for Consumer {}

    #[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Provider { }
    impl TruckStopType for Provider {}

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(bound = "")]
    pub struct TruckStop<I, T> {
        id: ObjectId<I>,
        pos: Position,
        phantom: PhantomData<T>
    }

    impl<I, T> Hash for TruckStop<I, T> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);

        }
    }

    impl<I, T> Eq for TruckStop<I, T> { }
    impl<I, T> PartialEq for TruckStop<I, T> {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }

    impl<T : TruckStopType> TruckStop<Structure, T> {
        fn from_structure<S: Into<Structure>>(structure: S) -> Self {
            let structure = structure.into();
            Self { id: structure.id(), pos: structure.pos(), phantom: PhantomData }
        }
    }

    pub trait ResolveStore { fn resolve_store(&self) -> Option<Store>; }
    impl<T: TruckStopType> ResolveStore for TruckStop<Structure, T> {
        fn resolve_store(&self) -> Option<Store> {
            StructureObject::from(self.id.resolve()?).as_has_store().map(HasStore::store)
        }
    }

    pub trait EntityWithStore: HasStore + JsCast + MaybeHasId {  }
    impl EntityWithStore for Tombstone {}
    impl EntityWithStore for Ruin {}
    impl<T: TruckStopType, I : EntityWithStore> ResolveStore for TruckStop<I, T> {
        fn resolve_store(&self) -> Option<Store> {
            Some(self.id.resolve()?.store())
        }
    }

    pub trait GetResourceUsed { fn get_resource_used(&self, ty: ResourceType) -> Option<u32>; }
    impl<I> GetResourceUsed for TruckStop<I, Provider> where Self : ResolveStore {
        fn get_resource_used(&self, ty: ResourceType) -> Option<u32> {
            Some(self.resolve_store()?.get_used_capacity(Some(ty)))
        }
    }

    impl GetResourceUsed for TruckStop<Resource, Provider> {
        fn get_resource_used(&self, ty: ResourceType) -> Option<u32> {
            let resource= self.id.resolve()?;
            if resource.resource_type() == ty {
                Some(resource.amount())
            } else {
                Some(0)
            }
        }
    }

    impl<I> TruckStop<I, Consumer> where Self : ResolveStore {
        pub fn get_resource_free(&self, ty: ResourceType) -> Option<u32> {
            Some(self.resolve_store()?.get_free_capacity(Some(ty)) as u32)
        }
    }

    impl<T: TruckStopType, I> HasPosition for TruckStop<I, T> {
        #[doc = " Position of the object."]
        fn pos(&self) -> Position {
            self.pos
        }
    }

    pub trait ProviderReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Structure, Provider> {
        pub fn new<S: ProviderReqs>(structure: S) -> Self {
            Self::from_structure(structure)
        }

        pub fn withdraw(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
            let structure = StructureObject::from(self.id.resolve().ok_or(())?);
            let withdraw_result = creep.withdraw(structure.as_withdrawable().unwrap(), ty, amount);

            if matches!(withdraw_result, Ok(()) | Err(WithdrawErrorCode::Full)) { Ok(()) } else { Err(()) }
        }
    }

    pub trait ConsumerReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Structure, Consumer> {
        pub fn new<S: ConsumerReqs>(structure: S) -> Self {
            Self::from_structure(structure)
        }

        pub fn transfer(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
            let structure = StructureObject::from(self.id.resolve().ok_or(())?);
            let withdraw_result = creep.transfer(structure.as_transferable().unwrap(), ty, amount);

            if matches!(withdraw_result, Ok(()) | Err(TransferErrorCode::Full)) { Ok(()) } else { Err(()) }
        }
    }

    pub trait ResolveProvider { fn resolve_provider(&self) -> Option<ProviderType>; }
    impl<R, S: ProviderReqs> ResolveProvider for R where R : ResolvableStructureRef<Structure = S> {
        fn resolve_provider(&self) -> Option<ProviderType> {
            self.resolve().map(TruckStop::<Structure, Provider>::new).map(ProviderType::Structure)
        }
    }

    pub trait ResolveConsumer { fn resolve_consumer(&self) -> Option<TruckStop<Structure, Consumer>>; }
    impl<R, S: ProviderReqs> ResolveConsumer for R where R : ResolvableStructureRef<Structure = S> {
        fn resolve_consumer(&self) -> Option<TruckStop<Structure, Consumer>> {
            self.resolve().map(TruckStop::<Structure, Consumer>::new)
        }
    }

    impl<S: ProviderReqs + StructureRefReq> PlannedStructureRefs<S> {
        pub fn resolve_providers(&self) -> impl Iterator<Item = ProviderType> {
            self.resolve().into_iter().map(TruckStop::<Structure, Provider>::new).map(ProviderType::Structure)
        }
    }

    impl<S: ConsumerReqs + StructureRefReq> PlannedStructureRefs<S> {
        pub fn resolve_consumers(&self) -> impl Iterator<Item = TruckStop<Structure, Consumer>> {
            self.resolve().into_iter().map(TruckStop::<Structure, Consumer>::new)
        }
    }

    #[derive(Serialize, Deserialize)]
    pub struct ProviderData {
        pub priority: u32,
        pub push_amount: Option<u32>
    }

    pub trait ProviderTasks { 
        fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (ProviderType, TaskAmount, ProviderData)>; 
    }

    impl<I : IntoIterator<Item = ProviderType>> ProviderTasks for I {
        fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (ProviderType, TaskAmount, ProviderData)> {
            self.into_iter().filter_map(move |provider| {
                let provide = provider.get_resource_used(ResourceType::Energy)?.saturating_sub(min_leave.unwrap_or(0));

                Some((provider, provide, ProviderData { priority, push_amount }))
            })
        }
    }

    pub trait ConsumerTasks { 
        fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (TruckStop<Structure, Consumer>, TaskAmount, u32)>; 
    }

    impl<I : IntoIterator<Item = TruckStop<Structure, Consumer>>> ConsumerTasks for I {
        fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (TruckStop<Structure, Consumer>, TaskAmount, u32)> {
            self.into_iter().filter_map(move |consumer| {
                let store = consumer.resolve_store()?;
                let capacity = max_fill.unwrap_or(store.get_capacity(Some(ResourceType::Energy)));
                let used = store.get_used_capacity(Some(ResourceType::Energy));
                let consume = capacity.saturating_sub(used);

                Some((consumer, consume, priority))
            })
        }
    }
}