use log::error;
use screeps::{StructureObject, StructureTower, find, game, prelude::*};

const FIX_THRESHOLD: f32 = 0.35;

pub fn do_towers() {
    for structure in game::structures().values() {
        if let StructureObject::StructureTower(tower)  = structure {
            do_tower(&tower);
        }
    }
}

fn do_tower(tower: &StructureTower) -> Option<()> {
    let room = tower.room()?;

    let hostile_creeps = room.find(find::HOSTILE_CREEPS, None);
    let attack_creep = hostile_creeps.into_iter()
        .min_by_key(|creep| tower.pos().get_range_to(creep.pos()));
    if let Some(attack_creep) = attack_creep {
        match tower.attack(&attack_creep) {
            Ok(()) => return Some(()),
            Err(e) => error!("Tower is unable to attack: {e}")
        }
    }

    let friendly_creeps = room.find(find::MY_CREEPS, None);
    let heal_creep = friendly_creeps.into_iter()
        .filter(|creep| creep.hits() < (creep.hits_max() as f32 * FIX_THRESHOLD) as u32)
        .min_by_key(|creep| tower.pos().get_range_to(creep.pos()));
    if let Some(heal_creep) = heal_creep {
        match tower.heal(&heal_creep) {
            Ok(()) => return Some(()),
            Err(e) => error!("Tower is unable to heal: {e}")
        }
    }

    let structures = room.find(find::STRUCTURES, None);
    let repairable = structures.iter()
        .filter_map(|structure| structure.as_repairable().map(|repairable| (repairable, structure)))
        .filter(|(repairable, _)| repairable.hits() < (repairable.hits_max() as f32 * FIX_THRESHOLD) as u32)
        .min_by_key(|(_, structure)| tower.pos().get_range_to(structure.pos()))
        .map(|(repairable, _)| repairable);
    if let Some(repairable) = repairable {
        match tower.repair(repairable) {
            Ok(()) => return Some(()),
            Err(e) => error!("Tower is unable to repair: {e}")
        }
    }

    Some(())
}