use std::fmt::Display;

use log::{warn, error};
use screeps::{Creep, HasId, HasPosition, MaybeHasId, ObjectId, Position, SharedCreepProperties, StructureSpawn, action_error_codes::{CreepMoveDirectionErrorCode, CreepMoveToErrorCode}, game};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyData, creeps::get_recycle_spawn, messages::{CreepMessage, Messages, QuickCreepMessage, SpawnMessage}, movement::Movement, statemachine::{StateMachine, StateMachineTransition, Transition}};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub enum TuggedCreep {
    #[default]
    Requesting,
    WaitingFor { tugboat: String },
    GettingTugged(ObjectId<Creep>),
    Finished
}

impl Display for TuggedCreep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl StateMachine<Creep, (), Messages> for TuggedCreep {
    fn update(self, tugged: &Creep, _: &(), messages: &mut Messages) -> Result<Transition<Self>, ()> {
        use TuggedCreep::*;
        use Transition::*;

        match &self {
            Requesting => {
                for msg in messages.creep(tugged).read_all() {
                    let CreepMessage::AssignedTugBoat(tugboat) = msg else { continue; };

                    return Ok(Continue(WaitingFor { tugboat }))
                }

                messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.try_id().unwrap()));
            },
            WaitingFor { tugboat } => {
                let Some(tugboat) = game::creeps().get(tugboat.clone()) else { 
                    warn!("Tugboat that was assigned to {} disapeared while waiting", tugged.name());
                    return Ok(Continue(Requesting)); 
                };

                if tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(Continue(GettingTugged(tugboat.try_id().unwrap())))
                }
            },
            GettingTugged(tugboat) => {
                let Some(tugboat) = tugboat.resolve() else {
                    warn!("Tugboat for {} disapeared mid-tug", tugged.name());
                    return Ok(Continue(Requesting)) 
                };

                if messages.creep_quick(tugged).read(QuickCreepMessage::TugMove) {
                    tugged.move_pulled_by(&tugboat).inspect_err(|e| error!("Pull failed: {e}")).map_err(|_| ())?;
                }
            },
            Finished => {  },
        }

        Ok(Break(self))
    }
}

impl TuggedCreep {
    pub fn move_tugged_to(&mut self, tugged: &Creep, messages: &mut Messages, target: Position, range: u32) {
        if tugged.pos().get_range_to(target.pos()) <= range {
            *self = TuggedCreep::Finished;
            return;
        }
        
        self.transition(tugged, &(), messages);

        if !messages.creep_quick(tugged).empty() { return; }

        let TuggedCreep::GettingTugged(tugboat) = self else { return; };
        let tugboat = tugboat.resolve().unwrap();

        messages.creep_quick(&tugboat).send(QuickCreepMessage::TuggedRequestMove { target, range });
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

impl Display for TugboatCreep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

type Data = (ColonyData, ObjectId<Creep>);
type Systems = (Movement, Messages);
impl StateMachine<Creep, Data, Systems> for TugboatCreep {
    fn update(self, tugboat: &Creep, data: &Data, systems: &mut Systems) -> Result<Transition<Self>, ()> {
        use TugboatCreep::*;
        use Transition::*;

        let (home, tugged) = data;
        let (movement, messages) = systems;

        if let Recycling(spawn) = self {
            let Ok(spawn) = spawn.resolve().ok_or(()) else {
                return Ok(Continue(Recycling(get_recycle_spawn(tugboat, &home.room_name).id())))
            };

            if tugboat.pos().is_near_to(spawn.pos()) {
                spawn.recycle_creep(tugboat).map_err(|_| ())?;
            } else {
                movement.smart_move_creep_to(tugboat, spawn).ok();
            }

            return Ok(Break(self));
        }

        let Some(tugged) = tugged.resolve() else {
            warn!("Tugged doesn't exist. Recycling tugboat");
            return Ok(Continue(Recycling(get_recycle_spawn(tugboat, &home.room_name).id())));
        };

        match &self {
            GoingTo => {
                if tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(Continue(Tugging { last_tug_tick: game::time() }));
                }

                movement.smart_move_creep_to(tugboat, tugged.pos()).ok();
                Ok(Break(self))
            },
            Tugging { last_tug_tick } => {
                if !tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(Continue(GoingTo))
                }

                for msg in messages.creep_quick(tugboat).read_all() {
                    let QuickCreepMessage::TuggedRequestMove { target, range } = msg else { continue; };

                    if tugboat.pos().get_range_to(target) > range {
                        return match tugboat.move_to(target) {
                            Ok(()) => {
                                tugboat.pull(&tugged).map_err(|_| ())?;
                                messages.creep_quick(&tugged).send(QuickCreepMessage::TugMove);
                                Ok(Break(Tugging { last_tug_tick: game::time() }))
                            }
                            Err(CreepMoveToErrorCode::Tired) => Ok(Break(self)),
                            Err(_) => Err(())
                        }
                    }
                    
                    let recycle_spawn = get_recycle_spawn(tugboat, &home.room_name);
                    return match tugboat.move_direction(tugboat.pos().get_direction_to(tugged.pos()).ok_or(())?) {
                        Ok(()) => {
                            tugboat.pull(&tugged).map_err(|_| ())?;
                            messages.creep_quick(&tugged).send(QuickCreepMessage::TugMove);
                            Ok(Continue(Recycling(recycle_spawn.id())))
                        }
                        Err(CreepMoveDirectionErrorCode::Tired) => Ok(Break(self)),
                        Err(_) => Err(())
                    }
                }

                if last_tug_tick + 5 <= game::time() {
                    return Ok(Continue(Recycling(get_recycle_spawn(tugboat, &home.room_name).id())))
                }

                Ok(Break(self))
            },
            Recycling(_) => unreachable!()
        }
    }
}