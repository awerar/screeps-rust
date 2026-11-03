pub mod prototype;
mod energy;
mod roles;
mod roster;
mod policies;

use crate::{memory::Memory, movement::requests::TugboatRequests, spawn::policies::schedule_terraformer};
use policies::{schedule_excavators, schedule_fabricators, schedule_flagships, schedule_import_trucks, schedule_remote_fabricators, schedule_tugboats, schedule_trucks};
use roster::Rosters;

#[expect(clippy::needless_pass_by_value)]
pub fn do_spawns(mem: &mut Memory, tugboat_requests: TugboatRequests) {
    let mut rosters = Rosters::new(mem);

    for (colony, roster) in rosters.iter_mut() {
        let fabricator_coordinator = mem.fabricator_coordinators.entry(*colony).or_default();
        let view = mem.colonies.view(*colony).unwrap();

        schedule_excavators(roster, &view);
        schedule_tugboats(roster, &tugboat_requests);
        schedule_trucks(roster, &view);
        schedule_terraformer(roster, &view);
        schedule_fabricators(roster, fabricator_coordinator);
    }

    schedule_remote_fabricators(&mut rosters, mem);
    schedule_flagships(&mut rosters, mem);
    schedule_import_trucks(&mut rosters, mem);

    rosters.gather_new_creeps(mem);
}
