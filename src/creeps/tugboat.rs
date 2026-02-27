use enum_display::EnumDisplay;
use anyhow::anyhow;
use log::warn;
use screeps::{Creep, HasPosition, Position, SharedCreepProperties, StructureSpawn, action_error_codes::{CreepMoveDirectionErrorCode, CreepMoveToErrorCode}, game};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyView, creeps::get_recycle_spawn, id::{IntoResolvedID, Resolved, ResolvedId, TryIntoResolvedID}, messages::{CreepMessage, Messages, QuickCreepMessage, SpawnMessage}, movement::Movement, statemachine::{StateMachine, StateMachineTransition, Transition}};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, EnumDisplay)]
pub enum TuggedCreep {
    #[default]
    Requesting,
    WaitingFor { tugboat: String },
    GettingTugged(ResolvedId<Creep>),
    Finished
}

impl StateMachine<Creep, Messages<Resolved>> for TuggedCreep {
    fn update(self, tugged: &Creep, messages: &mut Messages<Resolved>) -> anyhow::Result<Transition<Self>> {
        use TuggedCreep::*;
        use Transition::*;

        match &self {
            Requesting => {
                for msg in messages.creep(tugged).read_all() {
                    let CreepMessage::AssignedTugBoat(tugboat) = msg else { continue; };

                    return Ok(Continue(WaitingFor { tugboat }))
                }

                messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.clone().try_into_rid().unwrap()));
            },
            WaitingFor { tugboat } => {
                let Some(tugboat) = game::creeps().get(tugboat.clone()) else { 
                    warn!("Tugboat that was assigned to {} disapeared while waiting", tugged.name());
                    return Ok(Continue(Requesting)); 
                };

                if tugboat.pos().is_near_to(tugged.pos()) {
                    return Ok(Continue(GettingTugged(tugboat.clone().try_into_rid().unwrap())))
                }
            },
            GettingTugged(tugboat) => {
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
    pub fn move_tugged_to(&mut self, tugged: &Creep, messages: &mut Messages<Resolved>, target: Position, range: u32) {
        if tugged.pos().get_range_to(target.pos()) <= range {
            *self = TuggedCreep::Finished;
            return;
        }
        
        self.transition(tugged, messages);

        if !messages.creep_quick(tugged).empty() { return; }

        let TuggedCreep::GettingTugged(tugboat) = self else { return; };

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
    Recycling(ResolvedId<StructureSpawn>)
}

pub type Args<'a> = (ColonyView<'a>, ResolvedId<Creep>, &'a mut Movement<Resolved>, &'a mut Messages<Resolved>);
impl StateMachine<Creep, Args<'_>> for TugboatCreep {
    fn update(self, tugboat: &Creep, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use TugboatCreep::*;
        use Transition::*;

        let (home, tugged, movement, messages) = args;

        if let Recycling(ref spawn) = self {
            if tugboat.pos().is_near_to(spawn.pos()) {
                spawn.recycle_creep(tugboat)?;
            } else {
                movement.smart_move_creep_to(tugboat, spawn.pos()).ok();
            }

            return Ok(Break(self));
        }

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
                            Ok(Continue(Recycling(recycle_spawn.into_rid())))
                        }
                        Err(CreepMoveDirectionErrorCode::Tired) => Ok(Break(self)),
                        Err(e) => Err(anyhow!(e))
                    }
                }

                if last_tug_tick + 5 <= game::time() {
                    return Ok(Continue(Recycling(get_recycle_spawn(tugboat, home.name).into_rid())))
                }

                Ok(Break(self))
            },
            Recycling(_) => unreachable!()
        }
    }
}