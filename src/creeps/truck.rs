use std::cmp::Reverse;

use anyhow::anyhow;
use enum_display::EnumDisplay;
use itertools::Itertools;
use screeps::{Creep, HasPosition, MaybeHasId, Position, Resource, ResourceType, Room, Ruin, SharedCreepProperties, Structure, Tombstone, find};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{colony::{ColonyView, planning::{plan::ColonyPlan, planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}}}, creeps::truck::truck_stop::{Consumer, ConsumerStructureReqs, Provider, ProviderStructureReqs, TruckStop, TruckStopPos}, id::{IDMode, Resolved}, messages::{CreepMessage, Messages, TruckMessage}, movement::Movement, statemachine::{StateMachine, Transition}, tasks::{TaskAmount, TaskServer}};

#[derive(Serialize, Deserialize, Debug, Clone, Default, EnumDisplay)]
pub enum TruckCreep<M: IDMode> {
    #[default] Idle,
    Performing(TruckTask<M>),
    StoringAway,
    FillingUpFor(ConsumerTruckStop<M>)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TruckTask<M: IDMode> {
    CollectingFrom(ProviderTruckStop<M>),
    ProvidingTo(ConsumerTruckStop<M>)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderTruckStop<M: IDMode> {
    Ruin(TruckStop<M, Provider, Ruin>),
    Resource(TruckStop<M, Provider, Resource>),
    Tombstone(TruckStop<M, Provider, Tombstone>),
    Structure(TruckStop<M, Provider, Structure>),
    Creep(TruckStop<M, Provider, Creep>)
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConsumerTruckStop<M: IDMode> {
    Structure(TruckStop<M, Consumer, Structure>),
    Creep(TruckStop<M, Consumer, Creep>)
}

trait GetResourceAvaliable { fn get_resource_avaliable(&self, ty: ResourceType) -> u32; }
trait Withdraw { fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()>; }
trait Provide: GetResourceAvaliable + Withdraw + TruckStopPos {}
impl Provide for ProviderTruckStop<Resolved> {}
impl Provide for TruckStop<Resolved, Provider, Structure> {}
impl Provide for TruckStop<Resolved, Provider, Creep> {}
impl Provide for TruckStop<Resolved, Provider, Ruin> {}
impl Provide for TruckStop<Resolved, Provider, Resource> {}
impl Provide for TruckStop<Resolved, Provider, Tombstone> {}

trait GetResourceFree { fn get_resource_free(&self, ty: ResourceType) -> u32; }
trait Transfer { fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()>; }
trait Consume: GetResourceFree + GetResourceAvaliable + Transfer + TruckStopPos {}
impl Consume for ConsumerTruckStop<Resolved> {}
impl Consume for TruckStop<Resolved, Consumer, Structure> {}
impl Consume for TruckStop<Resolved, Consumer, Creep> {}

pub type Args<'a> = (ColonyView<'a>, &'a mut Movement<Resolved>, &'a mut TruckCoordinator<Resolved>, &'a mut Messages<Resolved>);
impl StateMachine<Creep, Args<'_>> for TruckCreep<Resolved> {
    fn update(self, creep: &Creep, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        let (home, movement, coordinator, messages) = args;
        
        let buffer = home.buffer.as_ref().ok_or(anyhow!("Unable to get buffer"))?;
        let buffer_energy = buffer.store().get_used_capacity(Some(ResourceType::Energy));
        let buffer_free_capacity = buffer.store().get_free_capacity(Some(ResourceType::Energy));

        match &self {
            Self::FillingUpFor(ConsumerTruckStop::Creep(creep)) |
            Self::Performing(TruckTask::ProvidingTo(ConsumerTruckStop::Creep(creep))) => {
                messages.creep(&creep.id).send(CreepMessage::TruckTarget);
            }
            _ => ()
        }

        match &self {
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

                Ok(Break(self))
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
                    movement.smart_move_creep_to(creep, task.pos()).ok();
                    Ok(Break(self))
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
                    movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(Break(self))
                }
            },
            Self::StoringAway => {
                if creep.pos().is_near_to(buffer.pos()) {
                    creep.transfer(buffer.transferable(), ResourceType::Energy, None).ok();
                    Ok(Break(Self::Idle))
                } else {
                    movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(Break(self))
                }
            },
        }
    }
}

impl TruckTask<Resolved> {
    fn pos(&self) -> Position {
        match self {
            TruckTask::CollectingFrom(provider) => provider.pos(),
            TruckTask::ProvidingTo(consumer) => consumer.pos()
        }
    }

    fn perform(&self, creep: &Creep) -> anyhow::Result<()> {
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
                provider.get_resource_avaliable(ResourceType::Energy) > 0,
            TruckTask::ProvidingTo(consumer) =>
                consumer.get_resource_free(ResourceType::Energy) > 0
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TruckCoordinator<M: IDMode + 'static> {
    providers: TaskServer<ProviderTruckStop<M>, ProviderTaskData>,
    consumers: TaskServer<ConsumerTruckStop<M>, u32>
}

impl TruckCoordinator<Resolved> {
    pub fn update(&mut self, plan: &ColonyPlan, room: &Room, messages: Vec<TruckMessage<Resolved>>) {
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
        providers.extend(plan.center.link.providers().tasks(3, Some(0), None));
        providers.extend(plan.sources.source_containers.providers().tasks(2, Some(500), None));
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
                .max_by_key(|(provider, _, data)| (data.priority, Reverse(provider.get_range_to(creep.pos()))))
        })
    }

    fn assign_provider(&mut self, creep: &Creep) -> Option<ProviderTruckStop> {
        let creep_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .max_by_key(|(provider, amount, data)| ((*amount).min(creep_capacity), data.priority, Reverse(provider.get_range_to(creep.pos()))))
        })
    }

    fn assign_consumer(&mut self, creep: &Creep) -> Option<ConsumerTruckStop> {
        let creep_energy = creep.store().get_used_capacity(Some(ResourceType::Energy));
        self.consumers.assign_task(creep, creep_energy, |tasks| {
            tasks.into_iter()
                .max_by_key(|(consumer, left, priority)| (*priority, *left, Reverse(consumer.get_range_to(creep.pos()))))
        })
    }
}

trait GetDistToTruckStop { fn get_range_to(&self, pos: Position) -> u32; }
impl<T : TruckStopPos> GetDistToTruckStop for T {
    fn get_range_to(&self, other_pos: Position) -> u32 {
        self.pos().map_or(u32::MAX, |pos| pos.get_range_to(other_pos))
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
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        self.get_provide().creep_withdraw(creep, ty)
    }
}

impl TruckStopPos for ProviderTruckStop {
    fn pos(&self) -> Option<Position> {
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
    fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        self.get_consume().creep_transfer(creep, ty)
    }
}

impl TruckStopPos for ConsumerTruckStop {
    fn pos(&self) -> Option<Position> {
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

impl IntoConsumers for Vec<TruckMessage<Resolved>> {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.iter().filter_map(|message| {
            let TruckMessage::Consumer(creep, _) = message else { return None };
            Some(ConsumerTruckStop::Creep(TruckStop::<Consumer, Creep>::new(creep.id)))
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

impl IntoProviders for Vec<TruckMessage<Resolved>> {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.iter().filter_map(|message| {
            let TruckMessage::Provider(creep, _) = message else { return None };
            Some(ProviderTruckStop::Creep(TruckStop::<Provider, Creep>::new(creep.id)))
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

    use anyhow::anyhow;
    use screeps::{Creep, HasId, HasPosition, HasStore, Position, Resource, ResourceType, Ruin, SharedCreepProperties, Store, Structure, StructureObject, Tombstone, Withdrawable};
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::JsCast;

    use crate::{creeps::truck::{GetResourceAvaliable, GetResourceFree, Transfer, Withdraw}, id::{IDMode, IDReqs, IntoResolvedID, Resolved, TryIntoResolvedID}};

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
    pub struct TruckStop<M: IDMode, T, I: IDReqs> {
        pub id: M::Wrap<I>,
        phantom: PhantomData<T>
    }

    impl<M: IDMode, T, I: IDReqs> Hash for TruckStop<M, T, I> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
        }
    }

    impl<M: IDMode, T, I: IDReqs> Eq for TruckStop<M, T, I> { }
    impl<M: IDMode, T, I: IDReqs> PartialEq for TruckStop<M, T, I> {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }

    pub trait TruckStopPos { fn pos(&self) -> Position; }
    impl<T: TruckStopType, I: HasPosition + IDReqs> TruckStopPos for TruckStop<Resolved, T, I> {
        fn pos(&self) -> Position {
            self.id.pos()
        }
    }

    impl<T : TruckStopType> TruckStop<Resolved, T, Structure> {
        fn from_structure<S: Into<Structure>>(structure: S) -> Self {
            let structure = structure.into();
            Self { id: structure.into_rid(), phantom: PhantomData }
        }
    }

    pub trait ProviderStructureReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Resolved, Provider, Structure> {
        pub fn new<S: ProviderStructureReqs>(structure: S) -> Self {
            Self::from_structure(structure)
        }
    }
    pub trait ConsumerStructureReqs = Withdrawable + HasStore + Into<Structure>;
    impl TruckStop<Resolved, Consumer, Structure> {
        pub fn new<S: ConsumerStructureReqs>(structure: S) -> Self {
            Self::from_structure(structure)
        }
    }

    impl<I : OtherEntity + IDReqs> TruckStop<Resolved, Provider, I> {
        pub fn new(entity: &I) -> Self {
            Self { 
                id: entity.clone().into_rid(), 
                phantom: PhantomData 
            }
        }
    }

    impl<T : TruckStopType> TruckStop<Resolved, T, Creep> {
        pub fn new(creep: &Creep) -> Self {
            Self { id: creep.clone().try_into_rid().unwrap(), phantom: PhantomData }
        }
    }


    trait TruckStopStore { fn store(&self) -> Store; }
    impl<T: TruckStopType> TruckStopStore for TruckStop<Resolved, T, Structure> {
        fn store(&self) -> Store {
            StructureObject::from(self.id.inner.clone()).as_has_store().unwrap().store()
        }
    }

    impl<T: TruckStopType> TruckStopStore for TruckStop<Resolved, T, Creep> {
        fn store(&self) -> Store {
            self.id.store()
        }
    }

    impl<T: TruckStopType, I : NormalOtherEntity + IDReqs> TruckStopStore for TruckStop<Resolved, T, I> {
        fn store(&self) -> Store {
            self.id.store()
        }
    }

    impl<I: IDReqs> GetResourceAvaliable for TruckStop<Resolved, Provider, I> where Self : TruckStopStore {
        fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
            self.store().get_used_capacity(Some(ty))
        }
    }

    impl GetResourceAvaliable for TruckStop<Resolved, Provider, Resource> {
        fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
            if self.id.resource_type() == ty {
                self.id.amount()
            } else {
                0
            }
        }
    }

    impl<I: IDReqs> GetResourceAvaliable for TruckStop<Resolved, Consumer, I> where Self : TruckStopStore {
        fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
            self.store().get_used_capacity(Some(ty))
        }
    }

    impl<I: IDReqs> GetResourceFree for TruckStop<Resolved, Consumer, I> where Self : TruckStopStore {
        fn get_resource_free(&self, ty: ResourceType) -> u32 {
            self.store().get_free_capacity(Some(ty)) as u32
        }
    }

    impl Transfer for TruckStop<Resolved, Consumer, Structure> {
        fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            let structure = StructureObject::from(self.id.inner.clone());
            Ok(creep.transfer(structure.as_transferable().ok_or(anyhow!("Entity is not transferable"))?, ty, None)?)
        }
    }

    impl Transfer for TruckStop<Resolved, Consumer, Creep> {
        fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            Ok(creep.transfer(&self.id.inner, ty, None)?)
        }
    }

    impl Withdraw for TruckStop<Resolved, Provider, Structure> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            let structure = StructureObject::from(self.id.inner.clone());
            Ok(creep.withdraw(structure.as_withdrawable().ok_or(anyhow!("Entity is not withdrawable"))?, ty, None)?)
        }
    }

    impl Withdraw for TruckStop<Resolved, Provider, Creep> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            Ok(self.id.inner.transfer(creep, ty, None)?)
        }
    }

    impl<I : NormalOtherEntity + IDReqs> Withdraw for TruckStop<Resolved, Provider, I> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            Ok(creep.withdraw(&self.id.inner, ty, None)?)
        }
    }

    impl Withdraw for TruckStop<Resolved, Provider, Resource> {
        fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            if self.id.resource_type() != ty { return Err(anyhow!("Resource has wrong type. Expected {ty}, found {}", resource.resource_type())) }
            Ok(creep.pickup(&self.id)?)
        }
    }
}