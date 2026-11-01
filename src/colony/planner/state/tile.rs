use option_entry::OptionEntry;
use screeps::StructureType;
use serde::{Deserialize, Serialize};
use anyhow::{anyhow, bail};

use crate::colony::{planner::state::{LifeTime, PlannedContainer, entities::{PlannedBlocker, PlannedEntity}}, steps::ColonyStep};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TileTimeline {
    blocker: Option<(ColonyStep, PlannedBlocker)>,
    container: Option<(LifeTime, PlannedContainer)>,
    road: Option<LifeTime>
}

impl TileTimeline {
    pub fn get_block_step(&self) -> Option<ColonyStep> {
        self.blocker.map(|(step, _) | step)
    }

    pub fn road(&self) -> Option<LifeTime> {
        self.road
    }

    pub fn is_free(&self) -> bool {
        self.blocker.is_none() && self.container.is_none()
    }

    fn is_container_blocker_compatible(container_lt: LifeTime, block_step: ColonyStep, blocker: PlannedBlocker) -> bool {
        blocker.is_creep() || container_lt.is_destroyed_at(block_step)
    }

    pub fn plan_container(&mut self, lt: LifeTime, container: PlannedContainer) -> anyhow::Result<()> {
        if self.blocker.is_some_and(|(step, blocker)| !Self::is_container_blocker_compatible(lt, step, blocker)) { bail!("Container collides with blocker") }

        match self.container.entry() {
            option_entry::Entry::Occupied(_) => bail!("Already planned a container"),
            option_entry::Entry::Vacant(entry) => entry.insert((lt, container)),
        };

        Ok(())
    }

    pub fn plan_blocker(&mut self, step: ColonyStep, blocker: PlannedBlocker) -> anyhow::Result<()> {
        if self.container.is_some_and(|(lt, _)| !Self::is_container_blocker_compatible(lt, step, blocker)) { bail!("Blocker collides with container") }

        match self.blocker.entry() {
            option_entry::Entry::Occupied(_) => bail!("Already planned a blocker"),
            option_entry::Entry::Vacant(entry) => entry.insert((step, blocker)),
        };

        if let Some(lt) = self.road.as_mut() {
            *lt = lt.clip_stop(step).ok_or_else(|| anyhow!("Unable to move road earlier for blocker"))?;
        }

        Ok(())
    }

    pub fn plan_road(&mut self, mut lt: LifeTime) -> anyhow::Result<()> {
        if let Some(old_lt) = self.road.take() {
            lt = lt.union(old_lt);
        }

        if let Some((block_step, _)) = self.blocker {
            lt = lt.clip_stop(block_step).ok_or_else(|| anyhow!("Unable to move road earlier for blocker"))?;
        }

        self.road = Some(lt);

        Ok(())
    }

    pub fn structures(&self) -> impl Iterator<Item = (LifeTime, StructureType)> {
        self.blocker.iter().filter_map(|(step, blocker)| {
                let structure = blocker.structure_type()?;
                Some((LifeTime::permanent(*step), structure))
            }).chain(self.container.iter().map(|(lt, _)| (*lt, StructureType::Container)))
            .chain(self.road.iter().map(|lt| (*lt, StructureType::Road)))
    }

    pub fn entities(&self) -> impl Iterator<Item = PlannedEntity> {
        self.blocker.iter().map(|(_, blocker)| (*blocker).into())
            .chain(self.container.iter().map(|(_, container)| (*container).into()))
    }
}
