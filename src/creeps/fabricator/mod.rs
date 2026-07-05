use crate::check::Expiration;

mod coordinator;
mod state;
mod task;

pub use self::{state::FabricatorCreep, coordinator::FabricatorCoordinator};

const REPAIR_PERCENTAGE: f32 = 0.75;
const EMERGENCY_REPAIR_PERCENTAGE: f32 = 0.5;
const CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE: f32 = 0.5;
const STORAGE_UPGRADE_CONTROLLER_THRESHOLD: f32 = 0.1;

const MAX_TASK_TICKS: u32 = 100;
const GUESSED_CREEP_MOVE_TO_TASK_TICKS: u32 = 50;

type TaskExpiration = Expiration<MAX_TASK_TICKS>;