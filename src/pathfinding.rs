use screeps::{Position, RoomName, pathfinder::{self, MultiRoomCostResult, SearchResults}};

pub fn search(from: Position, to: Position, range: u32) -> SearchResults {
    pathfinder::search::<fn(RoomName) -> MultiRoomCostResult>(from, to, range, None)
}