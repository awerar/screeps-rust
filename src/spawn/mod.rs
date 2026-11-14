mod roster;
mod policies;

use std::fmt::Display;

use crate::{colony::{Colonies, ColonyView}, memory::Memory, movement::requests::TugboatRequests, spawn::{policies::{schedule_seeders, schedule_servicers, schedule_terraformer}, roster::ColonyRoster}};
use log::{error, info};
use policies::{schedule_excavators, schedule_fabricators, schedule_flagship, schedule_export_trucks, schedule_tugboats, schedule_trucks};
use roster::Rosters;

impl Rosters {
    fn for_roster<'a>(&mut self, colonies: &'a Colonies, mut f: impl FnMut(&mut ColonyRoster, ColonyView<'a>)) {
        for (colony, roster) in self.iter_mut() {
            f(roster, colonies.view(*colony));
        }
    }
}

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

#[expect(clippy::needless_pass_by_value)]
pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut rosters = Rosters::new(mem);

    let colonies = &mem.colonies;

    rosters.for_roster(colonies, |roster, colony| {
        let name = colony.name;
        info!("Budget for {name}: {}", roster.full_budget());

        schedule_tugboats(roster, &tugboat_requests).log_failure(format_args!("tugboats in {name}"));
        schedule_excavators(roster, &colony).log_failure(format_args!("excavators in {name}"));
        schedule_trucks(roster, &colony).log_failure(format_args!("trucks in {name}"));
        schedule_terraformer(roster, &colony).log_failure(format_args!("terraformer in {name}"));
        schedule_servicers(roster, &colonies.coordinators(name).servicers).log_failure(format_args!("servicers in {name}"));
    });


    schedule_fabricators(&mut rosters, mem).log_failure("fabricators");
    schedule_flagship(&mut rosters, mem).log_failure("flagships");

    rosters.for_roster(&mem.colonies, |roster, colony|
        schedule_seeders(roster, &colony).log_failure(format_args!("seeders in {}", colony.name)));

    schedule_export_trucks(&mut rosters, mem).log_failure("export trucks");

    rosters.gather_new_creeps(mem);
}
