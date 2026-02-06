use std::collections::HashMap;

use js_sys::Math::random;
use screeps::{Creep, Position, action_error_codes::CreepMoveToErrorCode, game, prelude::*};
use serde::{Deserialize, Serialize};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize, Default)]
pub struct Movement {
    #[serde(default)]
    pub creeps_data: HashMap<String, CreepMovementData>,
}

#[derive(Serialize, Deserialize)]
pub struct CreepMovementData {
   pub last_pos: Option<Position>,
   pub snd_last_pos: Option<Position>,
   move_state: MoveState 
}

impl Default for CreepMovementData {
    fn default() -> Self {
        Self { snd_last_pos: None, last_pos: None, move_state: MoveState::Moving }
    }
}

#[derive(Serialize, Deserialize)]
enum MoveState {
    Moving,
    Sleeping(u32)
}

impl Movement {
    pub fn smart_move_creep_to<T>(&mut self, creep: &Creep, target: T) -> Result<(), CreepMoveToErrorCode>
        where T: HasPosition
    {
        let creep_data = self.creeps_data.entry(creep.name()).or_default();

        if let MoveState::Sleeping(_) = creep_data.move_state {
            //debug!("{} is sleeping... ZZZ", creep.name());
            return Ok(()) 
        }
        creep.move_to(target)
    }

    pub fn update_tick_start(&mut self) {
        for (creep_name, creep) in game::creeps().entries() {
            let creep_data = self.creeps_data.entry(creep_name.clone()).or_default();
            
            let new_state = match creep_data.move_state {
                MoveState::Sleeping(awake_time) => {
                    if game::time() >= awake_time { Some(MoveState::Moving) }
                    else { None }
                },
                MoveState::Moving => 'move_state: {
                    let Some(pos1) = creep_data.snd_last_pos else { break 'move_state None };
                    let Some(pos2) = creep_data.last_pos else { break 'move_state None };
                    let pos3 = creep.pos();

                    let is_deadlocked = pos3 == pos1 && pos3 != pos2;
                    if !is_deadlocked { break 'move_state None }

                    let sleep_ticks = (random() * 2.0) as u32;
                    //debug!("{} is deadlocked. Sleeping for {} ticks", creep.name(), sleep_ticks);

                    if sleep_ticks > 0 { Some(MoveState::Sleeping(game::time() + sleep_ticks)) }
                    else { None }
                },
            };

            if let Some(new_state) = new_state { creep_data.move_state = new_state }
        }
    }

    pub fn update_tick_end(&mut self) {
        for (creep_name, creep) in game::creeps().entries() {
            let creep_data = self.creeps_data.entry(creep_name.clone()).or_default();

            creep_data.snd_last_pos = creep_data.last_pos;
            creep_data.last_pos = Some(creep.pos());
        }
    }
}