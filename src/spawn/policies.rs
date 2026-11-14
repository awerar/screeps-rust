use std::sync::LazyLock;

use itertools::Itertools;
use log::warn;
use ordered_float::OrderedFloat;
use screeps::{BUILD_POWER, CONTROLLER_MAX_UPGRADE_PER_TICK, CREEP_LIFE_TIME, Creep, HasPosition, Part, Source};

use crate::{colony::{ColonyBuffer, ColonyView, plan::SourcePlan}, creeps::{CreepRole, body::{Body, ScaleStopCondition}, fabricator::{FABRICATOR_HORIZON, FabricatorCreep}, flagship::FlagshipCreep, servicer::{ServicerCoordinator, ServicerCreep}, truck::{ExportTruckCreep, TruckCreep}}, domain_traits::{EnergyStoreAccessors, HasId, HasName, ResolvableId}, memory::Memory, movement::requests::TugboatRequests, spawn::scheduler::{ColonyScheduler, ColonySchedulerMut, RoleSelector, SpawnScheduler}, statemachine::{Flow, OrBreak, break_if, run}};

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() {
        7
    } else {
        6
    };

    let excavator_works = energy.saturating_sub(Part::Carry.cost()).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, excavator_works as usize)
}

pub fn schedule_excavators(scheduler: &mut ColonySchedulerMut<'_>, view: &ColonyView<'_>) -> anyhow::Result<()> {
    for (source, source_plan) in &view.plan.sources {
        let Some(source) = source.resolve() else { continue; };
        schedule_excavator(scheduler, &source, source_plan).into_result()?;
    }

    Ok(())
}

fn schedule_excavator(scheduler: &mut ColonySchedulerMut<'_>, source: &Source, source_plan: &SourcePlan) -> Flow {
    break_if(!scheduler.has_free())?;
    break_if(scheduler.home_creeps().of_role(RoleSelector::SourceExcavator(source.id())).next().is_some())?;

    let body = get_excavator_body(scheduler.future_energy(), source_plan);
    let role = CreepRole::Excavator(source.id());

    if source_plan.spawn.resolve().is_some() {
        scheduler.schedule_selected(
            |mut iter| iter.find(|(_, spawn)| spawn.is_source_spawn(&source.id())).map(|(ix, _)| ix).or_break(),
            body,
            role
        )
    } else {
        scheduler.schedule(body, role)
    }
}

// Truck capacity C = 50y energy
// Roundtrip time T = 2x ticks
// Production P = 10 energy per tick
// C/T = P => C = PT => 50y = 20x => y = 0.4x
const TRUCK_SOURCE_CARRY_PER_DIST: f32 = 0.4;

// Napkin math
// Truck capacity C = 50y
// Center radius R = 5 steps
// Roundtrip time T = 2R = 10 ticks
// Consumption P = 20
// C = PT => 50y = 200 => y = 4
// Creep cost = 1.5y * 50 / 1500 = 0.2
const TRUCK_CENTER_CARRY: f32 = 4.0;
const TRUCK_FABRICATOR_CARRY: f32 = 10.0; // TODO: Fix this properly

const TRUCK_CARRY_MARGIN: f32 = 0.25;

static TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Move, Carry, Carry]) });
static MAX_TRUCK_ENERGY: LazyLock<u32> = LazyLock::new(||  (TRUCK_TEMPLATE.clone() * 10).energy_required());
fn get_truck_body(energy: u32) -> Option<Body> {
    TRUCK_TEMPLATE.scaled(energy.min(*MAX_TRUCK_ENERGY))
}

pub fn schedule_trucks(scheduler: &mut ColonySchedulerMut<'_>, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    let total_carry_for_sources = colony.plan.sources.values()
        .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
        .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
        .sum::<f32>();

    let target_carry = if scheduler.syndrome().any_problems() {
        1
    } else {
        ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize
    };

    run(|| {
        break_if(scheduler.home_creeps().part_count(RoleSelector::Truck, Part::Carry) >= target_carry)?;

        let body = get_truck_body(scheduler.future_energy()).or_break()?;

        scheduler.schedule(
            body,
            CreepRole::Truck(TruckCreep::default())
        )
    })
}

const IMPORT_BUDGET_LIMIT: f32 = 100.0;
const EXPORT_BUDGET_LIMIT: f32 = 10.0;

static EXPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Move, Carry]) });
pub fn schedule_export_trucks(scheduler: &mut SpawnScheduler, mem: &mut Memory) -> anyhow::Result<()> {
    for import_colony in mem.colonies.view_all() {
        if import_colony.buffer.is_some_and(|buffer| matches!(buffer, ColonyBuffer::Storage(_))) { continue; }

        run(|| {
            break_if(scheduler.get(import_colony.name).income() > IMPORT_BUDGET_LIMIT)?;

            let export_name = scheduler.iter()
                .filter(|candidate| candidate.is_available() && candidate.name() != import_colony.name)
                .filter(|candidate| matches!(mem.colonies.view(candidate.name()).buffer, Some(ColonyBuffer::Storage(_))))
                .map(|candidate| (candidate.name(), candidate.full_budget()))
                .filter(|(_, budget)| *budget > EXPORT_BUDGET_LIMIT)
                .max_by_key(|(_, budget)| OrderedFloat(*budget))
                .map(|(name, _)| name)
                .or_break()?;

            let mut scheduler = scheduler.get_mut(export_name);
            let body = EXPORT_TRUCK_TEMPLATE.scaled(scheduler.future_energy()).or_break()?;

            scheduler.schedule(
                body,
                CreepRole::ExportTruck(ExportTruckCreep::new(&mem.colonies.view(export_name), &import_colony))
            )
        })?;
    }

    Ok(())
}

static FLAGSHIP_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Claim, Move]) });
pub fn schedule_flagship(scheduler: &mut SpawnScheduler, mem: &mut Memory) -> anyhow::Result<()> {
    let coordinator = &mut mem.flagship_coordinator;
    if coordinator.rooms.is_empty() { return Ok(()); }

    if scheduler.creeps().of_role(RoleSelector::Flagship).next().is_some() { return Ok(()); }

    run(|| {
        let name = scheduler.iter()
            .filter(ColonyScheduler::has_free)
            .max_by_key(ColonyScheduler::future_energy)
            .map(|candidate| candidate.name())
            .or_break()?;

        scheduler.get_mut(name).schedule(FLAGSHIP_TEMPLATE.clone(), CreepRole::Flagship(FlagshipCreep::default()))
    })?;

    Ok(())
}

fn get_tugboat_body(energy: u32, tugged: &Creep) -> Body {
    let tugged_body = Body::of_creep(tugged);
    let target_tugboat_move_parts = tugged_body.total_parts().saturating_sub(2 * tugged_body.part_count(Part::Move));

    let tugged_empty_carry = tugged.store().get_free_capacity(None).div_floor(50) as usize;
    let target_tugboat_move_parts = target_tugboat_move_parts.saturating_sub(tugged_empty_carry);

    if target_tugboat_move_parts == 0 {
        warn!("Creep {} has requested tugboat, but doesn't actually benefit from it", tugged.name());
    }

    Body::of_part(Part::Move, target_tugboat_move_parts.clamp(0, (energy / 50) as usize))
}

pub fn schedule_tugboats(scheduler: &mut ColonySchedulerMut<'_>, tugboat_requests: &TugboatRequests) -> anyhow::Result<()> {
    let tugged = scheduler.syndrome().tugged_order()
        .unwrap_or_else(|| {
            tugboat_requests.iter()
            .filter(|tugged| scheduler.home_creeps().contains(&tugged.id()))
            .cloned()
            .collect_vec()
        });

    for tugged in tugged {
        schedule_tugboat(scheduler, &tugged).into_result()?;
    }

    Ok(())
}

fn schedule_tugboat(scheduler: &mut ColonySchedulerMut<'_>, tugged: &Creep) -> Flow {
    break_if(!scheduler.has_free())?;
    break_if(scheduler.home_creeps().of_role(RoleSelector::TugboatFor(tugged.id())).next().is_some())?;

    scheduler.schedule_selected(
        |iter| {
            iter.min_by_key(|(_, spawn)| spawn.spawn.pos().get_range_to(tugged.pos()))
                .map(|(ix, _)| ix)
                .or_break()
        },
        get_tugboat_body(scheduler.future_energy(), tugged),
        CreepRole::Tugboat(tugged.id())
    )
}

const MAX_LEVEL_FOR_FABRICATOR_IMPORT: u8 = 4;

static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Carry, Move, Work, Carry]) });
pub fn schedule_fabricators(scheduler: &mut SpawnScheduler, mem: &mut Memory) -> anyhow::Result<()> {
    for colony in mem.colonies.view_all() {
        let coordinator = &mem.colonies.coordinators(colony.name).fabricators;

        let total_work_ticks = coordinator.total_build_work_ticks();

        run(|| {
            let reserved_work_ticks = scheduler.get(colony.name).home_creeps()
                .of_role(RoleSelector::Fabricator)
                .map(|(creep, proto)| {
                    creep.resolve().ticks_to_live().unwrap_or(CREEP_LIFE_TIME).min(FABRICATOR_HORIZON) * proto.body().part_count(Part::Work) as u32
                }).sum::<u32>();

            break_if(reserved_work_ticks >= total_work_ticks)?;

            let max_work = scheduler.get(colony.name).full_budget() as u32 / BUILD_POWER;
            let may_import = colony.step.controller_level() <= MAX_LEVEL_FOR_FABRICATOR_IMPORT;

            let spawn_name = scheduler.iter()
                .filter(|candidate| candidate.is_available() && (candidate.name() == colony.name || may_import))
                .max_by_key(|candidate| {
                    let dist = colony.center.get_range_to(mem.colonies.view(candidate.name()).center);
                    (candidate.future_energy(), -(dist as i32))
                })
                .map(|candidate| candidate.name())
                .or_break()?;

            let mut colony_scheduler = scheduler.get_mut(spawn_name);
            let body = FABRICATOR_TEMPLATE.scaled_with_stop(colony_scheduler.future_energy(), ScaleStopCondition::PartCount(Part::Work, max_work)).or_break()?;

            colony_scheduler.schedule_for(colony.name, body, CreepRole::Fabricator(FabricatorCreep::default()))
        })?;
    }

    Ok(())
}

static SERVICER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Move, Work]) });
pub fn schedule_servicers(scheduler: &mut ColonySchedulerMut<'_>, coordinator: &ServicerCoordinator) -> anyhow::Result<()> {
    if scheduler.syndrome().any_problems() { return Ok(()) }

    let total_work_ticks = coordinator.total_work_ticks();

    run(|| {
        let need_emergency_servicer = scheduler.home_creeps().part_count(RoleSelector::Servicer, Part::Work) == 0 && coordinator.any_emergency_repair();

        let body = SERVICER_TEMPLATE.scaled(scheduler.future_energy()).or_break()?;

        if !need_emergency_servicer {
            let reserved_work_ticks = scheduler.home_creeps().part_ticks_left_of_role(RoleSelector::Servicer, Part::Work);

            let work_power = body.part_count(Part::Work) as u32;
            let additional_reserved_work_ticks = body.part_count(Part::Work) as u32 * body.life_time();

            break_if(reserved_work_ticks + additional_reserved_work_ticks >= total_work_ticks)?;
            break_if(work_power as f32 > scheduler.full_budget())?;
        }

        scheduler.schedule(
            body,
            CreepRole::Servicer(ServicerCreep::default())
        )
    })
}

const BASELINE_UPGRADE_POWER: u32 = 2;
const TARGET_BUFFER_FULLNESS: f32 = 0.8;
fn calculate_target_upgrade_work(colony: &ColonyView<'_>, scheduler: &ColonySchedulerMut<'_>) -> u32 {
    let current_upgrade_power = scheduler.home_creeps().part_count(RoleSelector::Upgrader, Part::Work) as u32;

    let mut target_upgrade_power = match colony.buffer {
        Some(ColonyBuffer::Storage(storage)) => {
            let storage = storage.resolve();
            let buffer_fullness = storage.used_energy_capacity() as f32 / storage.energy_capacity() as f32;
            let budget = (current_upgrade_power as f32 + scheduler.sustainable_budget()) as u32;

            BASELINE_UPGRADE_POWER + ((buffer_fullness / TARGET_BUFFER_FULLNESS) * (budget.saturating_sub(BASELINE_UPGRADE_POWER)) as f32) as u32
        },
        None | Some(ColonyBuffer::Container(_)) => {
            ((current_upgrade_power as f32 + scheduler.full_budget()) as u32).max(BASELINE_UPGRADE_POWER)
        },
    };

    if colony.controller.level() == 8 { target_upgrade_power = target_upgrade_power.min(CONTROLLER_MAX_UPGRADE_PER_TICK) }

    target_upgrade_power
}

pub fn schedule_terraformer(scheduler: &mut ColonySchedulerMut<'_>, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    if scheduler.syndrome().any_problems() { return Ok(()); }
    if scheduler.home_creeps().any_of_role(RoleSelector::Terraformer) { return Ok(()); }

    let mut energy = scheduler.future_energy();
    energy = energy.saturating_sub(Part::Carry.cost());

    let work = energy.div_floor(Part::Work.cost()).min(calculate_target_upgrade_work(colony, scheduler));
    if work == 0 { return Ok(()) }

    scheduler.schedule(
        Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, work as usize),
        CreepRole::Terraformer
    ).into_result()?;

    Ok(())
}

static SEEDER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Work, Move, Work, Work]) });
pub fn schedule_seeders(scheduler: &mut ColonySchedulerMut<'_>, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    if scheduler.syndrome().any_problems() { return Ok(()); }
    if !scheduler.home_creeps().any_of_role(RoleSelector::Terraformer) { return Ok(()); }

    run(|| {
        let target_work = calculate_target_upgrade_work(colony, scheduler);
        let target_seeder_work = target_work.saturating_sub(scheduler.home_creeps().part_count(RoleSelector::Terraformer, Part::Work) as u32);
        let missing_seeder_work = target_seeder_work.saturating_sub(scheduler.home_creeps().part_count(RoleSelector::Seeder, Part::Work) as u32);

        let max_body = SEEDER_TEMPLATE.scaled(scheduler.future_energy()).or_break()?;
        let max_body_work = max_body.part_count(Part::Work);

        let small_seeder_work = scheduler.home_creeps()
            .of_role(RoleSelector::Seeder)
            .map(|(_, proto)| proto.body().part_count(Part::Work))
            .filter(|work_count| *work_count < max_body_work)
            .sum::<usize>() as u32;

        let any_small_seeders = small_seeder_work > 0;

        if any_small_seeders {
            let allowed_seeder_work = missing_seeder_work + small_seeder_work;
            break_if(allowed_seeder_work == 0)?;
            break_if(max_body_work as u32 > allowed_seeder_work)?;

            scheduler.schedule(
                max_body,
                CreepRole::Seeder
            )
        } else {
            break_if(missing_seeder_work == 0)?;
            let body = SEEDER_TEMPLATE.scaled_with_stop(scheduler.future_energy(), ScaleStopCondition::PartCount(Part::Work, missing_seeder_work)).or_break()?;

            scheduler.schedule(body, CreepRole::Seeder)
        }
    })
}
