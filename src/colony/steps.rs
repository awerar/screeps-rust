use std::fmt::Debug;

use log::*;
use screeps::{RoomName, game};
use serde::{Deserialize, Serialize};

use crate::{memory::Memory, statemachine::StateMachine};

pub trait ColonyStepStateMachine where Self : Sized + Default + Eq + Debug + Clone + Ord {
    fn get_promotion(&self) -> Option<Self>;
    fn update_step(&self, name: RoomName, mem: &mut Memory) -> Result<ColonyStepTransition<Self>, ()>;
}

impl<T> StateMachine<RoomName> for T where T : ColonyStepStateMachine {
    fn update(&self, name: &RoomName, mem: &mut Memory) -> Result<Self, ()> {
        Ok(match self.update_step(*name, mem)? {
            ColonyStepTransition::None => self.clone(),
            ColonyStepTransition::Promotion => 
                self.get_promotion().ok_or(()).inspect_err(|_| error!("Promotion discreprancy for {self:?}"))?,
            ColonyStepTransition::Demotion(demotion) => demotion,
        })
    }
}

pub enum ColonyStepTransition<T> {
    None,
    Promotion,
    Demotion(T)
}

pub struct ColonyStepIterator<S> where S : ColonyStepStateMachine {
    step: S
}

impl<S> Iterator for ColonyStepIterator<S> where S : ColonyStepStateMachine {
    type Item = S;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(promotion) = self.step.get_promotion() else { return None; };
        self.step = promotion.clone();
        Some(promotion)
    }
}

impl ColonyStep {
    pub fn iter() -> ColonyStepIterator<Self> {
        ColonyStepIterator { step: Default::default() }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash, Copy)]
#[repr(u8)]
pub enum ColonyStep {
    #[default] 
    Unclaimed = 0,
    Level1(Level1Step) = 1,
    Level2 = 2,
    Level3 = 3,
    Level4 = 4,
    Level5 = 5,
    Level6 = 6,
    Level7 = 7,
    Level8 = 8
}

impl ColonyStep {
    pub fn controller_level(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
    }

    pub fn first_at_level(controller_level: u8) -> Self {
        use ColonyStep::*;

        match controller_level {
            0 => Unclaimed,
            1 => Level1(Default::default()),
            2 => Level2,
            3 => Level3,
            4 => Level4,
            5 => Level5,
            6 => Level6,
            7 => Level7,
            8 => Level8,
            _ => panic!("{controller_level} is not a valid controller level")
        }
    }
}

impl ColonyStepStateMachine for ColonyStep {
    fn get_promotion(&self) -> Option<Self> {
        use ColonyStep::*;

        match self {
            Unclaimed => Some(Level1(Default::default())),
            Level1(substep) => substep.get_promotion().map(|substep| Level1(substep)).or(Some(Level2)),
            Level2 => Some(Level3),
            Level3 => Some(Level4),
            Level4 => Some(Level5),
            Level5 => Some(Level6),
            Level6 => Some(Level7),
            Level7 => Some(Level8),
            Level8 => None
        }
    }

    fn update_step(&self, name: RoomName, mem: &mut Memory) -> Result<ColonyStepTransition<Self>, ()> {
        use ColonyStep::*;
        use ColonyStepTransition::*;

        let controller_level = mem.colony(name).unwrap().level();
        if self.controller_level() > controller_level { return Ok(Demotion(Self::first_at_level(controller_level))) }

        let controller_is_upgraded = controller_level > self.controller_level();
        let built_step = if let Some(plan_step) = mem.colony(name).unwrap().plan.steps.get(self) {
            plan_step.build(game::rooms().get(name).ok_or(())?).map_err(|e| { error!("Unable to build {self:?}: {e}"); })?
        } else { true };

        let can_level_promote = controller_is_upgraded && built_step;

        Ok(match self {
            Level1(substep) => match substep.update_step(name, mem)? {
                Demotion(demotion) => Demotion(Level1(demotion)),
                Promotion if built_step => Promotion,
                None if substep.get_promotion().is_none() && can_level_promote => Promotion,
                _ => None,
            },
            Unclaimed | Level2 | Level3 | Level4 | Level5 | Level6 | Level7 | Level8 if can_level_promote => Promotion,
            _ => None,
        })
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash, Copy)]
#[repr(u8)]
pub enum Level1Step {
    #[default]
    BuildContainerStorage,
    BuildSpawn,
    BuildSourceContainers,
    BuildArterialRoads
}

impl ColonyStepStateMachine for Level1Step {
    fn get_promotion(&self) -> Option<Self> {
        use Level1Step::*;

        match self {
            BuildContainerStorage => Some(BuildSpawn),
            BuildSpawn => Some(BuildSourceContainers),
            BuildSourceContainers => Some(BuildArterialRoads),
            BuildArterialRoads => None,
        }
    }
    
    fn update_step(&self, _: RoomName, _: &mut Memory) -> Result<ColonyStepTransition<Self>, ()> { Ok(ColonyStepTransition::Promotion) }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use super::*;

    fn discriminant<T>(step: &T) -> u8 where T : ColonyStepStateMachine {
        unsafe { *(step as *const T as *const u8) }
    }

    fn test_promotion<T>() where T : ColonyStepStateMachine {
        let mut step = T::default();
        assert_eq!(discriminant(&step), 0);

        while let Some(promotion) = step.get_promotion() {
            assert!(discriminant(&promotion) >= discriminant(&step) && discriminant(&promotion) <= discriminant(&step) + 1);
            assert!(promotion > step);

            step = promotion;
        }

        assert!(discriminant(&step) == (mem::variant_count::<T>() - 1) as u8)
    }

    #[test]
    fn test_promotions() {
        test_promotion::<ColonyStep>();
        test_promotion::<Level1Step>();
    }
}