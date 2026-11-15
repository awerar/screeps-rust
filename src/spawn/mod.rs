mod scheduler;
mod policies;

use std::fmt::Display;

use crate::{memory::Memory, movement::requests::TugboatRequests, spawn::policies::{schedule_for_syndrome, schedule_seeders, schedule_servicers, schedule_terraformer}};
use log::{error, info};
use policies::{schedule_fabricators, schedule_flagship, schedule_export_trucks, schedule_tugboats, schedule_trucks};
use scheduler::SpawnScheduler;

trait LogFailure {
    fn log_failure(self, scope: impl Display);
}

impl LogFailure for anyhow::Result<()> {
    fn log_failure(self, scope: impl Display) {
        if let Err(e) = self {
            error!("Spawning {scope} failed: {e:#}");
        }
    }
}

/*

How spawning is handled:
If a syndrome is detected, special spawning policy is used for colony
If a syndrome persists, spawning is blocked for the rest of tick

Critical creeps are spawned first, regardless of budget:
    Tugboats
    Trucks

Reserve baseline upgrader budget

*/

#[expect(clippy::needless_pass_by_value)]
pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut scheduler = SpawnScheduler::new(mem);

    let colonies = &mem.colonies;

    for name in scheduler.colonies() {
        let scheduler = &mut scheduler.get_mut(name);
        let colony = colonies.view(name);

        let budget = scheduler.budget();
        info!("{name} budgets: income = +{}, general = {}, terraformer = {}, seeder = {}", budget.income(), budget.general(), budget.upgrade(), budget.seeder());

        schedule_for_syndrome(scheduler, &colony).log_failure(format_args!("for syndrome in {name}"));

        schedule_tugboats(scheduler, &tugboat_requests).log_failure(format_args!("tugboats in {name}"));
        schedule_trucks(scheduler, &colony).log_failure(format_args!("trucks in {name}"));
        schedule_terraformer(scheduler, &colony).log_failure(format_args!("terraformer in {name}"));
        schedule_servicers(scheduler, &colonies.coordinators(name).servicers).log_failure(format_args!("servicers in {name}"));
    }

    schedule_fabricators(&mut scheduler, mem).log_failure("fabricators");
    schedule_flagship(&mut scheduler, mem).log_failure("flagships");
    schedule_export_trucks(&mut scheduler, mem).log_failure("export trucks");

    schedule_seeders(&mut scheduler, &mem.colonies).log_failure(format_args!("seeders"));

    scheduler.gather_new_creeps(mem);
}
