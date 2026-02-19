use screeps::{Creep, HasId, HasPosition, HasStore, MaybeHasId, ObjectId, Position, ResourceType, SharedCreepProperties, Store, Structure, StructureObject, Transferable, Withdrawable, action_error_codes::{TransferErrorCode, WithdrawErrorCode}};
use serde::{Deserialize, Serialize};

use crate::{colony::planning::{plan::ColonyPlan, planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}}, memory::Memory, statemachine::StateMachine, tasks::{MultiTasksQueue, ScheduledTask, TaskPriority}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
pub enum TruckCreep {
    #[default] Idle,
    Performing(TruckTask),
    StoringAway,
    FillingUpFor(ConsumerId)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum TruckTask {
    CollectingFrom(ProviderId),
    ProvidingTo(ConsumerId)
}

impl StateMachine<Creep> for TruckCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        let buffer = mem.creep_home(creep).ok_or(())?.buffer().ok_or(())?;
        let coordinator = mem.truck_coordinators.entry(mem.creep(creep).unwrap().home).or_default();

        let buffer_energy = buffer.store().get_used_capacity(Some(ResourceType::Energy));
        let buffer_free_capacity = buffer.store().get_free_capacity(Some(ResourceType::Energy));

        match self {
            Self::Idle => {
                let creep_energy = creep.store().get_used_capacity(Some(ResourceType::Energy));
                let creep_free_capacity = creep.store().get_free_capacity(Some(ResourceType::Energy)) as u32;

                if creep_energy > 0 {
                    let consumer = coordinator.consumers.assign_task_to(creep, creep_energy, true);
                    if let Some(consumer) = consumer { return Ok(Self::Performing(TruckTask::ProvidingTo(consumer))) }

                    if buffer_free_capacity >= 0 { return Ok(Self::StoringAway) }

                    Ok(Self::Idle)
                } else {
                    let full_provider = coordinator.providers.assign_task_to(creep, creep_free_capacity, false);
                    if let Some(provider) = full_provider { return Ok(Self::Performing(TruckTask::CollectingFrom(provider))) }

                    if buffer_energy > 0 {
                        let consumer = coordinator.consumers.assign_task_to(creep, creep_free_capacity, true);
                        if let Some(consumer) = consumer { return Ok(Self::FillingUpFor(consumer)) }
                    }

                    let fractional_provider = coordinator.providers.assign_task_to(creep, creep_free_capacity, true);
                    if let Some(provider) = fractional_provider { return Ok(Self::Performing(TruckTask::CollectingFrom(provider))) }

                    Ok(Self::Idle)
                }
            },
            Self::Performing(task) => {
                if !task.still_valid() {
                    coordinator.finish(creep, task, false);
                    return Ok(Self::Idle)
                }

                coordinator.heartbeat(creep, task);
                let target = task.pos()?;

                if creep.pos().is_near_to(target) {
                    task.perform(creep)?;
                    coordinator.finish(creep, task, true);
                    Ok(Self::Idle)
                } else {
                    mem.movement.smart_move_creep_to(creep, target).ok();
                    Ok(self.clone())
                }
            },
            Self::FillingUpFor(consumer) => {
                if buffer_energy == 0 {
                    coordinator.consumers.finish(creep.try_id().unwrap(), consumer, false);
                    return Ok(Self::Idle)
                }

                coordinator.consumers.heartbeat(creep, consumer);

                if creep.pos().is_near_to(buffer.pos()) {
                    creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None).ok();
                    Ok(Self::Performing(TruckTask::ProvidingTo(*consumer)))
                } else {
                    mem.movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(self.clone())
                }
            },
            Self::StoringAway => {
                if creep.pos().is_near_to(buffer.pos()) {
                    creep.transfer(buffer.transferable(), ResourceType::Energy, None).ok();
                    Ok(Self::Idle)
                } else {
                    mem.movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(self.clone())
                }
            },
        }
    }
}

impl TruckTask {
    fn pos(&self) -> Result<Position, ()> {
        match self {
            TruckTask::CollectingFrom(provider) => provider.resolve_pos().ok_or(()),
            TruckTask::ProvidingTo(consumer) => consumer.resolve_pos().ok_or(()),
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
            TruckTask::CollectingFrom(provider) => provider.store()
                .is_ok_and(|store| store.get_used_capacity(Some(ResourceType::Energy)) > 0),
            TruckTask::ProvidingTo(consumer) => consumer.store()
                .is_ok_and(|store| store.get_free_capacity(Some(ResourceType::Energy)) > 0),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TruckTaskCoordinator {
    providers: MultiTasksQueue<ProviderId>,
    consumers: MultiTasksQueue<ConsumerId>
}

impl TruckTaskCoordinator {
    pub fn update(&mut self, plan: &ColonyPlan) {
        self.consumers.handle_timeouts();
        self.providers.handle_timeouts();

        let mut providers = Vec::new();
        providers.extend(plan.center.link.resolve_provider_task(2));
        providers.extend(plan.sources.source_containers.resolve_provider_tasks(1));
        self.providers.set_tasks(providers);

        let mut consumers = Vec::new();
        consumers.extend(plan.center.spawn.resolve_consumer_task(4));
        consumers.extend(plan.center.extensions.resolve_consumer_tasks(3));
        consumers.extend(plan.center.towers.resolve_consumer_tasks(2));
        consumers.extend(plan.center.terminal.resolve_consumer_task(1));
        self.consumers.set_tasks(consumers);
    }

    fn finish(&mut self, creep: &Creep, task: &TruckTask, success: bool) {
        match task {
            TruckTask::CollectingFrom(task) => 
                self.providers.finish(creep.try_id().unwrap(), task, success),
            TruckTask::ProvidingTo(task) => 
                self.consumers.finish(creep.try_id().unwrap(), task, success),
        }
    }

    fn heartbeat(&mut self, creep: &Creep, task: &TruckTask) -> bool {
        match task {
            TruckTask::CollectingFrom(task) => self.providers.heartbeat(creep, task),
            TruckTask::ProvidingTo(task) => self.consumers.heartbeat(creep, task),
        }
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

        if matches!(withdraw_result, Ok(()) | Err(WithdrawErrorCode::Full)) { Ok(()) } else { Err(()) }
    }

    pub fn store(&self) -> Result<Store, ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        structure.as_has_store().map(HasStore::store).ok_or(())
    }

    pub fn resolve_pos(&self) -> Option<Position> {
        self.0.resolve().map(|structure| structure.pos())
    }
}

trait ResolvableProviderTaskRef { fn resolve_provider_task(&self, priority: TaskPriority) -> Option<ScheduledTask<ProviderId>>; }
impl<R: ResolvableStructureRef> ResolvableProviderTaskRef for R where R::Structure : ProviderReqs + StructureRefReq {
    fn resolve_provider_task(&self, priority: TaskPriority) -> Option<ScheduledTask<ProviderId>> {
        let provider = self.resolve()?;
       Some(ScheduledTask {
            priority,
            target: provider.store().get_used_capacity(Some(ResourceType::Energy)),
            pos: provider.pos(),
            task: ProviderId::new(provider),
        })
    }
}

impl<T : ProviderReqs + StructureRefReq> PlannedStructureRefs<T> {
    fn resolve_provider_tasks(&self, priority: TaskPriority) -> Vec<ScheduledTask<ProviderId>> {
        self.0.iter()
            .filter_map(|provider| provider.resolve_provider_task(priority))
            .collect()
    }
}

impl ConsumerId {
    pub fn new<T: ConsumerReqs>(t: T) -> Self {
        Self(t.into().id())
    }

    pub fn transfer(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        let withdraw_result = creep.transfer(structure.as_transferable().unwrap(), ty, amount);

        if matches!(withdraw_result, Ok(()) | Err(TransferErrorCode::Full)) { Ok(()) } else { Err(()) }
    }

    pub fn store(&self) -> Result<Store, ()> {
        let structure = StructureObject::from(self.0.resolve().ok_or(())?);
        structure.as_has_store().map(HasStore::store).ok_or(())
    }

    pub fn resolve_pos(&self) -> Option<Position> {
        self.0.resolve().map(|structure| structure.pos())
    }
}

trait ResolvableConsumerTaskRef { fn resolve_consumer_task(&self, priority: TaskPriority) -> Option<ScheduledTask<ConsumerId>>; }
impl<R: ResolvableStructureRef> ResolvableConsumerTaskRef for R where R::Structure : ConsumerReqs + StructureRefReq {
    fn resolve_consumer_task(&self, priority: TaskPriority) -> Option<ScheduledTask<ConsumerId>> {
        let consumer = self.resolve()?;
        Some(ScheduledTask {
            priority,
            target: consumer.store().get_used_capacity(Some(ResourceType::Energy)),
            pos: consumer.pos(),
            task: ConsumerId::new(consumer),
        })
    }
}

impl<T : ConsumerReqs + StructureRefReq> PlannedStructureRefs<T> {
    fn resolve_consumer_tasks(&self, priority: TaskPriority) -> Vec<ScheduledTask<ConsumerId>> {
        self.0.iter()
            .filter_map(|consumer| consumer.resolve_consumer_task(priority))
            .collect()
    }
}