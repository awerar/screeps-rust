
use enum_display::EnumDisplay;
use anyhow::anyhow;
use log::warn;
use screeps::{Creep, HasId, HasPosition, MaybeHasId, ObjectId, Position, SharedCreepProperties, StructureSpawn, action_error_codes::{CreepMoveDirectionErrorCode, CreepMoveToErrorCode}, game};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyView, creeps::get_recycle_spawn, messages::{CreepMessage, Messages, QuickCreepMessage, SpawnMessage}, movement::Movement, safeid::{GetSafeID}, statemachine::{StateMachine, StateMachineTransition, Transition}};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, EnumDisplay)]
pub enum TuggedCreep {
    #[default]
    Requesting,
    WaitingFor { tugboat: String },
    GettingTugged(ObjectId<Creep>),
    Finished
}

impl StateMachine<Creep, Messages> for TuggedCreep {
    fn update(self, tugged: &Creep, messages: &mut Messages) -> anyhow::Result<Transition<Self>> {
        use TuggedCreep::*;
        use Transition::*;

        match &self {
            Requesting => {
                for msg in messages.creep(tugged).read_all() {
                    let CreepMessage::AssignedTugBoat(tugboat) = msg else { continue; };

                    return Ok(Continue(WaitingFor { tugboat }))
                }

                messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.safe_id()));
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
                    tugged.move_pulled_by(&tugboat)?;
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
        
        self.transition(tugged, messages);

        if !messages.creep_quick(tugged).empty() { return; }

        let TuggedCreep::GettingTugged(tugboat) = self else { return; };
        let tugboat = tugboat.resolve().unwrap();

        messages.creep_quick(&tugboat).send(QuickCreepMessage::TuggedRequestMove { target, range });
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, TuggedCreep::Finished)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone, EnumDisplay)]
pub enum TugboatCreep {
    #[default]
    GoingTo,
    Tugging { last_tug_tick: u32 },
    Recycling(ObjectId<StructureSpawn>)
}

type Args<'a> = (ColonyView<'a>, ObjectId<Creep>, &'a mut Movement, &'a mut Messages);
impl StateMachine<Creep, Args<'_>> for TugboatCreep {
    fn update(self, tugboat: &Creep, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use TugboatCreep::*;
        use Transition::*;

        let (home, tugged, movement, messages) = args;

        if let Recycling(spawn) = self {
            let Ok(spawn) = spawn.resolve().ok_or(()) else {
                return Ok(Continue(Recycling(get_recycle_spawn(tugboat, home.name).id())))
            };

            if tugboat.pos().is_near_to(spawn.pos()) {
                spawn.recycle_creep(tugboat)?;
            } else {
                movement.smart_move_creep_to(tugboat, spawn).ok();
            }

            return Ok(Break(self));
        }

        let Some(tugged) = tugged.resolve() else {
            warn!("Tugged doesn't exist. Recycling tugboat");
            return Ok(Continue(Recycling(get_recycle_spawn(tugboat, home.name).id())));
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
                                tugboat.pull(&tugged)?;
                                messages.creep_quick(&tugged).send(QuickCreepMessage::TugMove);
                                Ok(Break(Tugging { last_tug_tick: game::time() }))
                            }
                            Err(CreepMoveToErrorCode::Tired) => Ok(Break(self)),
                            Err(e) => Err(anyhow!(e))
                        }
                    }
                    
                    let recycle_spawn = get_recycle_spawn(tugboat, home.name);
                    return match tugboat.move_direction(tugboat.pos().get_direction_to(tugged.pos()).unwrap()) {
                        Ok(()) => {
                            tugboat.pull(&tugged)?;
                            messages.creep_quick(&tugged).send(QuickCreepMessage::TugMove);
                            Ok(Continue(Recycling(recycle_spawn.id())))
                        }
                        Err(CreepMoveDirectionErrorCode::Tired) => Ok(Break(self)),
                        Err(e) => Err(anyhow!(e))
                    }
                }

                if last_tug_tick + 5 <= game::time() {
                    return Ok(Continue(Recycling(get_recycle_spawn(tugboat, home.name).id())))
                }

                Ok(Break(self))
            },
            Recycling(_) => unreachable!()
        }
    }
}
