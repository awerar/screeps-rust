use std::collections::HashSet;

use screeps::{Flag, HasPosition, OwnedStructureProperties, Room, RoomName, StructureController, StructureObject, StructureSpawn, game};
use serde::{Deserialize, Serialize};
use log::*;

use crate::{colony::planning::{plan::ColonyPlan, steps::ColonyState}, memory::Memory};

pub mod planning;

#[derive(Serialize, Deserialize)]
pub struct ColonyData {
    pub room_name: RoomName,
    pub plan: ColonyPlan,
    pub state: ColonyState
}

impl ColonyData {
    pub fn room(&self) -> Option<Room> {
        game::rooms().get(self.room_name)
    }

    pub fn controller(&self) -> Option<StructureController> {
        self.room()?.controller()
    }

    pub fn level(&self) -> u8 {
        self.controller().map(|controller| controller.level()).unwrap_or(0)
    }

    pub fn buffer_structure(&self) -> Option<StructureObject> {
        /*self.buffer_pos.look_for(look::STRUCTURES).ok()?.into_iter()
            .filter(|structure| matches!(structure, StructureObject::StructureStorage(_) | StructureObject::StructureContainer(_)))
            .next()*/
        todo!()
    }

    pub fn spawn(&self) -> Option<StructureSpawn> {
        /*self.center.look_for(look::STRUCTURES).ok()?.into_iter()
            .filter_map(|structure| structure.try_into().ok())
            .next()*/
        todo!()
    }

    fn try_construct_from(name: RoomName) -> Option<Self> {
        /*let center = game::rooms().get(name).and_then(|room| {
            room.find(find::MY_SPAWNS, None).into_iter()
                .sorted_by_key(|spawn| spawn.name())
                .find_or_first(|spawn| spawn.name().starts_with("Center"))
                .map(|spawn| spawn.pos())
        }).or_else(|| {
            find_claim_flags().into_iter()
                .map(|flag| flag.pos())
                .filter(|pos| pos.room_name() == name)
                .next()
        });

        let Some(center) = center else { return None; };

        let buffer_pos = game::rooms().get(name).and_then(|room| {
            let structures = room.find(find::MY_STRUCTURES, None).into_iter()
                .map(|structure| (structure.pos(), structure.structure_type()));

            let sites = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
                .map(|site| (site.pos(), site.structure_type()));

            structures.chain(sites)
                .filter(|(pos, ty)| {
                    match ty {
                        StructureType::Storage => true,
                        StructureType::Container => pos.get_range_to(center) == 1,
                        _ => false
                    }
                }).next()
                .map(|(pos, _)| pos)
        }).unwrap_or_else(|| {
            let mut terrain = RoomTerrain::new(name).unwrap();
            let mut dir = Direction::BottomRight;
            for _ in 0..4 {
                let candidate = center + dir;
                if terrain.get_xy(candidate.xy()) != Terrain::Wall {
                    return candidate;
                }

                dir = dir.multi_rot(2);
            }

            unreachable!();
        });

        Some(Self { room_name: name, center, buffer_pos, state: Default::default()  })*/

        todo!()
    }
}

const CLAIM_FLAG_PREFIX: &str = "Claim";
fn find_claim_flags() -> Vec<Flag> {
    game::flags().entries()
        .filter(|(name, _)| name.starts_with(CLAIM_FLAG_PREFIX))
        .map(|(_, flag)| flag)
        .collect()
}

pub fn update_rooms(mem: &mut Memory) {
    info!("Updating rooms...");

    let owned_rooms = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name);


    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = mem.colonies.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        mem.colonies.remove(room);
        warn!("Lost room {}", room);
    }

    for name in curr_rooms {
        if !mem.colonies.contains_key(&name) {
            let Some(colony) = ColonyData::try_construct_from(name) else {
                error!("Unable to construct colony config for {name}");
                continue; 
            };
            mem.colonies.insert(name, colony);
        }

        let state = mem.colonies[&name].state.clone();
        mem.colonies.get_mut(&name).unwrap().state = state.update(name, mem, 0);
    }
}