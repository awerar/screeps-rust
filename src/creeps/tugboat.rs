use log::*;
use screeps::{Creep, HasId, HasPosition, MaybeHasId, ObjectId, Position, SharedCreepProperties, StructureSpawn, action_error_codes::{CreepMoveDirectionErrorCode, CreepMoveToErrorCode}, game};
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepData, CreepRole, get_recycle_spawn, transition}, memory::Memory, messages::{CreepMessage, QuickCreepMessage, SpawnMessage}, statemachine::StateMachine};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub enum TuggedCreep {
    #[default]
    Requesting,
    WaitingFor { tugboat: String },
    GettingTugged(ObjectId<Creep>),
    Finished
}

impl StateMachine<Creep> for TuggedCreep {
    fn update(&self, tugged: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use TuggedCreep::*;

        match self {
            Requesting => {
                for msg in mem.messages.creep(tugged).read_all() {
                    #[expect(irrefutable_let_patterns)]
                    let CreepMessage::AssignedTugBoat(tugboat) = msg else { continue; };

                    return Ok(WaitingFor { tugboat })
                }

                mem.messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.try_id().unwrap()));
            },
            WaitingFor { tugboat } => {
                let Some(tugboat) = game::creeps().get(tugboat.clone()) else { 
                    warn!("Tugboat that was assigned to {} disapeared while waiting", tugged.name());
                    return Ok(Requesting); 
                };

                if tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(GettingTugged(tugboat.try_id().unwrap()))
                }
            },
            GettingTugged(tugboat) => {
                let Some(tugboat) = tugboat.resolve() else {
                    warn!("Tugboat for {} disapeared mid-tug", tugged.name());
                    return Ok(Requesting) 
                };

                if mem.messages.creep_quick(tugged).read(QuickCreepMessage::TugMove) {
                    tugged.move_pulled_by(&tugboat).inspect_err(|e| error!("Pull failed: {}", e)).map_err(|_| ())?;
                }
            },
            Finished => {  },
        }

        Ok(self.clone())
    }
}

impl TuggedCreep {
    pub fn move_tugged_to(&mut self, tugged: &Creep, mem: &mut Memory, target: Position, range: u32) {
        if tugged.pos().get_range_to(target.pos()) <= range {
            *self = TuggedCreep::Finished;
            return;
        }
        
        *self = transition(self, tugged, mem);

        if !mem.messages.creep_quick(tugged).empty() { return; }

        let TuggedCreep::GettingTugged(tugboat) = self else { return; };
        let tugboat = tugboat.resolve().unwrap();

        mem.messages.creep_quick(&tugboat).send(QuickCreepMessage::TuggedRequestMove { target, range });
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, TuggedCreep::Finished)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone)]
pub enum TugboatCreep {
    #[default]
    GoingTo,
    Tugging { last_tug_tick: u32 },
    Recycling(ObjectId<StructureSpawn>)
}

impl StateMachine<Creep> for TugboatCreep {
    fn update(&self, tugboat: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use TugboatCreep::*;

        let Some(CreepData { role: CreepRole::Tugboat(_, tugged), .. }) = mem.creep(tugboat) else { return Err(()) };

        if let Recycling(spawn) = self {
            let Ok(spawn) = spawn.resolve().ok_or(()) else {
                return Ok(Recycling(get_recycle_spawn(tugboat, mem).id()))
            };

            if tugboat.pos().is_near_to(spawn.pos()) {
                spawn.recycle_creep(tugboat).map_err(|_| ())?;
            } else {
                mem.movement.smart_move_creep_to(tugboat, spawn).ok();
            }

            return Ok(self.clone());
        }

        let Some(tugged) = tugged.resolve() else {
            warn!("Tugged doesn't exist. Recycling tugboat");
            return Ok(Recycling(get_recycle_spawn(tugboat, mem).id()));
        };

        match self {
            GoingTo => {
                if tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(Tugging { last_tug_tick: game::time() });
                }

                mem.movement.smart_move_creep_to(tugboat, tugged.pos()).ok();
                Ok(GoingTo)
            },
            Tugging { last_tug_tick } => {
                if !tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(GoingTo)
                }

                for msg in mem.messages.creep_quick(tugboat).read_all() {
                    let QuickCreepMessage::TuggedRequestMove { target, range } = msg else { continue; };

                    if tugboat.pos().get_range_to(target) > range {
                        return match tugboat.move_to(target) {
                            Ok(()) => {
                                tugboat.pull(&tugged).map_err(|_| ())?;
                                mem.messages.creep_quick(&tugged).send(QuickCreepMessage::TugMove);
                                Ok(Tugging { last_tug_tick: game::time() })
                            }
                            Err(CreepMoveToErrorCode::Tired) => Ok(self.clone()),
                            Err(_) => Err(())
                        }
                    } else {
                        let recycle_spawn = get_recycle_spawn(tugboat, mem);
                        return match tugboat.move_direction(tugboat.pos().get_direction_to(tugged.pos()).ok_or(())?) {
                            Ok(()) => {
                                tugboat.pull(&tugged).map_err(|_| ())?;
                                mem.messages.creep_quick(&tugged).send(QuickCreepMessage::TugMove);
                                Ok(Recycling(recycle_spawn.id()))
                            }
                            Err(CreepMoveDirectionErrorCode::Tired) => Ok(self.clone()),
                            Err(_) => Err(())
                        }
                    }
                }

                if last_tug_tick + 5 <= game::time() {
                    return Ok(Recycling(get_recycle_spawn(tugboat, mem).id()))
                }

                Ok(self.clone())
            },
            Recycling(_) => unreachable!()
        }
    }
}