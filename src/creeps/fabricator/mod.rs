use derive_deref::Deref;
use serde::{Serialize, Deserialize};
use derive_alias::derive_alias;

mod coordinator;
mod state;
mod task;

pub use self::{state::FabricatorCreep, coordinator::FabricatorCoordinator};

derive_alias! {
    derive_percentage => #[derive(Deref, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
}

derive_percentage! { struct HealthPercentage(f32); }
derive_percentage! { struct DowngradePercentage(f32); }
derive_percentage! { struct StorageFillPercentage(f32); }

const REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.75);
const EMERGENCY_REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.5);
const CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE: DowngradePercentage = DowngradePercentage(0.5);
const STORAGE_UPGRADE_CONTROLLER_THRESHOLD: StorageFillPercentage = StorageFillPercentage(0.1);

const MAX_TASK_TICKS: u32 = 100;
const GUESSED_CREEP_MOVE_TO_TASK_TICKS: u32 = 50;