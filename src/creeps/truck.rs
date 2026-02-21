use itertools::Itertools;
use screeps::{Creep, HasPosition, MaybeHasId, Position, Resource, ResourceType, Room, Ruin, SharedCreepProperties, Structure, Tombstone, find};
use serde::{Deserialize, Serialize};

use crate::{colony::planning::{plan::ColonyPlan, planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}}, creeps::truck::truck_stop::{Consumer, ConsumerStructureReqs, Provider, ProviderStructureReqs, TruckStop}, memory::Memory, messages::TruckMessage, statemachine::{StateMachine, Transition}, tasks::{TaskAmount, TaskServer}};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum TruckCreep {
    #[default] Idle,
    Performing(TruckTask),
    StoringAway,
    FillingUpFor(ConsumerTruckStop)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TruckTask {
    CollectingFrom(ProviderTruckStop),
    ProvidingTo(ConsumerTruckStop)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderTruckStop {
    Ruin(TruckStop<Provider, Ruin>),
    Resource(TruckStop<Provider, Resource>),
    Tombstone(TruckStop<Provider, Tombstone>),
    Structure(TruckStop<Provider, Structure>),
    Creep(TruckStop<Provider, Creep>)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConsumerTruckStop {
    Structure(TruckStop<Consumer, Structure>),
    Creep(TruckStop<Consumer, Creep>)
}

trait GetResourceAvaliable { fn get_resource_avaliable(&self, ty: ResourceType) -> Option<u32>; }
trait Withdraw { fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()>; }
trait Provide: GetResourceAvaliable + Withdraw + HasPosition {}
impl Provide for ProviderTruckStop {}
impl Provide for TruckStop<Provider, Structure> {}
impl Provide for TruckStop<Provider, Creep> {}
impl Provide for TruckStop<Provider, Ruin> {}
impl Provide for TruckStop<Provider, Resource> {}
impl Provide for TruckStop<Provider, Tombstone> {}

trait GetResourceFree { fn get_resource_free(&self, ty: ResourceType) -> Option<u32>; }
trait Transfer { fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()>; }
trait Consume: GetResourceFree + GetResourceAvaliable + Transfer + HasPosition {}
impl Consume for ConsumerTruckStop {}
impl Consume for TruckStop<Consumer, Structure> {}
impl Consume for TruckStop<Consumer, Creep> {}

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

                if !coordinator.heartbeat(creep, task) { return Ok(Continue(Self::Idle)) }

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

                if !coordinator.consumers.heartbeat_task(creep, consumer) { return Ok(Continue(Self::Idle)) }

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
                provider.creep_withdraw(creep, ResourceType::Energy),
            TruckTask::ProvidingTo(consumer) => 
                consumer.creep_transfer(creep, ResourceType::Energy)
        }
    }

    fn still_valid(&self) -> bool {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.get_resource_avaliable(ResourceType::Energy).is_some_and(|amount| amount > 0),
            TruckTask::ProvidingTo(consumer) =>
                consumer.get_resource_free(ResourceType::Energy).is_some_and(|amount| amount > 0)
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TruckCoordinator {
    providers: TaskServer<ProviderTruckStop, ProviderTaskData>,
    consumers: TaskServer<ConsumerTruckStop, u32>
}

impl TruckCoordinator {
    pub fn update(&mut self, plan: &ColonyPlan, room: &Room, messages: Vec<TruckMessage>) {
        self.consumers.handle_timeouts();
        self.providers.handle_timeouts();

        let messages = messages.into_iter()
            .filter(|message| *message.room_name() == room.name())
            .collect_vec();

        let mut providers = Vec::new();
        providers.extend(room.find(find::DROPPED_RESOURCES, None).providers().tasks(7, Some(0), None));
        providers.extend(messages.providers().tasks(6, Some(0),  None));
        providers.extend(room.find(find::TOMBSTONES, None).providers().tasks(5, None, None));
        providers.extend(room.find(find::RUINS, None).providers().tasks(4, None, None));
        providers.extend(plan.center.link.providers().tasks(3, Some(800), None));
        providers.extend(plan.sources.source_containers.providers().tasks(2, Some(1500), None));
        providers.extend(plan.center.terminal.providers().tasks(1, None, Some(10_000)));
        self.providers.set_tasks(providers);

        let mut consumers = Vec::new();
        consumers.extend(plan.center.spawn.consumers().tasks(5, None));
        consumers.extend(plan.center.extensions.consumers().tasks(4, None));
        consumers.extend(plan.center.towers.consumers().tasks(3, None));
        consumers.extend(messages.consumers().tasks(2, None));
        consumers.extend(plan.center.terminal.consumers().tasks(1, Some(2_000)));
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

    fn assign_push_provider(&mut self, creep: &Creep) -> Option<ProviderTruckStop> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .filter(|(_, amount, data)| data.push_amount.is_some_and(|push_amount| *amount >= push_amount))
                .max_by_key(|(_, amount, data)| (data.priority, *amount))
        })
    }

    fn assign_provider(&mut self, creep: &Creep) -> Option<ProviderTruckStop> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .max_by_key(|(_, amount, data)| ((*amount).min(creep_capacity), data.priority))
        })
    }

    fn assign_consumer(&mut self, creep: &Creep) -> Option<ConsumerTruckStop> {
        let creep_energy = creep.store().get_used_capacity(Some(ResourceType::Energy));
        self.consumers.assign_task(creep, creep_energy, |tasks| {
            tasks.into_iter()
                .max_set_by_key(|(_, _, priority)| *priority)
                .into_iter()
                .min_by_key(|(consumer, _, _)| consumer.pos().get_range_to(creep.pos()))
        })
    }
}

impl ProviderTruckStop {
    fn get_provide(&self) -> &dyn Provide {
        match self {
            ProviderTruckStop::Ruin(truck_stop) => truck_stop,
            ProviderTruckStop::Resource(truck_stop) => truck_stop,
            ProviderTruckStop::Tombstone(truck_stop) => truck_stop,
            ProviderTruckStop::Structure(truck_stop) => truck_stop,
            ProviderTruckStop::Creep(truck_stop) => truck_stop
        }
    }
}

impl GetResourceAvaliable for ProviderTruckStop {
    fn get_resource_avaliable(&self, ty: ResourceType) -> Option<u32> {
        self.get_provide().get_resource_avaliable(ty)
    }
}

impl Withdraw for ProviderTruckStop {
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
        self.get_provide().creep_withdraw(creep, ty)
    }
}

impl HasPosition for ProviderTruckStop {
    #[doc = " Position of the object."]
    fn pos(&self) -> Position {
        self.get_provide().pos()
    }
}

impl ConsumerTruckStop {
    fn get_consume(&self) -> &dyn Consume {
        match self {
            ConsumerTruckStop::Structure(truck_stop) => truck_stop,
            ConsumerTruckStop::Creep(truck_stop) => truck_stop
        }
    }
}

impl GetResourceAvaliable for ConsumerTruckStop {
    fn get_resource_avaliable(&self, ty: ResourceType) -> Option<u32> {
        self.get_consume().get_resource_avaliable(ty)
    }
}

impl GetResourceFree for ConsumerTruckStop {
    fn get_resource_free(&self, ty: ResourceType) -> Option<u32> {
        self.get_consume().get_resource_free(ty)
    }
}

impl Transfer for ConsumerTruckStop {
    fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
        self.get_consume().creep_transfer(creep, ty)
    }
}

impl HasPosition for ConsumerTruckStop {
    #[doc = " Position of the object."]
    fn pos(&self) -> Position {
        self.get_consume().pos()
    }
}

trait IntoConsumers { fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop>; }
impl<R, S: ProviderStructureReqs> IntoConsumers for R where R : ResolvableStructureRef<Structure = S> {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.resolve().map(TruckStop::<Consumer, Structure>::new).map(ConsumerTruckStop::Structure)
    }
}

impl<S: ConsumerStructureReqs + StructureRefReq> IntoConsumers for PlannedStructureRefs<S> {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.resolve().into_iter().map(TruckStop::<Consumer, Structure>::new).map(ConsumerTruckStop::Structure)
    }
}

impl IntoConsumers for Vec<TruckMessage> {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.iter().filter_map(|message| {
            let TruckMessage::Consumer(id, pos, _) = message else { return None };
            Some(ConsumerTruckStop::Creep(TruckStop::<Consumer, Creep>::new(*id, *pos)))
        })
    }
}

trait IntoProviders { fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop>; }
impl<R, S: ProviderStructureReqs> IntoProviders for R where R : ResolvableStructureRef<Structure = S> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.resolve().map(TruckStop::<Provider, Structure>::new).map(ProviderTruckStop::Structure)
    }
}

impl<S: ProviderStructureReqs + StructureRefReq> IntoProviders for PlannedStructureRefs<S> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.resolve().into_iter().map(TruckStop::<Provider, Structure>::new).map(ProviderTruckStop::Structure)
    }
}

impl IntoProviders for Vec<TruckMessage> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.iter().filter_map(|message| {
            let TruckMessage::Provider(id, pos, _) = message else { return None };
            Some(ProviderTruckStop::Creep(TruckStop::<Provider, Creep>::new(*id, *pos)))
        })
    }
}

impl IntoProviders for Vec<Resource> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.iter().map(TruckStop::<Provider, Resource>::new).map(ProviderTruckStop::Resource)
    }
}

impl IntoProviders for Vec<Ruin> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.iter().map(TruckStop::<Provider, Ruin>::new).map(ProviderTruckStop::Ruin)
    }
}

impl IntoProviders for Vec<Tombstone> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.iter().map(TruckStop::<Provider, Tombstone>::new).map(ProviderTruckStop::Tombstone)
    }
}

#[derive(Serialize, Deserialize)]
pub struct ProviderTaskData {
    pub priority: u32,
    pub push_amount: Option<u32>
}

pub trait CreateProviderTasks { 
    fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (ProviderTruckStop, TaskAmount, ProviderTaskData)>; 
}

impl<I : IntoIterator<Item = ProviderTruckStop>> CreateProviderTasks for I {
    fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (ProviderTruckStop, TaskAmount, ProviderTaskData)> {
        self.into_iter().filter_map(move |provider| {
            let provide = provider.get_resource_avaliable(ResourceType::Energy)?.saturating_sub(min_leave.unwrap_or(0));

            Some((provider, provide, ProviderTaskData { priority, push_amount }))
        })
    }
}

pub trait CreateConsumerTasks { 
    fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (ConsumerTruckStop, TaskAmount, u32)>; 
}

impl<I : IntoIterator<Item = ConsumerTruckStop>> CreateConsumerTasks for I {
    fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (ConsumerTruckStop, TaskAmount, u32)> {
        self.into_iter().filter_map(move |consumer| {
            let used = consumer.get_resource_avaliable(ResourceType::Energy)?;
            let capacity_left = consumer.get_resource_free(ResourceType::Energy)?;
            let consume = max_fill.map_or(capacity_left, |max_fill| max_fill.saturating_sub(used));

            Some((consumer, consume, priority))
        })
    }
}


mod truck_stop {
    use std::{hash::Hash, marker::PhantomData};

    use screeps::{Creep, HasId, HasPosition, HasStore, ObjectId, Position, Resource, ResourceType, Ruin, SharedCreepProperties, Store, Structure, StructureObject, Tombstone, Withdrawable};
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::JsCast;

    use crate::{creeps::truck::{GetResourceAvaliable, GetResourceFree, Transfer, Withdraw}};

    pub trait TruckStopType {}

    #[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Consumer { }
    impl TruckStopType for Consumer {}

    #[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Provider { }
    impl TruckStopType for Provider {}

    pub trait OtherEntity: JsCast + HasId + HasPosition {}
    impl OtherEntity for Ruin {}
    impl OtherEntity for Resource {}
    impl OtherEntity for Tombstone {}

    pub trait NormalOtherEntity: OtherEntity + HasStore + Withdrawable {}
    impl NormalOtherEntity for Ruin {}
    impl NormalOtherEntity for Tombstone {}

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(bound = "")]
    pub struct TruckStop<T, I> {
        id: ObjectId<I>,
        pos: Position,
        phantom: PhantomData<T>
    }

    impl<T, I> Hash for TruckStop<T, I> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);

        }
    }

    impl<T, I> Eq for TruckStop<T, I> { }
    impl<T, I> PartialEq for TruckStop<T, I> {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }

    impl<T: TruckStopType, I> HasPosition for TruckStop<T, I> {
        #[doc = " Position of the object."]
        fn pos(&self) -> Position {
            self.pos
        }
    }

    impl<T : TruckStopType> TruckStop<T, Structure> {
        fn from_structure<S: Into<Structure>>(structure: S) -> Self {
            let structure = structure.into();
            Self { id: structure.id(), pos: structure.pos(), phantom: PhantomData }
        }
    }

    pub trait ProviderStructureReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Provider, Structure> {
        pub fn new<S: ProviderStructureReqs>(structure: S) -> Self {
            Self::from_structure(structure)
        }
    }
    pub trait ConsumerStructureReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Consumer, Structure> {
        pub fn new<S: ConsumerStructureReqs>(structure: S) -> Self {
            Self::from_structure(structure)
        }
    }

    impl<I : OtherEntity> TruckStop<Provider, I> {
        pub fn new(entity: &I) -> Self {
            Self { 
                id: entity.id(), 
                pos: entity.pos(), 
                phantom: PhantomData 
            }
        }
    }

    impl<T : TruckStopType> TruckStop<T, Creep> {
        pub fn new(id: ObjectId<Creep>, pos: Position) -> Self {
            Self { id, pos, phantom: PhantomData }
        }
    }


    trait ResolveStore { fn resolve_store(&self) -> Option<Store>; }
    impl<T: TruckStopType> ResolveStore for TruckStop<T, Structure> {
        fn resolve_store(&self) -> Option<Store> {
            StructureObject::from(self.id.resolve()?).as_has_store().map(HasStore::store)
        }
    }

    impl<T: TruckStopType> ResolveStore for TruckStop<T, Creep> {
        fn resolve_store(&self) -> Option<Store> {
            self.id.resolve().map(|creep| creep.store())
        }
    }

    impl<T: TruckStopType, I : NormalOtherEntity> ResolveStore for TruckStop<T, I> {
        fn resolve_store(&self) -> Option<Store> {
            Some(self.id.resolve()?.store())
        }
    }

    impl<I> GetResourceAvaliable for TruckStop<Provider, I> where Self : ResolveStore {
        fn get_resource_avaliable(&self, ty: ResourceType) -> Option<u32> {
            Some(self.resolve_store()?.get_used_capacity(Some(ty)))
        }
    }

    impl GetResourceAvaliable for TruckStop<Provider, Resource> {
        fn get_resource_avaliable(&self, ty: ResourceType) -> Option<u32> {
            let resource= self.id.resolve()?;
            if resource.resource_type() == ty {
                Some(resource.amount())
            } else {
                Some(0)
            }
        }
    }

    impl<I> GetResourceAvaliable for TruckStop<Consumer, I> where Self : ResolveStore {
        fn get_resource_avaliable(&self, ty: ResourceType) -> Option<u32> {
            Some(self.resolve_store()?.get_used_capacity(Some(ty)))
        }
    }

    impl<I> GetResourceFree for TruckStop<Consumer, I> where Self : ResolveStore {
        fn get_resource_free(&self, ty: ResourceType) -> Option<u32> {
            Some(self.resolve_store()?.get_free_capacity(Some(ty)) as u32)
        }
    }

    impl Transfer for TruckStop<Consumer, Structure> {
        fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
            let structure = StructureObject::from(self.id.resolve().ok_or(())?);
            creep.transfer(structure.as_transferable().ok_or(())?, ty, None).map_err(|_| ())
        }
    }

    impl Transfer for TruckStop<Consumer, Creep> {
        fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
            let other_creep = self.id.resolve().ok_or(())?;
            creep.transfer(&other_creep, ty, None).map_err(|_| ())
        }
    }

    impl Withdraw for TruckStop<Provider, Structure> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
            let structure = StructureObject::from(self.id.resolve().ok_or(())?);
            creep.withdraw(structure.as_withdrawable().ok_or(())?, ty, None).map_err(|_| ())
        }
    }

    impl Withdraw for TruckStop<Provider, Creep> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
            let other_creep = self.id.resolve().ok_or(())?;
            other_creep.transfer(creep, ty, None).map_err(|_| ())
        }
    }

    impl<I : NormalOtherEntity> Withdraw for TruckStop<Provider, I> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
            creep.withdraw(&self.id.resolve().ok_or(())?, ty, None).map_err(|_| ())
        }
    }

    impl Withdraw for TruckStop<Provider, Resource> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> Result<(), ()> {
            let resource = self.id.resolve().ok_or(())?;
            if resource.resource_type() != ty { return Err(()) }
            creep.pickup(&resource).map_err(|_| ())
        }
    }
}