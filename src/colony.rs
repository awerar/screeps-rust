use std::{collections::HashSet, fmt::Debug, ops::Not};

use screeps::{Direction, Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, RoomTerrain, StructureController, StructureObject, StructureProperties, StructureSpawn, StructureType, Terrain, find, game, look};
use serde::{Deserialize, Serialize};
use log::*;

use crate::{memory::Memory, planning::{plan_center_in, plan_main_roads_in}};

// TODO: Implement deserialization fallback to default state

const CLAIM_FLAG_PREFIX: &str = "Claim";

trait State where Self : Sized + Default + Eq + Debug + Clone + Ord {
    fn get_promotion(&self) -> Option<Self>;
    fn can_promote(&self, colony: &ColonyConfig, memory: &Memory) -> bool;

    fn get_demotion(&self, colony: &ColonyConfig, memory: &Memory) -> Option<Self>;

    fn on_transition_into(&self, colony: &ColonyConfig, memory: &mut Memory) -> Result<(), ()>;
    fn on_update(&self, colony: &ColonyConfig, memory: &mut Memory) -> Result<(), ()>;
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
    fn controller_level(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
    }

    fn transition_into(&mut self, next_state: Self, colony: &ColonyConfig, memory: &mut Memory, transition_count: usize) {
        if transition_count > 20 {
            warn!("Room {} transitioned too many times. Breaking", colony.room_name);
        }
        
        if next_state.on_transition_into(colony, memory).is_err() {
            return error!("Transition from {self:?} into {next_state:?} failed");
        }
        
        *self = next_state;
        self.update(colony, memory, transition_count + 1);
    }

    fn update(&mut self, colony: &ColonyConfig, memory: &mut Memory, transition_count: usize) {
        if let Some(demotion) = self.get_demotion(colony, memory) { 
            assert!(demotion < *self, "Demotion from {self:?} to {demotion:?} is not actually a demotion");
            warn!("Demoting colony {} from {self:?} to {demotion:?}", colony.room_name);

            return self.transition_into(demotion, colony, memory, transition_count);
        };

        if self.can_promote(colony, memory) {
            if let Some(promotion) = self.get_promotion() {
                info!("Promoting colony {} from {self:?} to {promotion:?}", colony.room_name);

                return self.transition_into(promotion, colony, memory, transition_count);
            } else {
                warn!("Transition discreprancy: can promote from {self:?}, but there is no promotion state")
            }
        }

        if self.on_update(colony, memory).is_err() {
            if *self == Self::default() {
                error!("Room {} failed on default state {:?}", colony.room_name, self)
            } else {
                warn!("Room {} failed on state {:?}. Falling back to default state {:?}", colony.room_name, self, Self::default());
                
                return self.transition_into(Self::default(), colony, memory, transition_count);
            }
        }
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

    fn can_promote(&self, colony: &ColonyConfig, memory: &Memory) -> bool {
        use ColonyState::*;

        let controller_is_upgraded = colony.level() > self.controller_level();

        match self {
            Unclaimed => controller_is_upgraded,
            Level1(substate) => 
                substate.can_promote(colony, memory) || (substate.get_promotion().is_none() && controller_is_upgraded),
            Level2 => controller_is_upgraded,
            Level3 => controller_is_upgraded,
            Level4 => controller_is_upgraded,
            Level5 => controller_is_upgraded,
            Level6 => controller_is_upgraded,
            Level7 => controller_is_upgraded,
            Level8 => false,
        }
    }

    fn get_demotion(&self, colony: &ColonyConfig, memory: &Memory) -> Option<Self> {
        use ColonyState::*;

        if self.controller_level() > colony.level() {
            return Some(match colony.level() {
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
            Level1(substate) => substate.get_demotion(colony, memory).map(|substate| Level1(substate)),
            Level2 => None,
            Level3 => None,
            Level4 => None,
            Level5 => None,
            Level6 => None,
            Level7 => None,
            Level8 => None,
        }
    }
    
    fn on_update(&self, colony: &ColonyConfig, memory: &mut Memory) -> Result<(), ()> {
        use ColonyState::*;

        match &self {
            Unclaimed => Ok(()),
            Level1(substate) => substate.on_update(colony, memory),
            Level2 => Ok(()),
            Level3 => Ok(()),
            Level4 => Ok(()),
            Level5 => Ok(()),
            Level6 => Ok(()),
            Level7 => Ok(()),
            Level8 => Ok(()),
        }
    }
    
    fn on_transition_into(&self, colony: &ColonyConfig, memory: &mut Memory) -> Result<(), ()> {
        use ColonyState::*;
        
        match &self {
            Unclaimed => {
                memory.claim_requests.insert(colony.center);
            },
            Level1(substate) => substate.on_transition_into(colony, memory)?,
            Level2 | Level3 | Level4 | Level5 | Level6 | Level7 | Level8 => {
                plan_center_in(colony);
                plan_main_roads_in(&colony.room().unwrap());
            },
        }

        if *self == Level4 {
            if let Some(buffer) = colony.buffer_structure() {
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
    BuildContainerBuffer,
    BuildSpawn,
    BuildRoads,
    UpgradeController
}

impl State for Level1State {
    fn get_promotion(&self) -> Option<Self> {
        use Level1State::*;

        match self {
            BuildContainerBuffer => Some(BuildSpawn),
            BuildSpawn => Some(BuildRoads),
            BuildRoads => Some(UpgradeController),
            UpgradeController => None,
        }
    }

    fn can_promote(&self, colony: &ColonyConfig, _memory: &Memory) -> bool {
        use Level1State::*;

        match self {
            BuildContainerBuffer => colony.buffer_structure().is_some(),
            BuildSpawn => colony.spawn().is_some(),
            BuildRoads => {
                colony.room().unwrap().find(find::CONSTRUCTION_SITES, None).into_iter()
                    .any(|site| site.structure_type() == StructureType::Road)
                    .not()
            },
            UpgradeController => colony.level() > 1,
        }
    }

    fn get_demotion(&self, colony: &ColonyConfig, _memory: &Memory) -> Option<Self> {
        use Level1State::*;

        if *self > BuildContainerBuffer && colony.buffer_structure().is_none() {
            return Some(BuildContainerBuffer)
        }

        if *self > BuildSpawn && colony.spawn().is_none() {
            return Some(BuildSpawn)
        }

        None
    }

    fn on_transition_into(&self, colony: &ColonyConfig, memory: &mut Memory) -> Result<(), ()> {
        if self.can_promote(colony, memory) { return Ok(()) }

        match self {
            Level1State::BuildContainerBuffer => {
                if !self.can_promote(colony, memory) {
                    memory.remote_build_requests.create_request(colony.buffer_pos, StructureType::Container)
                } else {
                    Ok(())
                }
            },
            Level1State::BuildSpawn => {
                if !self.can_promote(colony, memory) {
                    memory.remote_build_requests.create_request(colony.center, StructureType::Road)
                } else {
                    Ok(())
                }
            },
            Level1State::BuildRoads => Ok(plan_main_roads_in(&colony.room().unwrap())),
            Level1State::UpgradeController => Ok(()),
        }
    }

    fn on_update(&self, _colony: &ColonyConfig, _memory: &mut Memory) -> Result<(), ()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct ColonyConfig {
    pub room_name: RoomName,
    pub center: Position,
    pub buffer_pos: Position
}

impl ColonyConfig {
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

    fn try_construct_from(room_name: RoomName) -> Result<Self, &'static str> {
        let center = game::rooms().get(room_name).and_then(|room| {
            room.find(find::MY_SPAWNS, None).into_iter()
                .next()
                .map(|spawn| spawn.pos())
        }).or_else(|| {
            find_claim_flags().into_iter()
                .map(|flag| flag.pos())
                .filter(|pos| pos.room_name() == room_name)
                .next()
        });

        let Some(center) = center else {
            return Err("Unable to determine center")
        };

        let buffer_pos = game::rooms().get(room_name).and_then(|room| {
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
            let mut terrain = RoomTerrain::new(room_name).unwrap();
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

        Ok(ColonyConfig { room_name, center, buffer_pos  })
    }
}

fn find_claim_flags() -> Vec<Flag> {
    game::flags().entries()
        .filter(|(name, _)| name.starts_with(CLAIM_FLAG_PREFIX))
        .map(|(_, flag)| flag)
        .collect()
}

pub fn update_rooms(memory: &mut Memory) {
    info!("Updating rooms...");

    let owned_rooms = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name);


    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = memory.machines.colonies.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        memory.machines.colonies.remove(room);
        warn!("Lost room {}", room);
    }

    for room_name in curr_rooms {
        let room_data = memory.machines.colonies.get_mut(&room_name);
        let room_data = match room_data {
            Some(room_data) => room_data,
            None => {
                let room_config = ColonyConfig::try_construct_from(room_name);
                let room_config = room_config.inspect_err(|e| error!("Unable to create room config from room {room_name}: {e}"));
                
                let Ok(room_config) = room_config else { continue; };

                memory.machines.colonies.try_insert(room_name, (room_config, ColonyState::default())).ok().unwrap()
            },
        };

        let (config, state) = room_data;
        state.update(config, memory, 0);
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