use std::collections::{HashMap, HashSet};

use anyhow::bail;
use ena::unify::{InPlaceUnificationTable, UnifyKey};
use itertools::Itertools;
use screeps::{Direction, Room, RoomXY, Terrain};

use crate::utils::GetXYGood;

#[derive(PartialEq, Eq, Debug, Clone, Copy, Hash)]
struct Key(u32);
impl UnifyKey for Key {
    type Value = ();
    fn index(&self) -> u32 { self.0 }
    fn from_index(u: u32) -> Self { Key(u) }
    fn tag() -> &'static str { "Key" }
}

pub struct WallUnionChecker {
    network: InPlaceUnificationTable<Key>,
    floating_blockers: HashSet<RoomXY>,
    keys: HashMap<RoomXY, Key>
}

impl WallUnionChecker {
    pub fn new(room: &Room) -> Self {
        let terrain = room.get_terrain();

        let mut network = InPlaceUnificationTable::new();
        let mut keys = HashMap::new();

        for x in 0..50 {
            for y in 0..50 {
                let xy = RoomXY::new(x.try_into().unwrap(), y.try_into().unwrap());
                if terrain.get_xy_good(xy) != Terrain::Wall { continue; }

                let key = network.new_key(());
                keys.insert(xy, key);
                
                for neigh in orth_neighs(xy) {
                    let Some(neigh_key) = keys.get(&neigh) else { continue; };

                    network.union(key, *neigh_key);
                }
            }
        }

        Self { 
            network, 
            floating_blockers: HashSet::new(), 
            keys 
        }
    }

    pub fn can_block(&mut self, xy: RoomXY) -> bool {
        if self.keys.contains_key(&xy) || self.floating_blockers.contains(&xy) { return false }

        orth_neighs(xy)
            .filter_map(|neigh| self.keys.get(&neigh))
            .map(|key| self.network.find(*key))
            .unique()
            .count() <= 1
    }

    pub fn block(&mut self, xy: RoomXY) -> anyhow::Result<()> {
        if self.keys.contains_key(&xy) || self.floating_blockers.contains(&xy) { 
            bail!("Already blocked at {xy}"); 
        }

        let wall_keys = orth_neighs(xy)
            .filter_map(|xy| self.keys.get(&xy))
            .map(|key| self.network.find(*key))
            .unique()
            .collect_vec();

        if wall_keys.len() > 1 { bail!("Blocking {xy} would connect different walls"); }

        let wall_key = wall_keys.first();
        if let Some(wall_key) = wall_key {
            let key = self.network.new_key(());
            self.keys.insert(xy, key);
            self.network.union(*wall_key, key);

            for neigh in orth_neighs(xy) {
                if self.floating_blockers.remove(&neigh) {
                    self.block(neigh)?;
                }
            }
        } else {
            self.floating_blockers.insert(xy);
        }

        Ok(())
    }
}

fn orth_neighs(xy: RoomXY) -> impl Iterator<Item = RoomXY> {
    Direction::iter()
        .filter(|dir| dir.is_orthogonal())
        .filter_map(move |dir| xy.checked_add_direction(*dir))
}