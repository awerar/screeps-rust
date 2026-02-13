use std::collections::{HashSet, VecDeque};
use screeps::{Direction, RoomTerrain, RoomXY, Terrain};

pub struct WalkableNeighs(RoomTerrain);
impl Neigh for WalkableNeighs {
    fn new(terrain: RoomTerrain) -> Self { Self(terrain) }

    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .flat_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

pub struct DiagonalWalkableNeighs(RoomTerrain);
impl Neigh for DiagonalWalkableNeighs {
    fn new(terrain: RoomTerrain) -> Self { Self(terrain) }

    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter(|dir| dir.is_diagonal())
            .flat_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

pub struct OrthogonalWalkableNeighs(RoomTerrain);
impl Neigh for OrthogonalWalkableNeighs {
    fn new(terrain: RoomTerrain) -> Self { Self(terrain) }

    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter(|dir| dir.is_orthogonal())
            .flat_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

pub trait Neigh {
    fn new(terrain: RoomTerrain) -> Self;
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY>;
}

pub struct FloodFill<N: Neigh> {
    queue: VecDeque<(usize, RoomXY)>,
    filled: HashSet<RoomXY>,

    neighs: N
}

impl<N> FloodFill<N> where N: Neigh {
    pub fn new<T>(seed: T, terrain: RoomTerrain) -> Self where T : IntoIterator<Item = RoomXY> {
        let mut queue = VecDeque::new();
        let mut filled = HashSet::new();

        for pos in seed {
            filled.insert(pos);
            queue.push_back((0, pos));
        }

        Self { queue, filled, neighs: N::new(terrain) }
    }
}

impl<N> Iterator for FloodFill<N> where N: Neigh {
    type Item = (usize, RoomXY);

    fn next(&mut self) -> Option<Self::Item> {
        let (dist, pos) = self.queue.pop_front()?;
        let neighs = self.neighs.neighbors_of(pos);

        for new_neigh in neighs {
            if !self.filled.insert(new_neigh) { continue; }
            self.queue.push_back((dist + 1, new_neigh));
        }

        Some((dist, pos))
    }
}