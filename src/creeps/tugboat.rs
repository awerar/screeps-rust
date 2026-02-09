use log::*;
use screeps::{Creep, HasId, HasPosition, MaybeHasId, ObjectId, Position, SharedCreepProperties, StructureSpawn, action_error_codes::CreepMoveToErrorCode, find, game};
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepData, CreepRole, CreepState, transition}, memory::Memory};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub enum TuggedState {
    #[default]
    Requesting,
    WaitingFor { tugboat: String },
    GettingTugged(ObjectId<Creep>),
    Arrived
}

impl CreepState for TuggedState {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use TuggedState::*;

        match self {
            Requesting => {  }, // Progressed by spawning system
            WaitingFor { tugboat } => {
                let Some(tugboat) = game::creeps().get(tugboat.clone()) else { return Ok(Requesting); };
                let Some(tugboat_data) = mem.creeps.get(&tugboat.name()) else  { return Ok(Requesting) };
                
                let creep_id = creep.try_id().unwrap();
                if tugboat_data.role.master().map_or(false, |master| master == creep_id) {
                    return Ok(GettingTugged(tugboat.try_id().unwrap()))
                }
            },
            GettingTugged(tugboat) => { // Progressed by tugboat
                if tugboat.resolve().is_none() {
                    return Ok(Requesting)
                };
            },
            Arrived => {  },
        }

        Ok(self.clone())
    }
}

impl TuggedState {
    fn move_tugged_to(&mut self, tugged: &Creep, mem: &mut Memory, target: Position, range: u32) -> Result<(), ()> {
        *self = transition(self, tugged, mem);

        let TuggedState::GettingTugged(tugboat) = self else { return Ok(()) };
        let tugboat = tugboat.resolve().unwrap();

        if tugged.pos().get_range_to(target) <= range {
            let new_tugstate = TugboatState::Recycling(get_recycle_spawn(&tugboat, mem)?);
            
            let tugboat_role = &mut mem.creeps.get_mut(&tugboat.name()).ok_or(())?.role.clone();
            let CreepRole::Tugboat(tugstate@TugboatState::Tugging, _) = tugboat_role else { return Err(()) };

            *tugstate = new_tugstate;
            *self = TuggedState::Arrived;
            return Ok(())
        } else if tugboat.pos() == target {
            let new_tugstate = TugboatState::Recycling(get_recycle_spawn(&tugboat, mem)?);
            
            let tugboat_role = &mut mem.creeps.get_mut(&tugboat.name()).ok_or(())?.role.clone();
            let CreepRole::Tugboat(tugstate@TugboatState::Tugging, _) = tugboat_role else { return Err(()) };

            *tugstate = transition(&new_tugstate, &tugboat, mem);
            tugged.move_pulled_by(&tugboat).map_err(|_| ());
            return Ok(())
        }

        match tugboat.move_to(target) {
            Ok(()) => {
                tugged.move_pulled_by(&tugboat).map_err(|_| ())
            },
            Err(CreepMoveToErrorCode::Tired) => Ok(()),
            Err(_) => Err(())
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone)]
pub enum TugboatState {
    #[default]
    GoingTo,
    Tugging,
    Recycling(ObjectId<StructureSpawn>)
}

impl CreepState for TugboatState {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use TugboatState::*;

        let Some(CreepData { role: CreepRole::Tugboat(_, tugged), .. }) = mem.creep(creep) else { return Err(()) };
        let Some(client) = tugged.resolve() else {
            warn!("Client doesn't exist. Recycling tugboat");
            return Ok(Recycling(get_recycle_spawn(creep, mem)?));
        };

        match self {
            GoingTo => {
                if creep.pos().is_near_to(client.pos()) {
                    return Ok(Tugging { last_active_tick: game::time() });
                }

                mem.movement.smart_move_creep_to(creep, client.pos()).ok();
                Ok(GoingTo)
            },
            Tugging { last_active_tick } => {
                if *last_active_tick + 2 < game::time() {
                    info!("Recycling {}", creep.name());
                    return Ok(Recycling(get_recycle_spawn(creep, mem)?))
                }

                Ok(self.clone())
            },
            Recycling(spawn) => {
                let Ok(spawn) = spawn.resolve().ok_or(()) else {
                    return Ok(Recycling(get_recycle_spawn(creep, mem)?))
                };

                if creep.pos().is_near_to(spawn.pos()) {
                    spawn.recycle_creep(creep).map_err(|_| ())?;
                }

                Ok(self.clone())
            },
        }
    }
}

fn get_recycle_spawn(creep: &Creep, mem: &mut Memory) -> Result<ObjectId<StructureSpawn>, ()> {
    let home_name = mem.creep(creep).ok_or(())?.home;

    if creep.pos().room_name() == home_name {
        Ok(creep.pos().find_closest_by_path(find::MY_SPAWNS, None).ok_or(())?.id())
    } else {
        Ok(game::rooms().get(home_name).ok_or(())?
            .find(find::MY_SPAWNS, None)
            .get(0).ok_or(())?.id())
    }
}