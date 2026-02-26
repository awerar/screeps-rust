use std::{fmt::Debug, mem};

use anyhow::Ok;
use enum_display::EnumDisplay;
use screeps::Room;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, FromRepr, IntoEnumIterator};

use crate::{colony::ColonyView, statemachine::{StateMachine, Transition}};

impl StateMachine<Room, &ColonyView<'_>> for ColonyStep {
    fn update(self, room: &Room, colony_data: &mut &ColonyView<'_>) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        let controller_level = colony_data.controller.level();
        if self.controller_level() > controller_level { return Ok(Continue(Self::first_at_level(controller_level))) }

        let controller_is_upgraded = controller_level > self.controller_level();
        let built_step = colony_data.plan.steps.get(&self).map_or(Ok(true), |step| step.build(room))?;

        let Some(promotion) = self.promotion() else { return Ok(Break(self)) };
        let promotion_is_upgrade = promotion.controller_level() > self.controller_level();
        if built_step && (!promotion_is_upgrade || controller_is_upgraded) {
            Ok(Continue(promotion))
        } else {
            Ok(Break(self))
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Clone, Debug, Hash, Copy, EnumIter, EnumDisplay, FromRepr)]
#[repr(u8)]
pub enum ColonyStep {
    #[default]
    BuildSpawn,
    BuildBufferAndSourceContainers,
    BuildArterialRoads,
    UpgradeToLevel2,
    BuildLvl2,
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
    pub fn controller_level(&self) -> u8 {
        match self {
            Self::BuildSpawn
            | Self::BuildBufferAndSourceContainers
            | Self::BuildArterialRoads
            | Self::UpgradeToLevel2 => 1,
            Self::BuildLvl2
            | Self::UpgradeToLevel3 => 2,
            Self::BuildLvl3
            | Self::UpgradeToLevel4 => 3,
            Self::BuildLvl4
            | Self::UpgradeToLevel5 => 4,
            Self::BuildLvl5
            | Self::UpgradeToLevel6 => 5,
            Self::BuildLvl6
            | Self::UpgradeToLevel7 => 6,
            Self::BuildLvl7
            | Self::UpgradeToLevel8 => 7,
            Self::BuildLvl8
            | Self::EndlesslyUpgrade => 8
        }
    }

    pub fn first_at_level(level: u8) -> Self {
        assert!(level <= 8);

        for step in Self::iter() {
            if step.controller_level() >= level {
                return step;
            }
        }

        panic!();
    }

    pub fn promotion(&self) -> Option<Self> {
        Self::from_repr(*self as u8 + 1)
    }

    pub const fn last() -> Self {
        Self::from_repr(mem::variant_count::<Self>() as u8 - 1).unwrap()
    }
}