use log::warn;
use screeps::{Position, StructureType, game, look};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use serde_json_any_key::*;

#[derive(Serialize, Deserialize)]
struct BuildData {
    structure_type: StructureType,
    progress: u32
}

#[derive(Serialize, Deserialize, Default)]
pub struct RemoteBuildRequests(#[serde(with = "any_key_map")] HashMap<Position, BuildData>);

impl RemoteBuildRequests {
    pub fn update_requests(&mut self) {
        let mut finished_requests = Vec::new();
        
        for (pos, build) in self.0.iter_mut() {
            if game::rooms().get(pos.room_name()).is_none() { continue; }

            let structure = pos.look_for(look::STRUCTURES).unwrap().into_iter()
                .filter(|structure| structure.structure_type() == build.structure_type)
                .next();

            if structure.is_some() {
                finished_requests.push(pos);
                continue;
            }

            let site = pos.look_for(look::CONSTRUCTION_SITES).unwrap().into_iter()
                .filter(|site| site.structure_type() == build.structure_type)
                .next();

            let Some(site) = site else {
                warn!("Remoted constructions site of {} at {pos} was unexpectedly removed", build.structure_type);
                finished_requests.push(pos);
                continue;
            };

            build.progress = site.progress();
        }
    }

    pub fn create_request(&mut self, pos: Position, structure_type: StructureType) -> Result<(), ()> {
        pos.create_construction_site(structure_type, None).map_err(|_| ())?;
        self.0.insert(pos, BuildData { structure_type, progress: 0 });

        Ok(())
    }
}