use std::sync::LazyLock;

use itertools::Itertools;
use log::warn;
use screeps::{Creep, HasPosition, Part};

use crate::{colony::{ColonyView, plan::{SourcePlan, refs::ResolvableStructureRef}, steps::ColonyStep}, creeps::{CreepRole, excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::{ImportTruckState, TruckCreep, STOP_IMPORT_STEP}}, domain_traits::{EnergyStoreAccessors, HasId, HasName}, logging::LogResultErr, memory::Memory, movement::requests::TugboatRequests, spawn::{prototype::{Body, Prototype, RelativePrototype}, roles::RoleSelector, roster::{ColonyRoster, Rosters}}};

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    let target_excavator_works = if source_plan.get_construction_site().is_some() { 7 } else { 5 };
    let excavator_works = energy.saturating_sub(Part::Carry.cost()).div_floor(Part::Work.cost()).min(target_excavator_works);
    Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, excavator_works as usize)
}

pub fn schedule_excavators(roster: &mut ColonyRoster, view: &ColonyView<'_>) {
    for (source, source_plan) in &view.plan.sources {
        let Some(source) = source.resolve() else { continue; };
        if !roster.has_free() { continue; }
        if roster.local_creeps().of_role(RoleSelector::SourceExcavator(source.id())).next().is_some() { continue; }

        let has_source_spawn = source_plan.spawn.resolve().is_some();

        roster.schedule_selected(
            |mut iter| {
                if has_source_spawn {
                    iter.find(|(_, spawn)| spawn.is_source_spawn(&source.id())).map(|(ix, _)| ix)
                } else {
                    ColonyRoster::default_select(iter)
                }
            },
            |info| {
                Some(RelativePrototype::new(
                    get_excavator_body(info.future_energy, source_plan),
                    CreepRole::Excavator(ExcavatorCreep::default(), source.id())
                ))
            }
        ).log_err();
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
    TRUCK_TEMPLATE.scaled(energy.min(*MAX_TRUCK_ENERGY), Some(2))
}

pub fn schedule_trucks(roster: &mut ColonyRoster, colony: &ColonyView<'_>) {
    let total_carry_for_sources = colony.plan.sources.values()
        .filter(|source_plan| !source_plan.link.is_complete() && source_plan.container.is_complete())
        .map(|source_plan| source_plan.distance as f32 * TRUCK_SOURCE_CARRY_PER_DIST)
        .sum::<f32>();

    let target_carry = if roster.syndrome().any_problems() {
        1
    } else {
        ((1.0 + TRUCK_CARRY_MARGIN) * (total_carry_for_sources + TRUCK_CENTER_CARRY + TRUCK_FABRICATOR_CARRY)).ceil() as usize
    };

    while roster.has_free() {
        if roster.local_creeps().part_count(RoleSelector::Truck, Part::Carry) >= target_carry { break; }

        roster.schedule(|info| {
            Some(RelativePrototype::new(
                get_truck_body(info.future_energy)?,
                CreepRole::Truck(TruckCreep::default())
            ))
        }).log_err();
    }
}

static IMPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Move, Carry]) });
pub fn schedule_import_trucks(rosters: &mut Rosters, mem: &mut Memory) {
    for colony in mem.colonies.view_all() {
        if colony.step >= STOP_IMPORT_STEP { continue; }

        let roster = rosters.get(colony.name).unwrap();
        if roster.local_creeps().part_count(RoleSelector::ImportTruck, Part::Carry) > 100 {
            continue;
        }

        rosters.schedule(|info| {
            Some(Prototype::absolute(
                IMPORT_TRUCK_TEMPLATE.scaled(info.future_energy, None)?,
                CreepRole::ImportTruck(ImportTruckState::default()),
                colony.name
            ))
        }).log_err();
    }
}

static FLAGSHIP_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Claim, Move]) });
pub fn schedule_flagships(rosters: &mut Rosters, mem: &mut Memory) {
    let coordinator = &mut mem.flagship_coordinator;
    if coordinator.rooms.is_empty() { return; }

    if rosters.global_creeps().of_role(RoleSelector::Flagship).count() > 0 { return; }

    rosters.schedule(|_| {
        Some(Prototype::relative(
            FLAGSHIP_TEMPLATE.clone(),
            CreepRole::Flagship(FlagshipCreep::default())
        ))
    }).log_err();
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

pub fn schedule_tugboats(roster: &mut ColonyRoster, tugboat_requests: &TugboatRequests) {
    let tugged = roster.syndrome().tugged_order()
        .unwrap_or_else(|| {
            tugboat_requests.iter()
            .filter(|tugged| roster.local_creeps().contains_key(&tugged.id()))
            .cloned()
            .collect_vec()
        });

    for tugged in tugged {
        if !roster.has_free() { continue; }
        if roster.local_creeps().of_role(RoleSelector::TugboatFor(tugged.id())).next().is_some() { continue; }

        roster.schedule_selected(
            |iter| {
                iter.min_by_key(|(_, spawn)| spawn.spawn.pos().get_range_to(tugged.pos()))
                    .map(|(ix, _)| ix)
            },
            |info| {
                Some(RelativePrototype::new(
                    get_tugboat_body(info.future_energy, &tugged),
                    CreepRole::Tugboat(tugged.id(), info.spawn.id())
                ))
            }
        ).log_err();
    }
}

const TARGET_IDLE_FABRICATOR_WORK_COUNT: usize = 20;
const TARGET_SURPLUS_FABRICATOR_WORK_COUNT: usize = 40;
const BUFFER_ENERGY_SURPLUS_THRESHOLD: u32 = 50_000;
static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Carry, Move, Work, Carry]) });
pub fn schedule_fabricators(roster: &mut ColonyRoster, colony: &ColonyView<'_>) {
    if roster.syndrome().any_problems() { return }

    let buffer_energy = colony.buffer.map_or(0, |buffer| buffer.used_energy_capacity());
    let work_target = if buffer_energy >= BUFFER_ENERGY_SURPLUS_THRESHOLD { TARGET_SURPLUS_FABRICATOR_WORK_COUNT } else { TARGET_IDLE_FABRICATOR_WORK_COUNT };

    while roster.has_free() {
        if roster.local_creeps().part_count(RoleSelector::Fabricator, Part::Work) >= work_target { break; }

        roster.schedule(|info| {
            Some(RelativePrototype::new(
                FABRICATOR_TEMPLATE.scaled(info.future_energy, None)?,
                CreepRole::Fabricator(FabricatorCreep::default())
            ))
        }).log_err();
    }
}

pub fn schedule_remote_fabricators(rosters: &mut Rosters, mem: &mut Memory) {
    for colony in mem.colonies.view_all() {
        if !matches!(colony.step, ColonyStep::BuildSpawn) { continue; }

        let roster = rosters.get(colony.name).unwrap();
        if roster.local_creeps().of_role(RoleSelector::Fabricator).next().is_some() { continue; }

        rosters.schedule(|info| {
            Some(Prototype::absolute(
                FABRICATOR_TEMPLATE.scaled(info.future_energy, None)?,
                CreepRole::Fabricator(FabricatorCreep::default()),
                colony.name
            ))
        }).log_err();
    }
}
