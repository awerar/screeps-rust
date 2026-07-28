use std::collections::{HashSet, VecDeque};
use screeps::{Direction, RoomTerrain, RoomXY, Terrain};

pub struct WalkableNeighs(RoomTerrain);
impl WalkableNeighs {
    pub fn new(terrain: RoomTerrain) -> Self { Self(terrain) }
}

impl Neigh for WalkableNeighs {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

pub struct StarWalkableNeighs {
    terrain: RoomTerrain,
    center: RoomXY
}

impl StarWalkableNeighs {
    pub fn new(terrain: RoomTerrain, center: RoomXY) -> Self { Self { terrain, center } }

    pub fn eligible(&self, pos: RoomXY) -> bool {
        if self.terrain.get(pos.x.u8(), pos.y.u8()) == Terrain::Wall { return false }

        let (dx, dy) = pos - self.center;
        let (dx, dy) = (dx.abs(), dy.abs());
        let d = dx + dy;

        d % 4 == 2 || (d % 4 == 0 && dx % 2 == 1)
    }
}

impl Neigh for StarWalkableNeighs {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.eligible(*neigh))
    }
}

pub struct OrthogonalWalkableNeighs(RoomTerrain);
impl OrthogonalWalkableNeighs {
    pub fn new(terrain: RoomTerrain) -> Self { Self(terrain) }
}

impl Neigh for OrthogonalWalkableNeighs {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter(|dir| dir.is_orthogonal())
            .filter_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

pub trait Neigh {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY>;
}

pub struct FloodFill<N: Neigh> {
    queue: VecDeque<(usize, RoomXY)>,
    filled: HashSet<RoomXY>,

    neighs: N
}

impl<N: Neigh> FloodFill<N> {
    pub fn new(seed: impl IntoIterator<Item = RoomXY>, neighs: N) -> Self {
        let mut queue = VecDeque::new();
        let mut filled = HashSet::new();

        for pos in seed {
            filled.insert(pos);
            queue.push_back((0, pos));
        }

        Self { queue, filled, neighs }
    }

    pub fn neighs(&self) -> &N {
        &self.neighs
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