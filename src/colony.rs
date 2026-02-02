use std::{collections::HashSet, fmt::Debug, mem};

use screeps::{Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, StructureController, game};
use serde::{Deserialize, Serialize};
use log::*;

use crate::memory::{Memory, SharedMemory};

const CLAIM_FLAG_PREFIX: &str = "Claim";

trait State where Self : Sized + Default + Eq + Debug + Clone {
    fn on_update(&self, colony: &ColonyConfig, memory: &mut SharedMemory) -> Result<(), ()>;
    fn on_transition_into(&self, colony: &ColonyConfig, memory: &mut SharedMemory) -> Result<(), ()>;

    fn get_demotion(&self, colony: &ColonyConfig, memory: &SharedMemory) -> Option<Self>;

    fn can_promote(&self, colony: &ColonyConfig, memory: &SharedMemory) -> bool;
    fn get_promotion(&self) -> Option<Self>;
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug)]
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

    fn get_next(&self, colony: &ColonyConfig, memory: &SharedMemory) -> Option<Self> {
        if let Some(demotion) = self.get_demotion(colony, memory) { return Some(demotion) };

        if self.can_promote(colony, memory) {
            if let Some(promotion) = self.get_promotion() {
                return Some(promotion)
            } else {
                warn!("Transition discreprancy: can promote from {self:?}, but there is no promotion state")
            }
        }

        None
    }

    fn transition_into(&mut self, next_state: Self, colony: &ColonyConfig, memory: &mut SharedMemory, transition_count: usize) {
        if transition_count > 20 {
            warn!("Room {} transitioned too many times. Breaking", colony.room_name);
        }
        
        if next_state.on_transition_into(colony, memory).is_err() {
            return error!("Transition from {self:?} into {next_state:?} failed");
        }
        
        *self = next_state;
        self.update(colony, memory, transition_count + 1);
    }

    fn update(&mut self, colony: &ColonyConfig, memory: &mut SharedMemory, transition_count: usize) {
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
    fn get_demotion(&self, colony: &ColonyConfig, memory: &SharedMemory) -> Option<Self> {
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
    
    fn on_update(&self, config: &ColonyConfig, memory: &mut SharedMemory) -> Result<(), ()> {
        use ColonyState::*;

        match &self {
            Unclaimed => Ok(()),
            Level1(state) => state.on_update(config, memory),
            Level2 => Ok(()),
            Level3 => Ok(()),
            Level4 => Ok(()),
            Level5 => Ok(()),
            Level6 => Ok(()),
            Level7 => Ok(()),
            Level8 => Ok(()),
        }
    }
    
    fn on_transition_into(&self, config: &ColonyConfig, memory: &mut SharedMemory) -> Result<(), ()> {
        use ColonyState::*;
        
        match &self {
            Unclaimed => Ok(()),
            Level1(state) => state.on_transition_into(config, memory),
            Level2 => Ok(()),
            Level3 => Ok(()),
            Level4 => Ok(()),
            Level5 => Ok(()),
            Level6 => Ok(()),
            Level7 => Ok(()),
            Level8 => Ok(()),
        }
    }
    
    fn can_promote(&self, config: &ColonyConfig, memory: &SharedMemory) -> bool {
        use ColonyState::*;

        match self {
            Unclaimed => todo!(),
            Level1(level1_state) => todo!(),
            Level2 => todo!(),
            Level3 => todo!(),
            Level4 => todo!(),
            Level5 => todo!(),
            Level6 => todo!(),
            Level7 => todo!(),
            Level8 => todo!(),
        }
    }
    
    fn get_promotion(&self) -> Option<Self> {
        use ColonyState::*;

        todo!()
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug)]
pub enum Level1State {
    #[default]
    BuildContainerBuffer,
    BuildSpawn,
    BuildRoads,
    UpgradeController
}

impl State for Level1State {
    fn on_transition_into(&self, config: &ColonyConfig, memory: &mut SharedMemory) -> Result<(), ()> {
        todo!()
    }

    fn on_update(&self, config: &ColonyConfig, memory: &mut SharedMemory) -> Result<(), ()> {
        todo!()
    }
    
    fn can_promote(&self, config: &ColonyConfig, memory: &SharedMemory) -> bool {
        todo!()
    }
    
    fn get_promotion(&self) -> Option<Self> {
        todo!()
    }
    
    fn get_demotion(&self, colony: &ColonyConfig, memory: &SharedMemory) -> Option<Self> {
        todo!()
    }
}

#[derive(Serialize, Deserialize)]
pub struct ColonyConfig {
    pub room_name: RoomName,
    pub center: Position
}

impl ColonyConfig {
    pub fn room(&self) -> Option<Room> {
        game::rooms().get(self.room_name)
    }

    pub fn controller(&self) -> Option<StructureController> {
        self.room().and_then(|room| room.controller()) 
    }

    pub fn level(&self) -> u8 {
        self.controller().map(|controller| controller.level()).unwrap_or(0)
    }

    fn try_construct_from(room_name: RoomName) -> Result<Self, &'static str> {
        todo!()
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
    let prev_rooms: HashSet<_> = memory.colonies.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        memory.colonies.remove(room);
        warn!("Lost room {}", room);
    }

    for room_name in curr_rooms {
        let room_data = memory.colonies.get_mut(&room_name);
        let room_data = match room_data {
            Some(room_data) => room_data,
            None => {
                let room_config = ColonyConfig::try_construct_from(room_name);
                let room_config = room_config.inspect_err(|e| error!("Unable to create room config from room {room_name}: {e}"));
                
                let Ok(room_config) = room_config else { continue; };

                memory.colonies.try_insert(room_name, (room_config, ColonyState::default())).ok().unwrap()
            },
        };

        let (config, state) = room_data;
        state.update(config, &mut memory.shared, 0);
    }
}