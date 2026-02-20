use itertools::Itertools;
use screeps::{Creep, HasPosition, MaybeHasId, Position, ResourceType, SharedCreepProperties};
use serde::{Deserialize, Serialize};

use crate::{colony::planning::plan::ColonyPlan, creeps::truck::truck_stop::{Consumer, ProviderData, ConsumerTasks, Provider, ProviderTasks, ResolveConsumer, ResolveProvider, TruckStop}, memory::Memory, statemachine::{StateMachine, Transition}, tasks::TaskServer};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum TruckCreep {
    #[default] Idle,
    Performing(TruckTask),
    StoringAway,
    FillingUpFor(TruckStop<Consumer>)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TruckTask {
    CollectingFrom(TruckStop<Provider>),
    ProvidingTo(TruckStop<Consumer>)
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
            TruckTask::ProvidingTo(consumer) => consumer.pos(),
        }
    }

    fn perform(&self, creep: &Creep) -> Result<(), ()> {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.withdraw(creep, ResourceType::Energy, None),
            TruckTask::ProvidingTo(consumer) => 
                consumer.transfer(creep, ResourceType::Energy, None),
        }
    }

    fn still_valid(&self) -> bool {
        match self {
            TruckTask::CollectingFrom(provider) => provider.resolve_store()
                .is_some_and(|store| store.get_used_capacity(Some(ResourceType::Energy)) > 0),
            TruckTask::ProvidingTo(consumer) => consumer.resolve_store()
                .is_some_and(|store| store.get_free_capacity(Some(ResourceType::Energy)) > 0),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TruckTaskCoordinator {
    providers: TaskServer<TruckStop<Provider>, ProviderData>,
    consumers: TaskServer<TruckStop<Consumer>, u32>
}

impl TruckTaskCoordinator {
    pub fn update(&mut self, plan: &ColonyPlan) {
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
            TruckTask::ProvidingTo(task) => self.consumers.heartbeat_task(creep, task),
        }
    }

    fn finish(&mut self, creep: &Creep, task: &TruckTask, success: bool) {
        match task {
            TruckTask::CollectingFrom(task) => 
                self.providers.finish_task(creep.try_id().unwrap(), task, success),
            TruckTask::ProvidingTo(task) => 
                self.consumers.finish_task(creep.try_id().unwrap(), task, success),
        }
    }

    fn assign_push_provider(&mut self, creep: &Creep) -> Option<TruckStop<Provider>> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .filter(|(_, amount, data)| data.push_amount.is_some_and(|push_amount| *amount >= push_amount))
                .max_by_key(|(_, amount, data)| (data.priority, *amount))
                .map(|(provider, _, _)| provider)
        })
    }

    fn assign_provider(&mut self, creep: &Creep) -> Option<TruckStop<Provider>> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .max_by_key(|(_, amount, data)| ((*amount).min(creep_capacity), data.priority))
                .map(|(provider, _, _)| provider)
        })
    }

    fn assign_consumer(&mut self, creep: &Creep) -> Option<TruckStop<Consumer>> {
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

    use screeps::{Creep, HasId, HasPosition, HasStore, ObjectId, Position, ResourceType, SharedCreepProperties, Store, Structure, StructureObject, Withdrawable, action_error_codes::{TransferErrorCode, WithdrawErrorCode}};
    use serde::{Deserialize, Serialize};

    use crate::{colony::planning::planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}, tasks::TaskAmount};
    
    pub trait TruckStopType {}

    #[derive(Serialize, Deserialize, Debug, Clone, Eq)]
    pub struct TruckStop<T> {
        id: ObjectId<Structure>,
        pos: Position,
        phantom: PhantomData<T>
    }

    impl<T> Hash for TruckStop<T> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);

        }
    }

    impl<T> PartialEq for TruckStop<T> {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Consumer { }
    impl TruckStopType for Consumer {}

    #[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Provider { }
    impl TruckStopType for Provider {}

    impl<T: TruckStopType> TruckStop<T> {
        fn internal_new<S: Into<Structure>>(structure: S) -> Self {
            let structure = structure.into();

            Self {
                id: structure.id(),
                pos: structure.pos(),
                phantom: PhantomData
            }
        }

        pub fn resolve_store(&self) -> Option<Store> {
            StructureObject::from(self.id.resolve()?).as_has_store().map(HasStore::store)
        }
    }

    impl<T: TruckStopType> HasPosition for TruckStop<T> {
        #[doc = " Position of the object."]
        fn pos(&self) -> Position {
            self.pos
        }
    }

    pub trait ProviderReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Provider> {
        pub fn new<S: ProviderReqs>(structure: S) -> Self {
            Self::internal_new(structure)
        }

        pub fn withdraw(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
            let structure = StructureObject::from(self.id.resolve().ok_or(())?);
            let withdraw_result = creep.withdraw(structure.as_withdrawable().unwrap(), ty, amount);

            if matches!(withdraw_result, Ok(()) | Err(WithdrawErrorCode::Full)) { Ok(()) } else { Err(()) }
        }
    }

    pub trait ConsumerReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Consumer> {
        pub fn new<S: ConsumerReqs>(structure: S) -> Self {
            Self::internal_new(structure)
        }

        pub fn transfer(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
            let structure = StructureObject::from(self.id.resolve().ok_or(())?);
            let withdraw_result = creep.transfer(structure.as_transferable().unwrap(), ty, amount);

            if matches!(withdraw_result, Ok(()) | Err(TransferErrorCode::Full)) { Ok(()) } else { Err(()) }
        }
    }

    pub trait ResolveProvider { fn resolve_provider(&self) -> Option<TruckStop<Provider>>; }
    impl<R, S: ProviderReqs> ResolveProvider for R where R : ResolvableStructureRef<Structure = S> {
        fn resolve_provider(&self) -> Option<TruckStop<Provider>> {
            self.resolve().map(TruckStop::<Provider>::new)
        }
    }

    pub trait ResolveConsumer { fn resolve_consumer(&self) -> Option<TruckStop<Consumer>>; }
    impl<R, S: ProviderReqs> ResolveConsumer for R where R : ResolvableStructureRef<Structure = S> {
        fn resolve_consumer(&self) -> Option<TruckStop<Consumer>> {
            self.resolve().map(TruckStop::<Consumer>::new)
        }
    }

    impl<S: ProviderReqs + StructureRefReq> PlannedStructureRefs<S> {
        pub fn resolve_providers(&self) -> impl Iterator<Item = TruckStop<Provider>> {
            self.resolve().into_iter().map(TruckStop::<Provider>::new)
        }
    }

    impl<S: ConsumerReqs + StructureRefReq> PlannedStructureRefs<S> {
        pub fn resolve_consumers(&self) -> impl Iterator<Item = TruckStop<Consumer>> {
            self.resolve().into_iter().map(TruckStop::<Consumer>::new)
        }
    }

    #[derive(Serialize, Deserialize)]
    pub struct ProviderData {
        pub priority: u32,
        pub push_amount: Option<u32>
    }

    pub trait ProviderTasks { 
        fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (TruckStop<Provider>, TaskAmount, ProviderData)>; 
    }

    impl<I : IntoIterator<Item = TruckStop<Provider>>> ProviderTasks for I {
        fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (TruckStop<Provider>, TaskAmount, ProviderData)> {
            self.into_iter().filter_map(move |provider| {
                let store = provider.resolve_store()?;
                let provide = store.get_used_capacity(Some(ResourceType::Energy)).saturating_sub(min_leave.unwrap_or(0));

                Some((provider, provide, ProviderData { priority, push_amount }))
            })
        }
    }

    pub trait ConsumerTasks { 
        fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (TruckStop<Consumer>, TaskAmount, u32)>; 
    }

    impl<I : IntoIterator<Item = TruckStop<Consumer>>> ConsumerTasks for I {
        fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (TruckStop<Consumer>, TaskAmount, u32)> {
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