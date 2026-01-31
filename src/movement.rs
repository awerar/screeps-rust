use std::{cell::RefCell, collections::HashMap, sync::LazyLock};

use js_sys::Math::random;
use screeps::{CircleStyle, Creep, Position, StructureType, action_error_codes::{CreepMoveToErrorCode, RoomPositionCreateConstructionSiteErrorCode}, game, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json_any_key::*;
use log::*;

extern crate serde_json_path_to_error as serde_json;

const HALF_TIME: f32 = 100.0;
const USAGE_PER_HALF_TIME_THRESHOLD: f32 = 7.5;

static TICK_DECAY: LazyLock<f32> = LazyLock::new(|| 0.5_f32.powf(1.0 / HALF_TIME));

thread_local! {
    pub static MOVEMENT_DATA: RefCell<MovementData> = RefCell::default();
}

#[derive(Serialize, Deserialize, Default)]
pub struct MovementData {
    #[serde(default)]
    pub creeps_data: HashMap<String, CreepMovementData>,
    #[serde(with = "any_key_map", default)]
    pub tile_usage: HashMap<Position, TileUsage>
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

pub fn smart_move_creep_to<T>(creep: &Creep, target: T) -> Result<(), CreepMoveToErrorCode>
    where 
        T: HasPosition
{
    MOVEMENT_DATA.with(|movement_data| {
        let mut movement_data = movement_data.borrow_mut();
        let creep_data = movement_data.creeps_data.entry(creep.name()).or_default();

        if let MoveState::Sleeping(_) = creep_data.move_state {
            info!("{} is sleeping... ZZZ", creep.name());
            return Ok(()) 
        }
        creep.move_to(target)
    })
}

#[derive(Serialize, Deserialize)]
pub struct TileUsage {
    usage: f32,
    last_update_tick: u32,
}

impl Default for TileUsage {
    fn default() -> Self {
        Self { usage: 0.0, last_update_tick: game::time() }
    }
}

impl TileUsage {
    fn update(&mut self) -> f32 {
        if self.last_update_tick == game::time() { return self.usage; }

        self.usage *= TICK_DECAY.powi((game::time() - self.last_update_tick) as i32);
        self.last_update_tick = game::time();
        self.usage
    }

    pub fn add_usage(&mut self, amnt: f32) -> f32 {
        self.update();
        self.usage += amnt;
        self.usage
    }
}

pub fn visualize_tile_usage() {
    MOVEMENT_DATA.with(|movement_data| {
        for (pos, usage) in movement_data.borrow_mut().tile_usage.iter_mut() {
            let usage = usage.update();

            let visual = game::rooms().get(pos.room_name()).unwrap().visual();
            visual.circle(
                pos.x().u8().into(), 
                pos.y().u8().into(), 
                Some(CircleStyle::default().radius(0.5 * (usage / USAGE_PER_HALF_TIME_THRESHOLD).min(1.0)))
            );
        }
    })
}

pub fn update_movement_tick_start() {
    MOVEMENT_DATA.with(|movement_data| {
        let mut movement_data = movement_data.borrow_mut();

        for (creep_name, creep) in game::creeps().entries() {
            let creep_data = movement_data.creeps_data.entry(creep_name.clone()).or_default();
            
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
                    info!("{} is deadlocked. Sleeping for {} ticks", creep.name(), sleep_ticks);

                    if sleep_ticks > 0 { Some(MoveState::Sleeping(game::time() + sleep_ticks)) }
                    else { None }
                },
            };

            if let Some(new_state) = new_state { creep_data.move_state = new_state }
        }
    })
}

pub fn update_movement_tick_end() {
    MOVEMENT_DATA.with(|movement_data| {
        let mut movement_data = movement_data.borrow_mut();

        for (creep_name, creep) in game::creeps().entries() {
            let creep_data = movement_data.creeps_data.entry(creep_name.clone()).or_default();

            if let Some(last_pos) = creep_data.last_pos {
                let did_move = creep.pos() != last_pos;
                if did_move {
                    let usage = movement_data.tile_usage.entry(creep.pos()).or_default().add_usage(1.0);
                    if usage > USAGE_PER_HALF_TIME_THRESHOLD {
                        match creep.pos().create_construction_site(StructureType::Road, None) {
                            Ok(()) => info!("Creating road at {}", creep.pos()),
                            Err(RoomPositionCreateConstructionSiteErrorCode::InvalidTarget) => (),
                            Err(err) => warn!("Couldn't create road at {}: {}", creep.pos(), err),
                        }
                    }
                }
            }
        }

        for (creep_name, creep) in game::creeps().entries() {
            let creep_data = movement_data.creeps_data.entry(creep_name.clone()).or_default();

            creep_data.snd_last_pos = creep_data.last_pos;
            creep_data.last_pos = Some(creep.pos());
        }
    })
}