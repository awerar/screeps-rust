use std::{fmt::Debug, mem};

use anyhow::{Ok, anyhow};
use enum_display::EnumDisplay;
use screeps::Room;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, FromRepr};

use crate::{colony::ColonyView, statemachine::Flow};

impl ColonyStep {
    pub fn update(self, room: &Room, colony_data: &ColonyView<'_>) -> Flow<Self> {

        let controller_level = colony_data.controller.level();
        if self.controller_level() > controller_level { 
            Err(anyhow!("Controller level is higher than expected"))?;
        }

        let controller_is_upgraded = controller_level > self.controller_level();
        let built_step = colony_data.plan.steps.get(&self).map_or(Ok(true), |step| step.build(room))?;

        let Some(promotion) = self.promotion() else { return Flow::Break };
        let promotion_is_upgrade = promotion.controller_level() > self.controller_level();
        if built_step && (!promotion_is_upgrade || controller_is_upgraded) {
            Flow::Continue(promotion)
        } else {
            Flow::Break
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash, Copy, EnumIter, EnumDisplay, FromRepr)]
#[repr(u8)]
pub enum ColonyStep {
    #[default]
    BuildSpawn,
    UpgradeToLevel2,
    BuildLevel2Extensions,
    BuildCenterAndSourceContainers,
    BuildArterialRoads,
    UpgradeToLevel3,
    BuildLvl3,
    UpgradeToLevel4,
    BuildLvl4,
    UpgradeToLevel5,
    BuildLvl5,
    UpgradeToLevel6,
    BuildLvl6,
    UpgradeToLevel7,
    BuildLvl7,
    UpgradeToLevel8,
    BuildLvl8,
    EndlesslyUpgrade
}

impl ColonyStep {
    const CONTROLLER_LEVELS: [u8; mem::variant_count::<Self>()] = {
        let mut levels = [1; mem::variant_count::<Self>()];
        let mut i = 1;
        while i < levels.len() {
            levels[i] = levels[i - 1];
            if Self::from_repr((i - 1) as u8).unwrap().is_controller_upgrade() {
                levels[i] += 1;
            }
            i += 1;
        }
        levels
    };

    const fn is_controller_upgrade(self) -> bool {
        matches!(
            self,
            Self::UpgradeToLevel2
                | Self::UpgradeToLevel3
                | Self::UpgradeToLevel4
                | Self::UpgradeToLevel5
                | Self::UpgradeToLevel6
                | Self::UpgradeToLevel7
                | Self::UpgradeToLevel8
        )
    }

    pub const fn controller_level(self) -> u8 {
        Self::CONTROLLER_LEVELS[self as usize]
    }

    const FIRST_AT_LEVEL: [Self; 9] = {
        let mut firsts = [Self::first(); 9];
        let mut level = 0;
        while level < firsts.len() {
            let mut i = 0;
            while Self::CONTROLLER_LEVELS[i] < level as u8 {
                i += 1;
            }
            firsts[level] = Self::from_repr(i as u8).unwrap();
            level += 1;
        }
        firsts
    };

    pub const fn first_at_level(level: u8) -> Self {
        Self::FIRST_AT_LEVEL[level as usize]
    }

    pub fn promotion(self) -> Option<Self> {
        Self::from_repr(self as u8 + 1)
    }

    pub const fn last() -> Self {
        Self::from_repr(mem::variant_count::<Self>() as u8 - 1).unwrap()
    }

    pub const fn first() -> Self {
        Self::from_repr(0).unwrap()
    }
}

const _: () = {
    assert!(ColonyStep::UpgradeToLevel2.controller_level() == 1);
    assert!(ColonyStep::UpgradeToLevel3.controller_level() == 2);
    assert!(ColonyStep::UpgradeToLevel4.controller_level() == 3);
    assert!(ColonyStep::UpgradeToLevel5.controller_level() == 4);
    assert!(ColonyStep::UpgradeToLevel6.controller_level() == 5);
    assert!(ColonyStep::UpgradeToLevel7.controller_level() == 6);
    assert!(ColonyStep::UpgradeToLevel8.controller_level() == 7);
    assert!(ColonyStep::last().controller_level() == 8);
};
