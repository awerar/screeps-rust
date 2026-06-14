use std::cmp::Reverse;

use screeps::{Creep, HasPosition, Room, StructureContainer};
use serde::{Deserialize, Serialize};

use crate::{colony::planning::{plan::ColonyPlan, planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}}, creeps::truck::{state::TruckTask, stop::{ConsumerTruckStop, ProviderTruckStop}}, safeid::SafeID, tasks::{TaskServer, prune_deserialize_taskserver}, utils::EnergyStore};

#[derive(Serialize, Deserialize, Default)]
pub struct TruckCoordinator {
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    pub providers: TaskServer<ProviderTruckStop, ProviderTaskData>,
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    pub consumers: TaskServer<ConsumerTruckStop, u32>
}

#[derive(Serialize, Deserialize)]
pub struct ProviderTaskData {
    pub priority: u32,
    pub push_amount: Option<u32>
}

pub struct CreepStops {
    pub consumers: Vec<SafeID<Creep>>,
    pub providers: Vec<SafeID<Creep>>
}

impl TruckCoordinator {
    pub fn update(&mut self, plan: &ColonyPlan, room: &Room, creep_stops: &CreepStops) {
        self.consumers.handle_timeouts();
        self.providers.handle_timeouts();

        let mut providers = Vec::new();
        /*providers.extend(room.find(find::DROPPED_RESOURCES, None).providers().tasks(7, Some(0), None));
        providers.extend(creep_stops.providers().tasks(6, Some(0),  None));
        providers.extend(room.find(find::TOMBSTONES, None).providers().tasks(5, None, None));
        providers.extend(room.find(find::RUINS, None).providers().tasks(4, None, None));
        providers.extend(plan.center.link.providers().tasks(3, Some(0), None));
        providers.extend(plan.unlinked_source_containers().providers().tasks(2, Some(500), None)); // TODO
        providers.extend(plan.center.terminal.providers().tasks(1, None, Some(10_000)));*/
        self.providers.set_tasks(providers);

        let mut consumers = Vec::new();
        /*consumers.extend(plan.center.spawn.consumers().tasks(5, None));
        consumers.extend(plan.center.extensions.consumers().tasks(4, None));
        consumers.extend(plan.center.towers.consumers().tasks(3, None));
        consumers.extend(creep_stops.consumers().tasks(2, None));
        consumers.extend(plan.center.terminal.consumers().tasks(1, Some(2_000)));*/
        self.consumers.set_tasks(consumers);
    }

    pub fn heartbeat(&mut self, creep: &Creep, task: &TruckTask) -> bool {
        match task {
            TruckTask::CollectingFrom(task) => self.providers.heartbeat_task(creep, task),
            TruckTask::ProvidingTo(task) => self.consumers.heartbeat_task(creep, task)
        }
    }

    pub fn finish(&mut self, creep: &Creep, task: &TruckTask, success: bool) {
        match task {
            TruckTask::CollectingFrom(task) => 
                self.providers.finish_task(creep.try_id().unwrap(), task, success),
            TruckTask::ProvidingTo(task) => 
                self.consumers.finish_task(creep.try_id().unwrap(), task, success)
        }
    }

    pub fn assign_push_provider(&mut self, creep: &Creep, delta: i32) -> Option<ProviderTruckStop> {
        let creep_capacity = (creep.store().free_energy_capacity() - delta) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .filter(|(_, amount, data)| data.push_amount.is_some_and(|push_amount| *amount >= push_amount))
                .max_by_key(|(provider, _, data)| (data.priority, Reverse(provider.pos().get_range_to(creep.pos()))))
        })
    }

    pub fn assign_provider(&mut self, creep: &Creep, delta: i32) -> Option<ProviderTruckStop> {
        let creep_capacity = (creep.store().free_energy_capacity() - delta) as u32;
        self.providers.assign_task(creep, creep_capacity, |tasks| {
            tasks.into_iter()
                .max_by_key(|(provider, amount, data)| ((*amount).min(creep_capacity), data.priority, Reverse(provider.pos().get_range_to(creep.pos()))))
        })
    }

    pub fn assign_consumer(&mut self, creep: &Creep, delta: i32) -> Option<ConsumerTruckStop> {
        let creep_energy = creep.store().used_energy_capacity().strict_add_signed(delta);
        self.consumers.assign_task(creep, creep_energy, |tasks| {
            tasks.into_iter()
                .max_by_key(|(consumer, left, priority)| (*priority, *left, Reverse(consumer.pos().get_range_to(creep.pos()))))
        })
    }
}

impl ColonyPlan {
    fn unlinked_source_containers(&self) -> PlannedStructureRefs<StructureContainer> {
        let center_link_exists = self.center.link.resolve().is_some();

        PlannedStructureRefs(
            self.sources.values()
                .filter(|source_plan| {
                    let source_link_exists = source_plan.link.resolve().is_some();
                    !center_link_exists || !source_link_exists
                }).filter_map(|source_plan| source_plan.container.0.clone())
                .collect()
        )
    }
}

/*trait IntoConsumers { fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop>; }
impl<R, S: ConsumerStructureReqs> IntoConsumers for R where R : ResolvableStructureRef<Structure = S> {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.resolve().map(TruckStop::<Consumer, Structure>::new).map(ConsumerTruckStop::Structure)
    }
}

impl<S: ConsumerStructureReqs + StructureRefReq> IntoConsumers for PlannedStructureRefs<S> {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.resolve().into_iter().map(TruckStop::<Consumer, Structure>::new).map(ConsumerTruckStop::Structure)
    }
}

impl IntoConsumers for CreepStops {
    fn consumers(&self) -> impl IntoIterator<Item = ConsumerTruckStop> {
        self.consumers.iter()
            .cloned()
            .map(TruckStop::<Consumer, Creep>::new)
            .map(ConsumerTruckStop::Creep)
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

impl IntoProviders for CreepStops {
    fn providers(&self) -> impl IntoIterator<Item = ProviderTruckStop> {
        self.providers.iter()
            .cloned()
            .map(TruckStop::<Provider, Creep>::new)
            .map(ProviderTruckStop::Creep)
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

pub trait CreateProviderTasks { 
    fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (ProviderTruckStop, TaskAmount, ProviderTaskData)>; 
}

impl<I : IntoIterator<Item = ProviderTruckStop>> CreateProviderTasks for I {
    fn tasks(self, priority: u32, push_amount: Option<u32>, min_leave: Option<u32>) -> impl Iterator<Item = (ProviderTruckStop, TaskAmount, ProviderTaskData)> {
        self.into_iter().map(move |provider| {
            let provide = provider.get_resource_avaliable(ResourceType::Energy).saturating_sub(min_leave.unwrap_or(0));

            (provider, provide, ProviderTaskData { priority, push_amount })
        })
    }
}

pub trait CreateConsumerTasks { 
    fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (ConsumerTruckStop, TaskAmount, u32)>; 
}

impl<I : IntoIterator<Item = ConsumerTruckStop>> CreateConsumerTasks for I {
    fn tasks(self, priority: u32, max_fill: Option<u32>) -> impl Iterator<Item = (ConsumerTruckStop, TaskAmount, u32)> {
        self.into_iter().map(move |consumer| {
            let used = consumer.get_resource_avaliable(ResourceType::Energy);
            let capacity_left = consumer.get_resource_free(ResourceType::Energy);
            let consume = max_fill.map_or(capacity_left, |max_fill| max_fill.saturating_sub(used));

            (consumer, consume, priority)
        })
    }
}*/