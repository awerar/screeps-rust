use std::sync::LazyLock;

use itertools::Itertools;
use log::warn;
use screeps::{BUILD_POWER, CONTROLLER_MAX_UPGRADE_PER_TICK, CREEP_LIFE_TIME, Creep, HARVEST_POWER, HasPosition, Part, UPGRADE_CONTROLLER_POWER};

use crate::{colony::{ColonyBuffer, ColonyView, plan::SourcePlan, steps::ColonyStep}, creeps::{CreepRole, excavator::ExcavatorCreep, fabricator::{FabricatorCoordinator, FabricatorCreep}, flagship::FlagshipCreep, seeder::SeederCreep, servicer::{ServicerCoordinator, ServicerCreep}, terraformer::TerraformerCreep, truck::{ImportTruckState, STOP_IMPORT_STEP, TruckCreep}}, domain_traits::{EnergyStoreAccessors, HasId, HasName, ResolvableId}, logging::LogResultErr, memory::Memory, movement::requests::TugboatRequests, spawn::{prototype::{Body, Prototype, RelativePrototype, ScaleStopCondition}, roles::RoleSelector, roster::{ColonyRoster, Rosters}}};

fn get_excavator_body(energy: u32, source_plan: &SourcePlan) -> Body {
    // TODO: Look over numbers here
    let target_excavator_works = if source_plan.get_construction_site().is_some() || source_plan.get_repair_target().is_some() { 
        7 
    } else { 
        5 
    };

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
            RelativePrototype::new(
                get_excavator_body(roster.future_energy(), source_plan),
                CreepRole::Excavator(ExcavatorCreep::default(), source.id())
            )
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

        let Some(body) = get_truck_body(roster.future_energy()) else { break };

        roster.schedule(
        RelativePrototype::new(
            body,
            CreepRole::Truck(TruckCreep::default())
        )
        ).log_err();
    }
}

static IMPORT_TRUCK_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Move, Carry]) });
pub fn schedule_import_trucks(rosters: &mut Rosters, mem: &mut Memory) {
    if mem.colonies.view_export_colonies().is_empty() { return; }

    for colony in mem.colonies.view_all() {
        if colony.step >= STOP_IMPORT_STEP { continue; }

        let roster = rosters.get(colony.name).unwrap();
        if roster.local_creeps().part_count(RoleSelector::ImportTruck, Part::Carry) > 100 {
            continue;
        }

        rosters.schedule(|roster| {
            Some(Prototype::absolute(
                IMPORT_TRUCK_TEMPLATE.scaled(roster.future_energy(), None)?,
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
            RelativePrototype::new(
                get_tugboat_body(roster.future_energy(), &tugged),
                CreepRole::Tugboat(tugged.id())
            )
            
        ).log_err();
    }
}

static FABRICATOR_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Carry, Move, Work, Carry]) });
pub fn schedule_fabricators(roster: &mut ColonyRoster, coordinator: &FabricatorCoordinator) {
    if roster.syndrome().any_problems() { return }

    let total_work_ticks = coordinator.total_build_work_ticks();
    
    while roster.has_free() {
        let Some(mut body) = FABRICATOR_TEMPLATE.scaled(roster.future_energy(), Some(4)) else { break };

        let reserved_work_ticks = roster.local_creeps()
            .of_role(RoleSelector::Fabricator)
            .map(|(creep , proto)| {
                creep.resolve().ticks_to_live().unwrap_or(CREEP_LIFE_TIME) * proto.body().part_count(Part::Work) as u32
            })
            .sum::<u32>();

        let work_power = body.part_count(Part::Work) as u32 * BUILD_POWER;

        if reserved_work_ticks >= total_work_ticks { break; }

        if work_power as f32 > roster.calculate_energy_budget() {
            let target_work = (roster.calculate_energy_budget() as u32).div_floor(BUILD_POWER);
            if target_work == 0 { break; }

            let Some(new_body) = FABRICATOR_TEMPLATE.scaled_with_stop(roster.future_energy(), Some(4), ScaleStopCondition::PartCount(Part::Work, target_work)) else { break; };
            body = new_body;
        }

        roster.schedule(
            RelativePrototype::new(
                body,
                CreepRole::Fabricator(FabricatorCreep::default())
            )
        ).log_err();
    }
}

pub fn schedule_servicers(roster: &mut ColonyRoster, coordinator: &ServicerCoordinator) {
    if roster.syndrome().any_problems() { return }

    let total_work_ticks = coordinator.total_work_ticks();
    
    while roster.has_free() {
        let need_emergency_servicer = roster.local_creeps().part_count(RoleSelector::Servicer, Part::Work) == 0 && coordinator.any_emergency_repair();

        let Some(body) = FABRICATOR_TEMPLATE.scaled(roster.future_energy(), Some(4)) else { break };

        if !need_emergency_servicer {
            let reserved_work_ticks = roster.local_creeps()
                .of_role(RoleSelector::Servicer)
                .map(|(creep , proto)| {
                    creep.resolve().ticks_to_live().unwrap_or(CREEP_LIFE_TIME) * proto.body().part_count(Part::Work) as u32
                }).sum::<u32>();

            let work_power = body.part_count(Part::Work) as u32;
            let additional_reserved_work_ticks = body.part_count(Part::Work) as u32 * body.life_time();
    
            if reserved_work_ticks + additional_reserved_work_ticks >= total_work_ticks { break; }
            if work_power as f32 > roster.calculate_energy_budget() { break; }
        }

        roster.schedule(
            RelativePrototype::new(
                body,
                CreepRole::Servicer(ServicerCreep::default())
            )
        ).log_err();
    }
}

pub fn schedule_remote_fabricators(rosters: &mut Rosters, mem: &mut Memory) {
    for colony in mem.colonies.view_all() {
        if !matches!(colony.step, ColonyStep::BuildSpawn) { continue; }

        let roster = rosters.get(colony.name).unwrap();
        if roster.local_creeps().of_role(RoleSelector::Fabricator).next().is_some() { continue; }

        rosters.schedule(|roster| {
            Some(Prototype::absolute(
                FABRICATOR_TEMPLATE.scaled(roster.future_energy(), None)?,
                CreepRole::Fabricator(FabricatorCreep::default()),
                colony.name
            ))
        }).log_err();
    }
}

const BASELINE_UPGRADE_POWER: u32 = 2;
const TARGET_BUFFER_FULLNESS: f32 = 0.8;
fn calculate_target_upgrade_work(colony: &ColonyView<'_>, roster: &ColonyRoster) -> u32 {
    let current_upgrade_power = roster.local_creeps().part_count(RoleSelector::Upgrader, Part::Work) as u32;

    let mut target_upgrade_power = match colony.buffer {
        Some(ColonyBuffer::Storage(storage)) => {
            let storage = storage.resolve();
            let buffer_fullness = storage.used_energy_capacity() as f32 / storage.energy_capacity() as f32;
            let budget = (current_upgrade_power as f32 + roster.calculcate_creep_energy_budget()) as u32;

            BASELINE_UPGRADE_POWER + ((buffer_fullness / TARGET_BUFFER_FULLNESS) * (budget.saturating_sub(BASELINE_UPGRADE_POWER)) as f32) as u32
        },
        None | Some(ColonyBuffer::Container(_)) => {
            (current_upgrade_power as f32 + roster.calculate_energy_budget()) as u32
        },
    };

    if colony.controller.level() == 8 { target_upgrade_power = target_upgrade_power.min(CONTROLLER_MAX_UPGRADE_PER_TICK) }

    target_upgrade_power
}

pub fn schedule_terraformer(roster: &mut ColonyRoster, colony: &ColonyView<'_>) {
    if roster.syndrome().any_problems() { return; }
    if roster.local_creeps().values().any(|proto| matches!(proto.role(), CreepRole::Terraformer(_))) { return; }

    let mut energy = roster.future_energy();
    energy = energy.saturating_sub(Part::Carry.cost());

    let work = energy.div_floor(Part::Work.cost()).min(calculate_target_upgrade_work(colony, roster));
    if work == 0 { return }

    roster.schedule(RelativePrototype::new(
        Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, work as usize), 
            CreepRole::Terraformer(TerraformerCreep::default())
        )
    ).log_err();
}

static SEEDER_TEMPLATE: LazyLock<Body> = LazyLock::new(|| { use Part::*; Body::from(vec![Carry, Work, Move, Work, Work]) });
pub fn schedule_seeders(roster: &mut ColonyRoster, colony: &ColonyView<'_>) {
    if roster.syndrome().any_problems() { return; }
    if !roster.local_creeps().values().any(|proto| matches!(proto.role(), CreepRole::Terraformer(_))) { return; }

    while roster.has_free() {
        let target_work = calculate_target_upgrade_work(colony, roster);
        let target_seeder_work = target_work.saturating_sub(roster.local_creeps().part_count(RoleSelector::Terraformer, Part::Work) as u32);
        let missing_seeder_work = target_seeder_work.saturating_sub(roster.local_creeps().part_count(RoleSelector::Seeder, Part::Work) as u32);

        let Some(max_body) = SEEDER_TEMPLATE.scaled(roster.future_energy(), Some(3)) else { break; };
        let max_body_work = max_body.part_count(Part::Work);

        let small_seeder_work = roster.local_creeps()
            .of_role(RoleSelector::Seeder)
            .map(|(_, proto)| proto.body().part_count(Part::Work))
            .filter(|work_count| *work_count < max_body_work)
            .sum::<usize>() as u32;

        let any_small_seeders = small_seeder_work > 0;

        if any_small_seeders {
            let allowed_seeder_work = missing_seeder_work + small_seeder_work;
            if allowed_seeder_work == 0 { break; }

            if max_body_work as u32 <= allowed_seeder_work {
                roster.schedule(
                    RelativePrototype::new(
                        max_body,
                        CreepRole::Seeder(SeederCreep::default()) 
                    )
                ).log_err();
            }
        } else {
            if missing_seeder_work == 0 { break; }
            let Some(body) = SEEDER_TEMPLATE.scaled_with_stop(roster.future_energy(), Some(3), ScaleStopCondition::PartCount(Part::Work, missing_seeder_work)) else { break };

            roster.schedule(RelativePrototype::new(body, CreepRole::Seeder(SeederCreep::default()))).log_err();
        }
    }
}

impl ColonyRoster {
    fn calculcate_creep_energy_budget(&self) -> f32 {
        self.local_creeps().values().map(|proto| {
            let body_drain = proto.body().energy_required() as f32 / proto.body().life_time() as f32;
            
            // TODO: Make more accurate
            let work_count = proto.body().part_count(Part::Work) as i32;
            let role_contribution = match proto.role() {
                CreepRole::Excavator(_, _) => (work_count * HARVEST_POWER as i32).min(10),
                CreepRole::Fabricator(_) => -work_count * BUILD_POWER as i32,
                CreepRole::Servicer(_) => -work_count,
                CreepRole::Terraformer(_) | CreepRole::Seeder(_) => 
                    -work_count * UPGRADE_CONTROLLER_POWER as i32,

                CreepRole::Flagship(_) | CreepRole::Truck(_) 
                | CreepRole::ImportTruck(_) | CreepRole::Tugboat(_) 
                | CreepRole::Scrap(_) => 0,
            };

            role_contribution as f32 - body_drain
        }).sum()
    }

    fn calculate_energy_budget(&self) -> f32 {
        let storage_budget = self.buffer_energy() as f32 / 1500.0;
        
        self.calculcate_creep_energy_budget() + storage_budget
    }
}