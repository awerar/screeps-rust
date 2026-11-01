use std::collections::{BTreeMap, HashMap};

use anyhow::anyhow;
use screeps::StructureType;
use strum::IntoEnumIterator;

use crate::colony::{planner::state::LifeTime, steps::ColonyStep};

// Roads aren't counted since their lifetimes keep changing through union and clip,
// so the counts for StructureType::Road aren't meaningful
#[derive(Default)]
pub struct StructureBudget {
    structure_steps: HashMap<StructureType, BTreeMap<ColonyStep, i32>>
}

impl StructureBudget {
    pub fn count_left_for(&self, structure: StructureType, step: ColonyStep) -> u32 {
        self.calculate_all_avaliable_at(structure).into_iter()
            .skip_while(|(s, _)| *s < step)
            .map(|(_, avaliable)| avaliable)
            .min().unwrap()
    }

    pub fn get_earliest_step_for(&mut self, structure: StructureType) -> anyhow::Result<ColonyStep> {
        Ok(self.calculate_all_avaliable_at(structure).into_iter()
            .rev()
            .take_while(|(_, avaliable)| *avaliable > 0)
            .last().ok_or(anyhow!("Unable to build any more {structure:?}"))?.0)
    }

    pub fn add(&mut self, structure: StructureType, lt: LifeTime) {
        let steps = self.structure_steps.entry(structure).or_default();
        *steps.entry(lt.build_step()).or_default() += 1;
        if let Some(destroy_step) = lt.destroy_step() {
            *steps.entry(destroy_step).or_default() -= 1;
        }
    }

    fn calculate_all_avaliable_at(&self, structure: StructureType) -> BTreeMap<ColonyStep, u32> {
        let default_build_steps = BTreeMap::new();
        let build_steps = self.structure_steps.get(&structure).unwrap_or(&default_build_steps);

        ColonyStep::iter()
            .map(|step| (step, build_steps.get(&step).copied().unwrap_or(0)))
            .fold((0_u32, BTreeMap::new()), |(mut count_acc, mut map_acc), (step, count)| {
                count_acc = count_acc.checked_add_signed(count).unwrap();

                let max = structure.controller_structures(u32::from(step.controller_level()));
                let avaliable = max - count_acc;
                map_acc.insert(step,  avaliable);

                (count_acc, map_acc)
            }).1
    }
}
