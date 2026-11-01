use serde::{Deserialize, Serialize};

use crate::colony::steps::ColonyStep;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct LifeTime {
    build_step: ColonyStep,
    destroy_step: Option<ColonyStep>
}

impl LifeTime {
    pub fn permanent(build: ColonyStep) -> Self {
        LifeTime { build_step: build, destroy_step: None }
    }

    pub fn temporary(build: ColonyStep, destroy: ColonyStep) -> Option<Self> {
        if destroy <= build { return None }

        Some(LifeTime { build_step: build, destroy_step: Some(destroy) })
    }

    pub fn build_step(self) -> ColonyStep {
        self.build_step
    }

    pub fn destroy_step(self) -> Option<ColonyStep> {
        self.destroy_step
    }

    pub fn is_built_at(self, step: ColonyStep) -> bool {
        step >= self.build_step && self.destroy_step.is_none_or(|destroy_step| destroy_step > step)
    }

    pub fn is_planned_at(self, step: ColonyStep) -> bool {
        step < self.build_step
    }

    pub fn is_destroyed_at(self, step: ColonyStep) -> bool {
        self.destroy_step.is_some_and(|destroy_step| step >= destroy_step)
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            build_step: self.build_step.min(other.build_step),
            destroy_step: self.destroy_step.zip(other.destroy_step).map(|(s1, s2)| s1.max(s2))
        }
    }

    pub fn clip_start(self, min_build_step: ColonyStep) -> Option<Self> {
        let build_step = self.build_step.max(min_build_step);
        self.destroy_step.map_or(
            Some(LifeTime::permanent(build_step)),
            |destroy_step| LifeTime::temporary(build_step, destroy_step)
        )
    }

    pub fn clip_stop(self, max_destroy_step: ColonyStep) -> Option<Self> {
        LifeTime::temporary(
            self.build_step,
            self.destroy_step.map_or(max_destroy_step, |destroy_step| destroy_step.min(max_destroy_step))
        )
    }
}
