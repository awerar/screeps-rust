
use enum_display::EnumDisplay;
use screeps::{Creep, Position};
use serde::{Deserialize, Serialize};

use crate::{messages::{CreepMessage, Messages, SpawnMessage}, movement::MovementSolver, safeid::{GetSafeID, IDKind, SafeIDs, TryMakeSafe, UnsafeIDs}, statemachine::{StateMachine, StateMachineTransition, Transition}};

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, EnumDisplay)]
pub enum TuggedCreep<I: IDKind = SafeIDs> {
    #[default]
    Requesting,
    GettingTugged(I::ID<Creep>),
    Finished
}

impl<'de> Deserialize<'de> for TuggedCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = TuggedCreep::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match us {
            TuggedCreep::Requesting => Self::Requesting,
            TuggedCreep::GettingTugged(tugboat) => 
                tugboat.try_make_safe().map(Self::GettingTugged).unwrap_or(TuggedCreep::Requesting),
            TuggedCreep::Finished => Self::Finished,
        })
    }
}

impl StateMachine<Creep, Messages> for TuggedCreep {
    fn update(self, tugged: &Creep, messages: &mut Messages) -> anyhow::Result<Transition<Self>> {
        use TuggedCreep::*;
        use Transition::*;

        match self {
            Requesting => {
                for msg in messages.creep(tugged).read_all() {
                    let CreepMessage::AssignedTugBoat(tugboat) = msg else { continue; };
                    return Ok(Continue(GettingTugged(tugboat)))
                }

                messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.safe_id()));
                Ok(Break(self))
            },
            GettingTugged(_) => { Ok(Break(self)) },
            Finished => { Ok(Break(self)) },
        }
    }
}

impl TuggedCreep {
    pub fn move_tugged_to(&mut self, tugged: &Creep, messages: &mut Messages, movement_solver: &mut MovementSolver, target: Position, range: u32) -> bool {
        self.transition(tugged, messages);
        match self {
            TuggedCreep::Requesting => false,
            TuggedCreep::Finished => true,
            TuggedCreep::GettingTugged(tugboat) => {
                let done = movement_solver.move_tugged_to(tugged, tugboat, target, range);
                if done { *self = Self::Finished; }
                done
            }
        }
    }
}