use std::fmt::Debug;

use screeps::RoomName;
use serde::{Deserialize, Serialize};
use log::*;

use crate::memory::Memory;

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

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash, Copy)]
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

    pub fn update(&self, name: RoomName, mem: &mut Memory, transition_count: usize) -> Self {
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
            Level1(substate) => substate.get_promotion().map_or(controller_is_upgraded,|_| substate.can_promote(name, mem)),
            Level2 | Level3 | Level4 | Level5 | Level6 | Level7 => controller_is_upgraded,
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
            Level2 | Level3 | Level4 | Level5 | Level6 | Level7 | Level8 => None,
        }
    }
    
    fn on_update(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()> {
        use ColonyState::*;

        match &self {
            Unclaimed => Ok(()),
            Level1(substate) => substate.on_update(name, mem),
            Level2 | Level3 | Level4 | Level5 | Level6 | Level7 | Level8 => Ok(()),
        }
    }
    
    fn on_transition_into(&self, name: RoomName, mem: &mut Memory) -> Result<(), ()> {
        use ColonyState::*;
        
        match &self {
            Unclaimed => Ok(()),
            Level1(substate) => substate.on_transition_into(name, mem),
            Level2 | Level3 | Level4 | Level5 | Level6 | Level7 | Level8 => Ok(()),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash, Copy)]
#[repr(u8)]
pub enum Level1State {
    #[default]
    BuildContainerStorage,
    BuildSpawn,
    BuildSourceContainers,
    BuildArterialRoads
}

impl State for Level1State {
    fn get_promotion(&self) -> Option<Self> {
        use Level1State::*;

        match self {
            BuildContainerStorage => Some(BuildSpawn),
            BuildSpawn => Some(BuildSourceContainers),
            BuildSourceContainers => Some(BuildArterialRoads),
            BuildArterialRoads => None,
        }
    }

    fn can_promote(&self, _: RoomName, _: &Memory) -> bool { true }
    fn get_demotion(&self, _: RoomName, _: &Memory) -> Option<Self> { None }
    fn on_transition_into(&self, _: RoomName, _: &mut Memory) -> Result<(), ()> { Ok(()) }
    fn on_update(&self, _name: RoomName, _mem: &mut Memory) -> Result<(), ()> { Ok(()) }
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