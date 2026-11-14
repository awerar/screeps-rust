use std::collections::HashMap;

use screeps::{HasPosition, Source};

use crate::{colony::ColonyView, creeps::{CreepRole, excavator}, domain_traits::{CreepId, HasId, ObjectId, ResolvableId}, spawn::scheduler::{RoleSelector, creeps::HomeCreeps, prototype::RelativePrototype}};

pub enum ExcavatorSyndrome {
    NoExcavator,
    NoTugboatFor(CreepId)
}

pub struct ColonySyndrome {
    pub any_trucks: bool,
    pub excavators: HashMap<ObjectId<Source>, ExcavatorSyndrome>
}

impl ColonySyndrome {
    pub fn new(creeps: HomeCreeps<'_>, view: &ColonyView<'_>) -> Self {
        Self {
            any_trucks: creeps.iter().any(|(_, proto)| matches!(proto.role(), CreepRole::Truck(_))),
            excavators:
                view.plan.sources.iter()
                    .filter_map(|(id, source_plan)| Some((id.resolve()?.id(), source_plan)))
                    .filter_map(|(source, source_plan)| {
                        let Some((excavator, _)) = creeps.iter().find(|(_, proto)| matches!(proto.role(), CreepRole::Excavator(source2) if source == *source2)) else {
                            return Some((source, ExcavatorSyndrome::NoExcavator));
                        };

                        let creep = excavator.resolve();
                        let spawning_into_position = creep.spawning() && creep.pos() == source_plan.spawn.pos;

                        if !excavator::is_in_position(creep.pos(), &source, view)
                            && !spawning_into_position
                            && !creeps.any_of_role(RoleSelector::TugboatFor(excavator.clone())){
                                return Some((source, ExcavatorSyndrome::NoTugboatFor(excavator.clone())));
                        }

                        None
                    }).collect()
        }
    }

    pub fn any_problems(&self) -> bool {
        !self.any_trucks || !self.excavators.is_empty()
    }

    pub fn get_missing_excavator(&self) -> Option<ObjectId<Source>> {
        for (source, syndrome) in &self.excavators {
            if matches!(syndrome, ExcavatorSyndrome::NoExcavator) {
                return Some(*source)
            }
        }

        None
    }

    pub fn get_tugboatless_excavator(&self) -> Option<CreepId> {
        for syndrome in self.excavators.values() {
            if let ExcavatorSyndrome::NoTugboatFor(excavator) = syndrome {
                return Some(excavator.clone())
            }
        }

        None
    }

    pub fn update(&mut self, id: &CreepId, proto: &RelativePrototype) {
        match proto.role() {
            CreepRole::Truck(_) => self.any_trucks = true,
            CreepRole::Excavator(source) => {
                let Some(syndrome) = self.excavators.get_mut(source) else { return };

                match syndrome {
                    ExcavatorSyndrome::NoExcavator => *syndrome = ExcavatorSyndrome::NoTugboatFor(id.clone()),
                    ExcavatorSyndrome::NoTugboatFor(_) => { unreachable!("There should only ever be one excavator for each source") },
                }
            },
            CreepRole::Tugboat(tugged) => {
                self.excavators.retain(|_, syndrome| {
                    !matches!(syndrome, ExcavatorSyndrome::NoTugboatFor(e) if e == tugged)
                });
            },
            _ => ()
        }
    }
}
