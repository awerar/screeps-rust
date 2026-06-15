use std::cmp::Reverse;

use screeps::{Creep, HasPosition, ResourceType, Room, StructureContainer, find};
use serde::{Deserialize, Serialize};

use crate::{colony::planning::{plan::ColonyPlan, planned_ref::{PlannedStructureRefs, ResolvableStructureRef, StructureRefReq}}, creeps::truck::{state::TruckTask, stop::{ConsumerTruckStop, ProviderTruckStop, safe_structure::{ConsumerStructure, ProviderStructure}}}, safeid::{GetSafeID, SafeID}, tasks::{TaskAmount, TaskServer, prune_deserialize_taskserver}, utils::EnergyStore};

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
    pub fn update(&mut self, plan: &ColonyPlan, room: &Room, creep_stops: CreepStops) {
        self.consumers.handle_timeouts();
        self.providers.handle_timeouts();

        self.update_providers(plan, room, creep_stops.providers);
        self.update_consumers(plan, creep_stops.consumers);
    }

    fn update_providers(&mut self, plan: &ColonyPlan, room: &Room, provider_creeps: Vec<SafeID<Creep>>) {
        let dropped_resources = room.find(find::DROPPED_RESOURCES, None).into_iter().map(|x| x.safe_id()).map(ProviderTruckStop::Resource);
        let tombstones = room.find(find::TOMBSTONES, None).into_iter().map(|x| x.safe_id()).map(ProviderTruckStop::Tombstone);
        let ruins = room.find(find::RUINS, None).into_iter().map(|x| x.safe_id()).map(ProviderTruckStop::Ruin);

        let creep_providers = provider_creeps.into_iter().map(ProviderTruckStop::Creep);
        let center_link = plan.center.link.resolve().map(ProviderStructure::new).map(ProviderTruckStop::Structure);
        let unlinked_source_containers = plan.unlinked_source_containers().0.into_iter().filter_map(|x| x.resolve()).map(ProviderStructure::new).map(ProviderTruckStop::Structure);
        
        let terminal = plan.center.terminal.resolve().map(ProviderStructure::new).map(ProviderTruckStop::Structure);

        let mut providers = ProviderTasksBuilder::new();
        providers.add_next_priority_group(dropped_resources).push_amount(0);
        providers.add_next_priority_group(creep_providers).push_amount(0);
        providers.add_next_priority_group(tombstones);
        providers.add_next_priority_group(ruins);
        providers.add_next_priority_group(center_link).push_amount(0);
        providers.add_next_priority_group(unlinked_source_containers).push_amount(500);
        providers.add_next_priority_group(terminal).min_leave(10_000);
        self.providers.set_tasks(providers.build());
    }

    fn update_consumers(&mut self, plan: &ColonyPlan, consumer_creeps: Vec<SafeID<Creep>>) {
        let creep_consumers = consumer_creeps.into_iter().map(ConsumerTruckStop::Creep);

        let center_spawn = plan.center.spawn.resolve().map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);
        let center_extensions = plan.center.extensions.iter().filter_map(ResolvableStructureRef::resolve).map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);
        let towers = plan.center.towers.iter().filter_map(ResolvableStructureRef::resolve).map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);
        let terminal = plan.center.terminal.resolve().map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);

        let mut consumers = ConsumerTasksBuilder::new();
        consumers.add_next_priority_group(center_spawn);
        consumers.add_next_priority_group(center_extensions);
        consumers.add_next_priority_group(towers);
        consumers.add_next_priority_group(creep_consumers);
        consumers.add_next_priority_group(terminal).max_fill(2_000);
        self.consumers.set_tasks(consumers.build());
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

struct ProviderTasksBuilder {
    groups: Vec<(Vec<ProviderTruckStop>, ProviderTasksGroupConfig)>
}

impl ProviderTasksBuilder {
    fn new() -> Self {
        ProviderTasksBuilder { groups: Vec::new() }
    }

    fn add_next_priority_group(&mut self, iter: impl IntoIterator<Item = ProviderTruckStop>) -> &mut ProviderTasksGroupConfig {
        &mut self.groups.push_mut((iter.into_iter().collect(), ProviderTasksGroupConfig::default())).1
    }

    fn build(self) -> impl Iterator<Item = (ProviderTruckStop, TaskAmount, ProviderTaskData)> {
        self.groups.into_iter().rev().enumerate().flat_map(|(priority, (providers, config))| {
            providers.into_iter().map(move |provider| {
                let provide = provider.get_resource_avaliable(ResourceType::Energy).saturating_sub(config.min_leave.unwrap_or(0));

                (provider, provide, ProviderTaskData { priority: priority as u32, push_amount: config.push_amount })
            })
        })
    }
}

#[derive(Default)]
struct ProviderTasksGroupConfig {
    push_amount: Option<u32>, 
    min_leave: Option<u32>
}

impl ProviderTasksGroupConfig {
    fn push_amount(&mut self, x: u32) -> &mut Self { self.push_amount = Some(x); self }
    fn min_leave(&mut self, x: u32) -> &mut Self { self.min_leave = Some(x); self }
}

struct ConsumerTasksBuilder {
    groups: Vec<(Vec<ConsumerTruckStop>, ConsumerTasksGroupConfig)>
}

impl ConsumerTasksBuilder {
    fn new() -> Self {
        ConsumerTasksBuilder { groups: Vec::new() }
    }

    fn add_next_priority_group(&mut self, iter: impl IntoIterator<Item = ConsumerTruckStop>) -> &mut ConsumerTasksGroupConfig {
        &mut self.groups.push_mut((iter.into_iter().collect(), ConsumerTasksGroupConfig::default())).1
    }

    fn build(self) -> impl Iterator<Item = (ConsumerTruckStop, TaskAmount, u32)> {
        self.groups.into_iter().rev().enumerate().flat_map(|(priority, (consumers, config))| {
            consumers.into_iter().map(move |consumer| {
                let used = consumer.get_resource_avaliable(ResourceType::Energy);
                let capacity_left = consumer.get_resource_free(ResourceType::Energy);
                let consume = config.max_fill.map_or(capacity_left, |max_fill| max_fill.saturating_sub(used));

                (consumer, consume, priority as u32)
            })
        })
    }
}

#[derive(Default)]
struct ConsumerTasksGroupConfig {
    max_fill: Option<u32>
}

impl ConsumerTasksGroupConfig {
    fn max_fill(&mut self, x: u32) -> &mut Self { self.max_fill = Some(x); self }
}