use std::cmp::Reverse;

use screeps::{Creep, ResourceType, Room, StructureContainer, find};
use serde::{Deserialize, Serialize};

use crate::{check::{Filtered, TriviallyChecked, deserialize_filter_check}, colony::planning::{plan::ColonyPlan, planned_ref::{PlannedStructureRefs, ResolvableStructureRef}}, coordination::{collaboration::{Collaboration, CollaborativeWorkerHandle, RemainingWork}, tasks::{OverwriteableTaskData, Tasks}}, creeps::{truck::{state::TruckTask, stop::{ConsumerTruckStop, ProviderTruckStop, safe_structure::{ConsumerStructure, ProviderStructure}}}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{ById, Handle, WithId}};

#[derive(Serialize, Deserialize, Default)]
pub struct TruckCoordinator {
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub providers: Tasks<ProviderTruckStop, (ProviderTaskData, Filtered<Collaboration>)>,
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub consumers: Tasks<ConsumerTruckStop, (ConsumerTaskPriority, Filtered<Collaboration>)>
}

#[derive(Serialize, Deserialize)]
pub struct ProviderTaskData {
    pub priority: u32,
    pub push_amount: Option<u32>
}

impl TriviallyChecked for ProviderTaskData {}
impl OverwriteableTaskData for ProviderTaskData {}

#[derive(Serialize, Deserialize)]
pub struct ConsumerTaskPriority(pub u32);

impl TriviallyChecked for ConsumerTaskPriority {}
impl OverwriteableTaskData for ConsumerTaskPriority {}

pub struct CreepStops {
    pub consumers: Vec<WithId<Creep>>,
    pub providers: Vec<WithId<Creep>>
}

impl TruckCoordinator {
    pub fn update(&mut self, plan: &ColonyPlan, room: &Room, creep_stops: CreepStops) {
        self.update_providers(plan, room, creep_stops.providers);
        self.update_consumers(plan, creep_stops.consumers);
    }

    fn update_providers(&mut self, plan: &ColonyPlan, room: &Room, provider_creeps: Vec<WithId<Creep>>) {
        let dropped_resources = room.find(find::DROPPED_RESOURCES, None).into_iter().map(ById).map(ProviderTruckStop::Resource);
        let tombstones = room.find(find::TOMBSTONES, None).into_iter().map(ById).map(ProviderTruckStop::Tombstone);
        let ruins = room.find(find::RUINS, None).into_iter().map(ById).map(ProviderTruckStop::Ruin);

        let creep_providers = provider_creeps.into_iter().map(ById).map(ProviderTruckStop::Creep);
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

    fn update_consumers(&mut self, plan: &ColonyPlan, consumer_creeps: Vec<WithId<Creep>>) {
        let creep_consumers = consumer_creeps.into_iter().map(ById).map(ConsumerTruckStop::Creep);

        let center_spawn = plan.center.spawn.resolve().map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);
        let center_extensions = plan.center.extensions.iter().filter_map(ResolvableStructureRef::resolve).map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);
        let towers = plan.center.towers.iter().filter_map(ResolvableStructureRef::resolve).map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);
        let terminal = plan.center.terminal.resolve().map(ConsumerStructure::new).map(ConsumerTruckStop::Structure);

        let mut consumers = ConsumerTasksBuilder::new();
        consumers.add_next_priority_group(center_spawn);
        consumers.add_next_priority_group(center_extensions);
        consumers.add_next_priority_group(towers).threshold(0.8);
        consumers.add_next_priority_group(creep_consumers).threshold(0.35);
        consumers.add_next_priority_group(terminal).max_fill(2_000).threshold(0.5);
        self.consumers.set_tasks(consumers.build());
    }

    pub fn heartbeat_consumer(&mut self, creep: Handle<WithId<Creep>>, task: &ConsumerTruckStop) -> Option<CollaborativeWorkerHandle<'_>> {
        self.consumers.get_mut(task).and_then(|(_, collab)| collab.heartbeat(creep))
    }

    pub fn heartbeat_provider(&mut self, creep: Handle<WithId<Creep>>, task: &ProviderTruckStop) -> Option<CollaborativeWorkerHandle<'_>> {
        self.providers.get_mut(task).and_then(|(_, collab)| collab.heartbeat(creep))
    }

    pub fn heartbeat(&mut self, creep: Handle<WithId<Creep>>, task: &TruckTask) -> Option<CollaborativeWorkerHandle<'_>> {
        match task {
            TruckTask::CollectingFrom(task) => self.heartbeat_provider(creep, task),
            TruckTask::ProvidingTo(task) => self.heartbeat_consumer(creep, task)
        }
    }

    pub fn assign_push_provider(&mut self, truck: &VirtualCreep) -> Option<ProviderTruckStop> {
        self.providers.iter_mut()
                .filter(|(_, (data, collab))| data.push_amount.is_some_and(|push_amount| collab.unassigned_work() >= push_amount))
                .max_by_key(|(provider, (data, _))|  {
                    (
                        data.priority, 
                        Reverse(provider.pos().get_range_to(truck.pos()))
                    )
                }).map(|(task, (_, collab))| {
                    collab.add(truck.handle(), truck.next_free_capacity());
                    task.clone()
                })
    }

    pub fn assign_provider(&mut self, truck: &VirtualCreep) -> Option<ProviderTruckStop> {
        self.providers.iter_mut()
            .max_by_key(|(provider, (data, collab))| {
                (
                    collab.unassigned_work().min(truck.next_free_capacity()), 
                    data.priority,
                    Reverse(provider.pos().get_range_to(truck.pos()))
                )
            }).map(|(task, (_, collab))| {
                collab.add(truck.handle(), truck.next_free_capacity());
                task.clone()
            })
    }

    pub fn assign_consumer(&mut self, truck: &VirtualCreep) -> Option<ConsumerTruckStop> {
        self.consumers.iter_mut()
            .max_by_key(|(consumer, (priority, collab))| { 
                (
                    priority.0, 
                    collab.unassigned_work(), 
                    Reverse(consumer.pos().get_range_to(truck.pos()))
                )
            }).map(|(task, (_, collab))| {
                collab.add(truck.handle(), truck.next_free_capacity());
                task.clone()
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

// TODO: VVVVVV Could probably simplified

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

    fn build(self) -> impl Iterator<Item = (ProviderTruckStop, (ProviderTaskData, RemainingWork))> {
        self.groups.into_iter().rev().enumerate()
            .flat_map(|(priority, (providers, config))| {
                providers.into_iter().map(move |provider| {
                    let provide = provider.get_resource_avaliable(ResourceType::Energy).saturating_sub(config.min_leave.unwrap_or(0));

                    (provider, (ProviderTaskData { priority: priority as u32, push_amount: config.push_amount }, RemainingWork(provide)))
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

    fn build(self) -> impl Iterator<Item = (ConsumerTruckStop, (ConsumerTaskPriority, RemainingWork))> {
        self.groups.into_iter().rev().enumerate()
            .flat_map(|(priority, (consumers, config))| {
                consumers.into_iter()
                    .filter(move |consumer| {
                        let Some(fullness_threshold) = config.fullness_threshold else { return true };

                        let upper_limit = config.max_fill.unwrap_or_else(|| consumer.energy_capacity());
                        let ratio = consumer.used_energy_capacity() as f32 / upper_limit as f32;
                        ratio <= fullness_threshold
                    }).map(move |consumer| {
                        let used = consumer.used_energy_capacity();
                        let capacity_left = consumer.free_energy_capacity();
                        let consume = config.max_fill.map_or(capacity_left, |max_fill| max_fill.saturating_sub(used));

                        (consumer, (ConsumerTaskPriority(priority as u32), RemainingWork(consume)))
                    })
            })
    }
}

#[derive(Default)]
struct ConsumerTasksGroupConfig {
    max_fill: Option<u32>,
    fullness_threshold: Option<f32>
}

impl ConsumerTasksGroupConfig {
    fn max_fill(&mut self, x: u32) -> &mut Self { self.max_fill = Some(x); self }
    fn threshold(&mut self, x: f32) -> &mut Self { self.fullness_threshold = Some(x); self }
}