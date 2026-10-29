use itertools::Itertools;
use screeps::{Position, Room, RoomName, look};
use anyhow::anyhow;

use crate::colony::plan::{ColonyPlanDiff, ColonyPlanStep, get_all_structures_in};

impl ColonyPlanStep {
    pub fn build(&self, room: &Room) -> anyhow::Result<bool> {
        for (xy, ty) in &self.expired_structures {
            let structures = room.look_for_at_xy(look::STRUCTURES, xy.x.u8(), xy.y.u8());
            if let Some(structure) = structures.into_iter().find(|structure| structure.structure_type() == *ty) {
                structure.as_structure().destroy()?;
            }
        }

        let all_structures = get_all_structures_in(room);
        let all_structures_keys = all_structures.clone().into_keys().collect();
        let unprocessed_new_structures = self.new_structures.difference(&all_structures_keys).collect_vec();

        for (pos, ty) in &unprocessed_new_structures {
            let pos = Position::new(pos.x, pos.y, room.name());
            if pos.look_for(look::CONSTRUCTION_SITES).ok().is_none_or(|sites| sites.is_empty()) {
                pos.create_construction_site(*ty, None).map_err(|e| anyhow!("Unable to create structure {ty} at {pos}: {e}"))?;
            }
        }

        let has_finished_step = self.new_structures.iter()
            .all(|new_structure|  {
                all_structures.get(new_structure).copied().unwrap_or(false)
            });
        Ok(has_finished_step)
    }
}

impl ColonyPlanDiff {
    pub fn migrate(self, room: RoomName) {
        for (pos, ty) in self.0 {
            let pos = Position::new(pos.x, pos.y, room);

            let structure = pos.look_for(look::STRUCTURES).unwrap().into_iter()
                .find(|structure| structure.structure_type() == ty);
            if let Some(structure) = structure { structure.as_structure().destroy().ok(); }

            let site = pos.look_for(look::CONSTRUCTION_SITES).unwrap().into_iter()
                .find(|site| site.structure_type() == ty);
            if let Some(site) = site { site.remove().ok(); }
        }
    }
}
