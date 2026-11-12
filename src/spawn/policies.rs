use std::sync::LazyLock;

use itertools::Itertools;
use log::warn;
use ordered_float::OrderedFloat;
use screeps::{BUILD_POWER, CONTROLLER_MAX_UPGRADE_PER_TICK, CREEP_LIFE_TIME, Creep, HasPosition, Part, Source};

use crate::{colony::{ColonyBuffer, ColonyView, plan::SourcePlan}, creeps::{CreepRole, body::{Body, ScaleStopCondition}, fabricator::FabricatorCreep, flagship::FlagshipCreep, servicer::{ServicerCoordinator, ServicerCreep}, truck::{ExportTruckCreep, TruckCreep}}, domain_traits::{EnergyStoreAccessors, HasId, HasName, ResolvableId}, memory::Memory, movement::requests::TugboatRequests, spawn::roster::{ColonyRoster, RoleSelector, Rosters}, statemachine::{Flow, OrBreak, break_if, run}};

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() {
        7
    } else {
        6
    };

    let excavator_works = energy.saturating_sub(Part::Carry.cost()).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, excavator_works as usize)
}

pub fn schedule_excavators(roster: &mut ColonyRoster, view: &ColonyView<'_>) -> anyhow::Result<()> {
    for (source, source_plan) in &view.plan.sources {
        let Some(source) = source.resolve() else { continue; };
        schedule_excavator(roster, &source, source_plan).into_result()?;
    }

    Ok(())
}

fn schedule_excavator(roster: &mut ColonyRoster, source: &Source, source_plan: &SourcePlan) -> Flow {
    break_if(!roster.has_free())?;
    break_if(roster.local_creeps().of_role(RoleSelector::SourceExcavator(source.id())).next().is_some())?;

    let has_source_spawn = source_plan.spawn.resolve().is_some();

    roster.schedule_selected(
        |mut iter| {
            if has_source_spawn {
                iter.find(|(_, spawn)| spawn.is_source_spawn(&source.id())).map(|(ix, _)| ix).or_break()
            } else {
                ColonyRoster::default_select(iter)
            }
        },
        get_excavator_body(roster.future_energy(), source_plan),
        CreepRole::Excavator(source.id())
    )
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

pub fn schedule_trucks(roster: &mut ColonyRoster, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    let total_carry_for_sources = colony.plan.sources.values()
        .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
        .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
        .sum::<f32>();

    let target_carry = if roster.syndrome().any_problems() {
        1
    } else {
        ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize
    };

    run(|| {
        break_if(roster.local_creeps().part_count(RoleSelector::Truck, Part::Carry) >= target_carry)?;

        let body = get_truck_body(roster.future_energy()).or_break()?;

        roster.schedule(
            body,
            CreepRole::Truck(TruckCreep::default())
        )
    })
}

const IMPORT_BUDGET_LIMIT: f32 = 100.0;
const EXPORT_BUDGET_LIMIT: f32 = 10.0;

static EXPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Move, Carry]) });
pub fn schedule_export_trucks(rosters: &mut Rosters, mem: &mut Memory) -> anyhow::Result<()> {
    for import_colony in mem.colonies.view_all() {
        if import_colony.buffer.is_some_and(|buffer| matches!(buffer, ColonyBuffer::Storage(_))) { continue; }

        run(|| {
            break_if(rosters.get(&import_colony).income() > IMPORT_BUDGET_LIMIT)?;

            rosters.schedule_selected(
                |iter| {
                    iter.filter(|(_, roster)| roster.has_free())
                        .filter_map(|(name, roster)| Some((mem.colonies.view(*name)?, roster.full_budget())))
                        .filter(|(export_colony, _)| export_colony.name != import_colony.name)
                        .filter(|(export_colony, _)| matches!(export_colony.buffer, Some(ColonyBuffer::Storage(_))))
                        .filter(|(_, budget)| *budget > EXPORT_BUDGET_LIMIT)
                        .max_by_key(|(_, budget)| OrderedFloat(*budget))
                        .map(|(room, _)| room.name)
                        .or_break()
                },
                |roster| EXPORT_TRUCK_TEMPLATE.scaled(roster.future_energy()).or_break(),
                |roster| {
                    CreepRole::ExportTruck(ExportTruckCreep::new(
                        &mem.colonies.view(roster.room()).unwrap(),
                        &import_colony
                    ))
                },
                None
            )
        })?;
    }

    Ok(())
}

static FLAGSHIP_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Claim, Move]) });
pub fn schedule_flagship(rosters: &mut Rosters, mem: &mut Memory) -> anyhow::Result<()> {
    let coordinator = &mut mem.flagship_coordinator;
    if coordinator.rooms.is_empty() { return Ok(()); }

    if rosters.global_creeps().of_role(RoleSelector::Flagship).count() > 0 { return Ok(()); }

    rosters.schedule(
        |_| Flow::Continue(FLAGSHIP_TEMPLATE.clone()),
        |_| CreepRole::Flagship(FlagshipCreep::default()),
        None
    ).into_result()?;

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

pub fn schedule_tugboats(roster: &mut ColonyRoster, tugboat_requests: &TugboatRequests) -> anyhow::Result<()> {
    let tugged = roster.syndrome().tugged_order()
        .unwrap_or_else(|| {
            tugboat_requests.iter()
            .filter(|tugged| roster.local_creeps().contains_key(&tugged.id()))
            .cloned()
            .collect_vec()
        });

    for tugged in tugged {
        schedule_tugboat(roster, &tugged).into_result()?;
    }

    Ok(())
}

fn schedule_tugboat(roster: &mut ColonyRoster, tugged: &Creep) -> Flow {
    break_if(!roster.has_free())?;
    break_if(roster.local_creeps().of_role(RoleSelector::TugboatFor(tugged.id())).next().is_some())?;

    roster.schedule_selected(
        |iter| {
            iter.min_by_key(|(_, spawn)| spawn.spawn.pos().get_range_to(tugged.pos()))
                .map(|(ix, _)| ix)
                .or_break()
        },
        get_tugboat_body(roster.future_energy(), tugged),
        CreepRole::Tugboat(tugged.id())
    )
}

const MAX_LEVEL_FOR_FABRICATOR_IMPORT: u8 = 4;

static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Carry, Move, Work, Carry]) });
pub fn schedule_fabricators(rosters: &mut Rosters, mem: &mut Memory) -> anyhow::Result<()> {
    for colony in mem.colonies.view_all() {
        let coordinator = mem.fabricator_coordinators.entry(colony.name).or_default();

        let total_work_ticks = coordinator.total_build_work_ticks();

        run(|| {
            let reserved_work_ticks = rosters.get(&colony).local_creeps().part_ticks_left_of_role(RoleSelector::Fabricator, Part::Work);

            break_if(reserved_work_ticks >= total_work_ticks)?;

            let max_work = rosters.get(&colony).full_budget() as u32 / BUILD_POWER;
            let may_import = colony.step.controller_level() <= MAX_LEVEL_FOR_FABRICATOR_IMPORT;

            rosters.schedule_selected(
                |iter| {
                    iter.filter(|(_, other_roster)| {
                        other_roster.room() == colony.name || (may_import && other_roster.has_free())
                    }).filter_map(|(other_name, other_roster)| {
                        Some((*other_name, other_roster, mem.colonies.view(*other_name)?))
                    }).max_by_key(|(_, other_roster, other_colony)| {
                        let dist = colony.center.get_range_to(other_colony.center);

                        (other_roster.future_energy(), -(dist as i32))
                    }).map(|(other_name, _, _)| other_name)
                    .or_break()
                },
                |roster| FABRICATOR_TEMPLATE.scaled_with_stop(roster.future_energy(), ScaleStopCondition::PartCount(Part::Work, max_work)).or_break(),
                |_| CreepRole::Fabricator(FabricatorCreep::default()),
                Some(colony.name)
            )
        })?;
    }

    Ok(())
}

static SERVICER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Move, Work]) });
pub fn schedule_servicers(roster: &mut ColonyRoster, coordinator: &ServicerCoordinator) -> anyhow::Result<()> {
    if roster.syndrome().any_problems() { return Ok(()) }

    let total_work_ticks = coordinator.total_work_ticks();

    run(|| {
        let need_emergency_servicer = roster.local_creeps().part_count(RoleSelector::Servicer, Part::Work) == 0 && coordinator.any_emergency_repair();

        let body = SERVICER_TEMPLATE.scaled(roster.future_energy()).or_break()?;

        if !need_emergency_servicer {
            let reserved_work_ticks = roster.local_creeps()
                .of_role(RoleSelector::Servicer)
                .map(|(creep , proto)| {
                    creep.resolve().ticks_to_live().unwrap_or(CREEP_LIFE_TIME) * proto.body().part_count(Part::Work) as u32
                }).sum::<u32>();

            let work_power = body.part_count(Part::Work) as u32;
            let additional_reserved_work_ticks = body.part_count(Part::Work) as u32 * body.life_time();

            break_if(reserved_work_ticks + additional_reserved_work_ticks >= total_work_ticks)?;
            break_if(work_power as f32 > roster.full_budget())?;
        }

        roster.schedule(
            body,
            CreepRole::Servicer(ServicerCreep::default())
        )
    })
}

const BASELINE_UPGRADE_POWER: u32 = 2;
const TARGET_BUFFER_FULLNESS: f32 = 0.8;
fn calculate_target_upgrade_work(colony: &ColonyView<'_>, roster: &ColonyRoster) -> u32 {
    let current_upgrade_power = roster.local_creeps().part_count(RoleSelector::Upgrader, Part::Work) as u32;

    let mut target_upgrade_power = match colony.buffer {
        Some(ColonyBuffer::Storage(storage)) => {
            let storage = storage.resolve();
            let buffer_fullness = storage.used_energy_capacity() as f32 / storage.energy_capacity() as f32;
            let budget = (current_upgrade_power as f32 + roster.sustainable_budget()) as u32;

            BASELINE_UPGRADE_POWER + ((buffer_fullness / TARGET_BUFFER_FULLNESS) * (budget.saturating_sub(BASELINE_UPGRADE_POWER)) as f32) as u32
        },
        None | Some(ColonyBuffer::Container(_)) => {
            ((current_upgrade_power as f32 + roster.full_budget()) as u32).max(BASELINE_UPGRADE_POWER)
        },
    };

    if colony.controller.level() == 8 { target_upgrade_power = target_upgrade_power.min(CONTROLLER_MAX_UPGRADE_PER_TICK) }

    target_upgrade_power
}

pub fn schedule_terraformer(roster: &mut ColonyRoster, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    if roster.syndrome().any_problems() { return Ok(()); }
    if roster.local_creeps().values().any(|proto| matches!(proto.role(), CreepRole::Terraformer)) { return Ok(()); }

    let mut energy = roster.future_energy();
    energy = energy.saturating_sub(Part::Carry.cost());

    let work = energy.div_floor(Part::Work.cost()).min(calculate_target_upgrade_work(colony, roster));
    if work == 0 { return Ok(()) }

    roster.schedule(
        Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, work as usize),
        CreepRole::Terraformer
    ).into_result()?;

    Ok(())
}

static SEEDER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Work, Move, Work, Work]) });
pub fn schedule_seeders(roster: &mut ColonyRoster, colony: &ColonyView<'_>) -> anyhow::Result<()> {
    if roster.syndrome().any_problems() { return Ok(()); }
    if !roster.local_creeps().values().any(|proto| matches!(proto.role(), CreepRole::Terraformer)) { return Ok(()); }

    run(|| {
        let target_work = calculate_target_upgrade_work(colony, roster);
        let target_seeder_work = target_work.saturating_sub(roster.local_creeps().part_count(RoleSelector::Terraformer, Part::Work) as u32);
        let missing_seeder_work = target_seeder_work.saturating_sub(roster.local_creeps().part_count(RoleSelector::Seeder, Part::Work) as u32);

        let max_body = SEEDER_TEMPLATE.scaled(roster.future_energy()).or_break()?;
        let max_body_work = max_body.part_count(Part::Work);

        let small_seeder_work = roster.local_creeps()
            .of_role(RoleSelector::Seeder)
            .map(|(_, proto)| proto.body().part_count(Part::Work))
            .filter(|work_count| *work_count < max_body_work)
            .sum::<usize>() as u32;

        let any_small_seeders = small_seeder_work > 0;

        if any_small_seeders {
            let allowed_seeder_work = missing_seeder_work + small_seeder_work;
            break_if(allowed_seeder_work == 0)?;
            break_if(max_body_work as u32 > allowed_seeder_work)?;

            roster.schedule(
                max_body,
                CreepRole::Seeder
            )
        } else {
            break_if(missing_seeder_work == 0)?;
            let body = SEEDER_TEMPLATE.scaled_with_stop(roster.future_energy(), ScaleStopCondition::PartCount(Part::Work, missing_seeder_work)).or_break()?;

            roster.schedule(body, CreepRole::Seeder)
        }
    })
}
