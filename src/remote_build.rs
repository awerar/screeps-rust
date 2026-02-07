use js_sys::JsString;
use log::warn;
use screeps::{ConstructionSite, Position, StructureType, game, look};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use serde_json_any_key::*;

#[derive(Serialize, Deserialize)]
pub struct BuildData {
    pub pos: Position,
    pub structure_type: StructureType,
    pub progress: u32
}

impl BuildData {
    pub fn site(&self) -> Option<ConstructionSite> {
        self.pos.look_for(look::CONSTRUCTION_SITES).unwrap().into_iter()
            .filter(|site| site.structure_type() == self.structure_type)
            .next()
    }
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
                finished_requests.push(pos.clone());
                continue;
            }

            let Some(site) = build.site() else {
                warn!("Remoted constructions site of {} at {pos} was unexpectedly removed", build.structure_type);
                finished_requests.push(pos.clone());
                continue;
            };

            build.progress = site.progress();
        }

        for request in finished_requests {
            self.0.remove(&request);
        }
    }

    pub fn create_request(&mut self, pos: Position, structure_type: StructureType, name: Option<&str>) -> Result<(), ()> {
        if let Some(build) = self.get_request_data(&pos) {
            if build.structure_type == structure_type {
                return Ok(())
            }
        }

        let build = BuildData { structure_type, progress: 0, pos };
        let already_sited = build.site().is_some();
        self.0.insert(pos, build);

        if already_sited { return Ok(()) }

        let name = name.map(|name| JsString::from(name));
        pos.create_construction_site(structure_type, name.as_ref()).map_err(|_| ())?;

        Ok(())
    }

    pub fn get_new_request(&self) -> Option<Position> {
        self.0.keys().next().cloned()
    }

    pub fn get_request_data(&self, pos: &Position) -> Option<&BuildData> {
        self.0.get(pos)
    }

    pub fn get_total_work_ticks(&self) -> u32 {
        self.0.values().map(|build| build.structure_type.construction_cost().unwrap() - build.progress).sum::<u32>() / 5
    }
}