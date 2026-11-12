mod roster;
mod policies;

use crate::{colony::{Colonies, ColonyView}, memory::Memory, movement::requests::TugboatRequests, spawn::{policies::{schedule_seeders, schedule_servicers, schedule_terraformer}, roster::ColonyRoster}};
use log::info;
use policies::{schedule_excavators, schedule_fabricators, schedule_flagships, schedule_export_trucks, schedule_tugboats, schedule_trucks};
use roster::Rosters;

impl Rosters {
    fn for_roster<'a>(&mut self, colonies: &'a Colonies, mut f: impl FnMut(&mut ColonyRoster, ColonyView<'a>)) {
        for (colony, roster) in self.iter_mut() {
            f(roster, colonies.view(*colony).unwrap());
        }
    }
}

#[expect(clippy::needless_pass_by_value)]
pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut rosters = Rosters::new(mem);

    let colonies = &mem.colonies;

    rosters.for_roster(colonies, |roster, colony| {
        info!("Budget for {}: {}", colony.name, roster.full_budget());

        schedule_tugboats(roster, &tugboat_requests);
        schedule_excavators(roster, &colony);
        schedule_trucks(roster, &colony);
        schedule_terraformer(roster, &colony);
        schedule_servicers(roster, mem.servicer_coordinators.entry(colony.name).or_default());
    });


    schedule_fabricators(&mut rosters, mem);
    schedule_flagships(&mut rosters, mem);

    rosters.for_roster(&mem.colonies, |roster, colony| 
        schedule_seeders(roster, &colony));

    schedule_export_trucks(&mut rosters, mem);

    rosters.gather_new_creeps(mem);
}
