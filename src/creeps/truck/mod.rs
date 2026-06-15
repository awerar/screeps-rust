mod coordinator;
mod stop;
mod state;

pub use self::state::{TruckCreep, VirtualTruck};
pub use self::coordinator::{CreepStops, TruckCoordinator};