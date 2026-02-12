use std::{collections::HashSet, fmt::Debug, ops::Not};

use itertools::Itertools;
use screeps::{Direction, Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, RoomTerrain, StructureController, StructureObject, StructureProperties, StructureSpawn, StructureType, Terrain, find, game, look};
use serde::{Deserialize, Serialize};
use log::*;

use crate::{memory::Memory, planning::{plan_center_in, plan_main_roads_in}};

const CLAIM_FLAG_PREFIX: &str = "Claim";

pub trait State where Self : Sized + Default + Eq + Debug + Clone + Ord {
    fn get_promotion(&self) -> Option<Self>;
    fn can_promote(&self, name: RoomName, mem: &Memory) -> bool;

    fn get_demotion(&self, name: RoomName, mem: &Memory) -> Option<Self>;

    fn on_transition_into(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()>;
    fn on_update(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()>;
}

pub struct StateIterator<S> where S : State {
    state: S
}

impl<S> Iterator for StateIterator<S> where S : State {
    type Item = S;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(promotion) = self.state.get_promotion() else { return None; };
        self.state = promotion.clone();
        Some(promotion)
    }
}

impl ColonyState {
    pub fn iter() -> StateIterator<Self> {
        StateIterator { state: Default::default() }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash)]
#[repr(u8)]
pub enum ColonyState {
    #[default] 
    Unclaimed = 0,
    Level1(Level1State) = 1,
    Level2 = 2,
    Level3 = 3,
    Level4 = 4,
    Level5 = 5,
    Level6 = 6,
    Level7 = 7,
    Level8 = 8
}

impl ColonyState {
    pub fn controller_level(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
    }

    pub fn first_at_level(controller_level: u8) -> Option<Self> {
        use ColonyState::*;

        Some(match controller_level {
            0 => Unclaimed,
            1 => Level1(Default::default()),
            2 => Level2,
            3 => Level3,
            4 => Level4,
            5 => Level5,
            6 => Level6,
            7 => Level7,
            8 => Level8,
            _ => return None
        })
    }

    fn transition_into(&self, next_state: Self, name: RoomName, mem: &mut Memory, transition_count: usize) -> Self {
        if transition_count > 20 {
            warn!("Room {} transitioned too many times. Breaking", name);
        }
        
        if next_state.on_transition_into(name, mem).is_err() {
            error!("Transition from {self:?} into {next_state:?} failed");
            return self.clone()
        }
        
        return next_state.update(name, mem, transition_count + 1);
    }

    fn update(&self, name: RoomName, mem: &mut Memory, transition_count: usize) -> Self {
        if let Some(demotion) = self.get_demotion(name, mem) { 
            assert!(demotion < *self, "Demotion from {self:?} to {demotion:?} is not actually a demotion");
            warn!("Demoting colony {} from {self:?} to {demotion:?}", name);

            return self.transition_into(demotion, name, mem, transition_count);
        };

        if self.can_promote(name, mem) {
            if let Some(promotion) = self.get_promotion() {
                info!("Promoting colony {} from {self:?} to {promotion:?}", name);

                return self.transition_into(promotion, name, mem, transition_count);
            } else {
                warn!("Transition discreprancy: can promote from {self:?}, but there is no promotion state")
            }
        }

        if self.on_update(name, mem).is_err() {
            if *self == Self::default() {
                error!("Room {} failed on default state {:?}", name, self);
                return Self::default()
            } else {
                warn!("Room {} failed on state {:?}. Falling back to default state {:?}", name, self, Self::default());
                
                return self.transition_into(Self::default(), name, mem, transition_count);
            }
        }

        self.clone()
    }
}

impl State for ColonyState {
    fn get_promotion(&self) -> Option<Self> {
        use ColonyState::*;

        match self {
            Unclaimed => Some(Level1(Default::default())),
            Level1(substate) => substate.get_promotion().map(|substate| Level1(substate)).or(Some(Level2)),
            Level2 => Some(Level3),
            Level3 => Some(Level4),
            Level4 => Some(Level5),
            Level5 => Some(Level6),
            Level6 => Some(Level7),
            Level7 => Some(Level8),
            Level8 => None
        }
    }

    fn can_promote(&self, name: RoomName, mem: &Memory) -> bool {
        use ColonyState::*;

        let controller_is_upgraded = mem.colony(name).unwrap().level() > self.controller_level();

        match self {
            Unclaimed => controller_is_upgraded,
            Level1(substate) => 
                substate.can_promote(name, mem) || (substate.get_promotion().is_none() && controller_is_upgraded),
            Level2 => controller_is_upgraded,
            Level3 => controller_is_upgraded,
            Level4 => controller_is_upgraded,
            Level5 => controller_is_upgraded,
            Level6 => controller_is_upgraded,
            Level7 => controller_is_upgraded,
            Level8 => false,
        }
    }

    fn get_demotion(&self, name: RoomName, mem: &Memory) -> Option<Self> {
        use ColonyState::*;

        if self.controller_level() > mem.colony(name).unwrap().level() {
            return Some(match mem.colony(name).unwrap().level() {
                0 => Unclaimed,
                1 => Level1(Default::default()),
                2 => Level2,
                3 => Level3,
                4 => Level4,
                5 => Level5,
                6 => Level6,
                7 => Level7,
                8 => Level8,
                _ => unreachable!()
            });
        }

        match self {
            Unclaimed => None,
            Level1(substate) => substate.get_demotion(name, mem).map(|substate| Level1(substate)),
            Level2 => None,
            Level3 => None,
            Level4 => None,
            Level5 => None,
            Level6 => None,
            Level7 => None,
            Level8 => None,
        }
    }
    
    fn on_update(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()> {
        use ColonyState::*;

        match &self {
            Unclaimed => Ok(()),
            Level1(substate) => substate.on_update(name, mem),
            Level2 => Ok(()),
            Level3 => Ok(()),
            Level4 => Ok(()),
            Level5 => Ok(()),
            Level6 => Ok(()),
            Level7 => Ok(()),
            Level8 => Ok(()),
        }
    }
    
    fn on_transition_into(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()> {
        use ColonyState::*;
        
        match &self {
            Unclaimed => {
                if !self.can_promote(name, mem) {
                    mem.claim_requests.insert(mem.colony(name).unwrap().center);
                }
            },
            Level1(substate) => substate.on_transition_into(name, mem)?,
            Level2 | Level3 | Level4 | Level5 | Level6 | Level7 | Level8 => {
                if !self.can_promote(name, mem) {
                    plan_center_in(mem.colony(name).unwrap());
                    plan_main_roads_in(&mem.colony(name).unwrap().room().unwrap());
                }
            },
        }

        if *self == Level4 {
            if let Some(buffer) = mem.colony(name).unwrap().buffer_structure() {
                if buffer.structure_type() == StructureType::Container {
                    buffer.destroy().ok();
                }
            }
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash)]
#[repr(u8)]
pub enum Level1State {
    #[default]
    BuildContainerStorage,
    BuildSpawn,
    BuildSourceContainers,
    BuildArterialRoads,
    UpgradeController
}

impl State for Level1State {
    fn get_promotion(&self) -> Option<Self> {
        use Level1State::*;

        match self {
            BuildContainerStorage => Some(BuildSpawn),
            BuildSpawn => Some(BuildSourceContainers),
            BuildSourceContainers => Some(BuildArterialRoads),
            BuildArterialRoads => Some(UpgradeController),
            UpgradeController => None,
        }
    }

    fn can_promote(&self, name: RoomName, mem: &Memory) -> bool {
        use Level1State::*;

        match self {
            BuildContainerStorage => mem.colony(name).unwrap().buffer_structure().is_some(),
            BuildSpawn => mem.colony(name).unwrap().spawn().is_some(),
            BuildSourceContainers => true,
            BuildArterialRoads => {
                mem.colony(name).unwrap().room().unwrap().find(find::CONSTRUCTION_SITES, None).into_iter()
                    .any(|site| site.structure_type() == StructureType::Road)
                    .not()
            },
            UpgradeController => mem.colony(name).unwrap().level() > 1,
        }
    }

    fn get_demotion(&self, name: RoomName, mem: &Memory) -> Option<Self> {
        use Level1State::*;

        if *self > BuildContainerStorage && mem.colony(name).unwrap().buffer_structure().is_none() {
            return Some(BuildContainerStorage)
        }

        if *self > BuildSpawn && mem.colony(name).unwrap().spawn().is_none() {
            return Some(BuildSpawn)
        }

        None
    }

    fn on_transition_into(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()> {
        match self {
            Level1State::BuildContainerStorage => {
                if !self.can_promote(name, mem) {
                    //mem.remote_build_requests.create_request(mem.colony(name).unwrap().buffer_pos, StructureType::Container, None)
                    Ok(())
                } else {
                    Ok(())
                }
            },
            Level1State::BuildSpawn => {
                if !self.can_promote(name, mem) {
                    //mem.remote_build_requests.create_request(mem.colony(name).unwrap().center, StructureType::Spawn, Some("Center"))
                    Ok(())
                } else {
                    Ok(())
                }
            },
            Level1State::BuildSourceContainers => Ok(()),
            Level1State::BuildArterialRoads => Ok(plan_main_roads_in(&mem.colony(name).ok_or(())?.room().ok_or(())?)),
            Level1State::UpgradeController => Ok(()),
        }
    }

    fn on_update(&self, _name: RoomName, _mem: &mut Memory) -> Result<(), ()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct ColonyData {
    pub room_name: RoomName,
    pub center: Position,
    pub buffer_pos: Position,
    pub state: ColonyState
}

impl ColonyData {
    pub fn room(&self) -> Option<Room> {
        game::rooms().get(self.room_name)
    }

    pub fn controller(&self) -> Option<StructureController> {
        self.room()?.controller()
    }

    pub fn level(&self) -> u8 {
        self.controller().map(|controller| controller.level()).unwrap_or(0)
    }

    pub fn buffer_structure(&self) -> Option<StructureObject> {
        self.buffer_pos.look_for(look::STRUCTURES).ok()?.into_iter()
            .filter(|structure| matches!(structure, StructureObject::StructureStorage(_) | StructureObject::StructureContainer(_)))
            .next()
    }

    pub fn spawn(&self) -> Option<StructureSpawn> {
        self.center.look_for(look::STRUCTURES).ok()?.into_iter()
            .filter_map(|structure| structure.try_into().ok())
            .next()
    }

    fn try_construct_from(name: RoomName) -> Option<Self> {
        let center = game::rooms().get(name).and_then(|room| {
            room.find(find::MY_SPAWNS, None).into_iter()
                .sorted_by_key(|spawn| spawn.name())
                .find_or_first(|spawn| spawn.name().starts_with("Center"))
                .map(|spawn| spawn.pos())
        }).or_else(|| {
            find_claim_flags().into_iter()
                .map(|flag| flag.pos())
                .filter(|pos| pos.room_name() == name)
                .next()
        });

        let Some(center) = center else { return None; };

        let buffer_pos = game::rooms().get(name).and_then(|room| {
            let structures = room.find(find::MY_STRUCTURES, None).into_iter()
                .map(|structure| (structure.pos(), structure.structure_type()));

            let sites = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
                .map(|site| (site.pos(), site.structure_type()));

            structures.chain(sites)
                .filter(|(pos, ty)| {
                    match ty {
                        StructureType::Storage => true,
                        StructureType::Container => pos.get_range_to(center) == 1,
                        _ => false
                    }
                }).next()
                .map(|(pos, _)| pos)
        }).unwrap_or_else(|| {
            let mut terrain = RoomTerrain::new(name).unwrap();
            let mut dir = Direction::BottomRight;
            for _ in 0..4 {
                let candidate = center + dir;
                if terrain.get_xy(candidate.xy()) != Terrain::Wall {
                    return candidate;
                }

                dir = dir.multi_rot(2);
            }

            unreachable!();
        });

        Some(Self { room_name: name, center, buffer_pos, state: Default::default()  })
    }
}

fn find_claim_flags() -> Vec<Flag> {
    game::flags().entries()
        .filter(|(name, _)| name.starts_with(CLAIM_FLAG_PREFIX))
        .map(|(_, flag)| flag)
        .collect()
}

pub fn update_rooms(mem: &mut Memory) {
    info!("Updating rooms...");

    let owned_rooms = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name);


    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = mem.colonies.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        mem.colonies.remove(room);
        warn!("Lost room {}", room);
    }

    for name in curr_rooms {
        if !mem.colonies.contains_key(&name) {
            let Some(colony) = ColonyData::try_construct_from(name) else {
                error!("Unable to construct colony config for {name}");
                continue; 
            };
            mem.colonies.insert(name, colony);
        }

        let state = mem.colonies[&name].state.clone();
        mem.colonies.get_mut(&name).unwrap().state = state.update(name, mem, 0);
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use super::*;

    fn discriminant<T>(state: &T) -> u8 where T : State {
        unsafe { *(state as *const T as *const u8) }
    }

    fn test_promotion<T>() where T : State {
        let mut state = T::default();
        assert_eq!(discriminant(&state), 0);

        while let Some(promotion) = state.get_promotion() {
            assert!(discriminant(&promotion) >= discriminant(&state) && discriminant(&promotion) <= discriminant(&state) + 1);
            assert!(promotion > state);

            state = promotion;
        }

        assert!(discriminant(&state) == (mem::variant_count::<T>() - 1) as u8)
    }

    #[test]
    fn test_promotions() {
        test_promotion::<ColonyState>();
        test_promotion::<Level1State>();
    }
}