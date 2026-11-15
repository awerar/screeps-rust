use std::sync::LazyLock;

use itertools::Itertools;
use log::warn;
use ordered_float::OrderedFloat;
use screeps::{BUILD_POWER, CONTROLLER_MAX_UPGRADE_PER_TICK, CREEP_LIFE_TIME, Creep, HasPosition, Part, Source, UPGRADE_CONTROLLER_POWER};

use crate::{colony::{Colonies, ColonyBuffer, ColonyName, ColonyView, plan::SourcePlan, steps::ColonyStep}, creeps::{CreepRole, body::{Body, ScaleStopCondition}, fabricator::{FABRICATOR_HORIZON, FabricatorCreep}, flagship::FlagshipCreep, servicer::{ServicerCoordinator, ServicerCreep}, truck::{ExportTruckCreep, TruckCreep}}, domain_traits::{EnergyStoreAccessors, HasId, HasName, ResolvableId}, memory::Memory, movement::requests::TugboatRequests, spawn::scheduler::{ColonyScheduler, ColonySchedulerMut, ColonySchedulerRef, RoleSelector, SpawnScheduler}, statemachine::{Flow, OrBreak, break_if, run}};

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() {
        7
    } else {
        6
    };

    let excavator_works = energy.saturating_sub(Part::Carry.cost()).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, excavator_works as usize)
}

fn schedule_excavator(scheduler: &mut ColonySchedulerMut<'_>, source: &Source, source_plan: &SourcePlan) -> Flow {
    assert!(!scheduler.home_creeps().any_of_role(RoleSelector::SourceExcavator(source.id())));

    let body = get_excavator_body(scheduler.future_energy(), source_plan);
    let role = CreepRole::Excavator(source.id());

    if let Some(spawn) = source_plan.spawn.resolve() {
        scheduler.schedule_selected(spawn.id(), body, role)
    } else {
        scheduler.schedule(body, role)
    }
}

pub fn schedule_for_syndrome(scheduler: &mut ColonySchedulerMut<'_>, view: &ColonyView<'_>) -> anyhow::Result<()> {
    let result = run(|| {
        let syndrome = scheduler.syndrome();

        if syndrome.any_trucks {
            if let Some(excavator) = syndrome.get_tugboatless_excavator() {
                schedule_tugboat(scheduler, &excavator.resolve())
            } else if let Some(source) = syndrome.get_missing_excavator() {
                let source_plan = view.plan.sources.get(&source.screeps_id()).unwrap();
                schedule_excavator(scheduler, &source.resolve(), source_plan)
            } else {
                Flow::Break
            }
        } else {
            let body = get_truck_body(scheduler.future_energy()).expect("It should always be possible to construct a truck body from the future energy");
            scheduler.schedule(body, CreepRole::Truck(TruckCreep::default()))
        }
    });

    if scheduler.syndrome().any_problems() { scheduler.block_all(); }

    result
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

    let target_carry = ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY)).ceil() as usize;

    run(|| {
        break_if(scheduler.home_creeps().part_count(RoleSelector::Truck, Part::Carry) >= target_carry)?;

        let body = get_truck_body(scheduler.future_energy()).or_break()?;

        scheduler.schedule(
            body,
            CreepRole::Truck(TruckCreep::default())
        )
    })
}

const STOP_IMPORT_ENERGY_STEP: ColonyStep = ColonyStep::UpgradeToLevel5;

impl ColonyView<'_> {
    fn should_import_energy(&self) -> bool {
        self.step < STOP_IMPORT_ENERGY_STEP
    }
}

const IMPORT_BUDGET_LIMIT: f32 = 100.0;
const EXPORT_MIN_FREE_BUDGET: f32 = 10.0;

static EXPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Move, Carry]) });
pub fn schedule_export_trucks(scheduler: &mut SpawnScheduler, mem: &mut Memory) -> anyhow::Result<()> {
    for import_colony in mem.colonies.view_all() {
        if !import_colony.should_import_energy() { continue; }

        run(|| {
            break_if(scheduler.get(import_colony.name).budget().income() > IMPORT_BUDGET_LIMIT)?;

            let export_name = scheduler.iter()
                .filter(|candidate| candidate.spawns().any_free() && candidate.name() != import_colony.name)
                .filter(|candidate| matches!(mem.colonies.view(candidate.name()).buffer, Some(ColonyBuffer::Storage(_))))
                .map(|candidate| (candidate.name(), candidate.budget().general()))
                .filter(|(_, budget)| *budget > EXPORT_MIN_FREE_BUDGET)
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

    if scheduler.creeps().any_of_role(RoleSelector::Flagship) { return Ok(()); }

    run(|| {
        let name = scheduler.iter()
            .filter(|candidate| candidate.spawns().any_free())
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
    let tugged = tugboat_requests.iter()
        .filter(|tugged| scheduler.home_creeps().contains(&tugged.id()))
        .filter(|tugged| !scheduler.home_creeps().any_of_role(RoleSelector::TugboatFor(tugged.id())))
        .cloned()
        .collect_vec();

    for tugged in tugged {
        schedule_tugboat(scheduler, &tugged).into_result()?;
    }

    Ok(())
}

fn schedule_tugboat(scheduler: &mut ColonySchedulerMut<'_>, tugged: &Creep) -> Flow {
    let spawn_id = scheduler.spawns().free()
        .map(|(id, _)| id)
        .min_by_key(|id| id.resolve().pos().get_range_to(tugged.pos()))
        .or_break()?;

    scheduler.schedule_selected(
        spawn_id,
        get_tugboat_body(scheduler.future_energy(), tugged),
        CreepRole::Tugboat(tugged.id())
    )
}

impl SpawnScheduler {
    fn closest_creep_export_colony(&self, colony: ColonyName, colonies: &Colonies) -> Option<ColonyName> {
        let min_future_energy = self.get(colony).future_energy();
        let center = colonies.view(colony).center;

        self.iter()
            .filter(|candidate| candidate.spawns().any_free())
            .filter(|candidate| candidate.future_energy() >= min_future_energy)
            .min_by_key(|candidate| {
                center.get_range_to(colonies.view(candidate.name()).center)
            }).map(|candidate| candidate.name())
    }
}

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

            let spawn_name = if colony.should_import_energy() {
                scheduler.closest_creep_export_colony(colony.name, &mem.colonies).or_break()?
            } else {
                colony.name
            };

            let max_work = scheduler.get(colony.name).budget().general() as u32 / BUILD_POWER;
            let mut colony_scheduler = scheduler.get_mut(spawn_name);
            let body = FABRICATOR_TEMPLATE.scaled_with_stop(colony_scheduler.future_energy(), ScaleStopCondition::PartCount(Part::Work, max_work)).or_break()?;

            colony_scheduler.schedule_for(colony.name, body, CreepRole::Fabricator(FabricatorCreep::default()))
        })?;
    }

    Ok(())
}

static SERVICER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Move, Work]) });
pub fn schedule_servicers(scheduler: &mut ColonySchedulerMut<'_>, coordinator: &ServicerCoordinator) -> anyhow::Result<()> {
    let total_work_ticks = coordinator.total_work_ticks();

    run(|| {
        let need_emergency_servicer = !scheduler.home_creeps().any_of_role(RoleSelector::Servicer) && coordinator.any_emergency_repair();

        let body = SERVICER_TEMPLATE.scaled(scheduler.future_energy()).or_break()?;

        if !need_emergency_servicer {
            let reserved_work_ticks = scheduler.home_creeps().part_ticks_left_of_role(RoleSelector::Servicer, Part::Work);

            let work_power = body.part_count(Part::Work) as u32;
            let additional_reserved_work_ticks = body.part_count(Part::Work) as u32 * body.life_time();

            break_if(reserved_work_ticks + additional_reserved_work_ticks >= total_work_ticks)?;
            break_if(work_power as f32 > scheduler.budget().general())?;
        }

        scheduler.schedule(
            body,
            CreepRole::Servicer(ServicerCreep::default())
        )
    })
}

pub const BASELINE_UPGRADE_POWER: u32 = 2;
pub const BASELINE_UPGRADE_WORKS: u32 = BASELINE_UPGRADE_POWER.div_ceil(UPGRADE_CONTROLLER_POWER);
pub static BASELINE_TERRAFORMER_BODY: LazyLock<Body> = LazyLock::new(|| Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, BASELINE_UPGRADE_WORKS as usize));

const TARGET_BUFFER_FULLNESS: f32 = 0.8;
fn calculate_target_upgrade_works(colony: &ColonyView<'_>, scheduler: &ColonySchedulerRef<'_>) -> u32 {
    let budget_coeff = match colony.buffer {
        Some(ColonyBuffer::Storage(storage)) => {
            let storage = storage.resolve();
            let buffer_fullness = storage.used_energy_capacity() as f32 / storage.energy_capacity() as f32;

            buffer_fullness / TARGET_BUFFER_FULLNESS
        },
        None | Some(ColonyBuffer::Container(_)) => {
            1.0
        },
    };

    let mut target_works = BASELINE_UPGRADE_WORKS + (budget_coeff * scheduler.budget().upgrade()) as u32;
    if colony.controller.level() == 8 { target_works = target_works.min(CONTROLLER_MAX_UPGRADE_PER_TICK) }

    target_works
}

pub fn schedule_terraformer(scheduler: &mut ColonySchedulerMut<'_>, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    if scheduler.home_creeps().any_of_role(RoleSelector::Terraformer) { return Ok(()); }

    let baseline_works = scheduler.future_energy()
        .checked_sub(Part::Carry.cost())
        .map(|energy| (energy / Part::Work.cost()).min(BASELINE_UPGRADE_WORKS))
        .filter(|works| *works > 0)
        .expect("It should always be possible to spawn a terraformer");

    let baseline_body = Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, baseline_works as usize);

    let extra_energy = scheduler.future_energy() - baseline_body.energy_required();
    let target_extra_work = calculate_target_upgrade_works(colony, &scheduler.as_shared()) - baseline_works;
    let extra_work = target_extra_work.min(extra_energy / Part::Work.cost());

    scheduler.schedule(
        baseline_body + Body::of_part(Part::Work, extra_work as usize),
        CreepRole::Terraformer
    ).into_result()?;

    Ok(())
}

static SEEDER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Work, Move, Work, Work]) });
pub fn schedule_seeders(scheduler: &mut SpawnScheduler, colonies: &Colonies) -> anyhow::Result<()> {
    for colony_name in scheduler.colonies() {
        let colony = colonies.view(colony_name);
    
        run(|| {
            let colony_scheduler = scheduler.get(colony_name);
            let home_creeps = colony_scheduler.home_creeps();

            break_if(!home_creeps.any_of_role(RoleSelector::Terraformer))?;

            let target_work = calculate_target_upgrade_works(&colony, &colony_scheduler);
            let target_seeder_work = target_work.saturating_sub(home_creeps.part_count(RoleSelector::Terraformer, Part::Work) as u32);
            let missing_seeder_work = target_seeder_work.saturating_sub(home_creeps.part_count(RoleSelector::Seeder, Part::Work) as u32);
            let missing_seeder_work = missing_seeder_work.min(colony_scheduler.budget().seeder() as u32);

            break_if(missing_seeder_work == 0)?;
    
            let spawn_name = if colony.should_import_energy() {
                scheduler.closest_creep_export_colony(colony_name, colonies).or_break()?
            } else {
                colony_name
            };

            let mut spawn_scheduler = scheduler.get_mut(spawn_name);
    
            let body = SEEDER_TEMPLATE.scaled_with_stop(spawn_scheduler.future_energy(), ScaleStopCondition::PartCount(Part::Work, missing_seeder_work)).or_break()?;
            spawn_scheduler.schedule(body, CreepRole::Seeder)
        })?;
    }

    Ok(())
}
