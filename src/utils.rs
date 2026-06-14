use itertools::Itertools;
use screeps::{Position, ResourceType, Store};

pub fn adjacent_positions(pos: Position) -> impl Iterator<Item = Position> {
    (-1..=1).cartesian_product(-1..=1)
        .filter(|(x, y)| !(*x == 0 && *y == 0))
        .map(move |offset| pos + offset)
}

pub trait EnergyStore {
    #[expect(unused)]
    fn energy_capacity(&self) -> u32;
    fn used_energy_capacity(&self) -> u32;
    fn free_energy_capacity(&self) -> i32;
}

impl EnergyStore for Store {
    fn energy_capacity(&self) -> u32 {
        self.get_capacity(Some(ResourceType::Energy))
    }

    fn used_energy_capacity(&self) -> u32 {
        self.get_used_capacity(Some(ResourceType::Energy))
    }

    fn free_energy_capacity(&self) -> i32 {
        self.get_free_capacity(Some(ResourceType::Energy))
    }
}