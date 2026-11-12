use std::collections::HashMap;

use screeps::{Creep, HasPosition, Source};

use crate::{colony::ColonyView, creeps::{CreepRole, excavator}, domain_traits::{CreepId, HasId, ObjectId, ResolvableId}, spawn::roster::{RoleSelector, creeps::ColonyCreeps}};

pub enum ExcavatorSyndrome {
    NoExcavator,
    NoTugboatFor(CreepId)
}

pub struct ColonySyndrome {
    pub any_trucks: bool,
    pub any_excavating_excavators: bool,
    pub excavators: HashMap<ObjectId<Source>, ExcavatorSyndrome>
}

impl ColonySyndrome {
    pub fn new(creeps: &ColonyCreeps, view: &ColonyView<'_>) -> Self {
        Self {
            any_trucks: creeps.0.values().any(|proto| matches!(proto.role(), CreepRole::Truck(_))),
            any_excavating_excavators: creeps.iter().any(|(creep, proto)| {
                let CreepRole::Excavator(source) = proto.role() else { return false; };
                excavator::is_in_position(creep.resolve().pos(), source, view)
            }),
            excavators:
                view.plan.sources.keys()
                    .filter_map(|id| Some(id.resolve()?.id()))
                    .filter_map(|source| {
                        let Some((excavator, _)) = creeps.0.iter().find(|(_, proto)| matches!(proto.role(), CreepRole::Excavator(source2) if source == *source2)) else {
                            return Some((source, ExcavatorSyndrome::NoExcavator));
                        };

                        if !excavator::is_in_position(excavator.resolve().pos(), &source, view)
                            && !creeps.any_of_role(RoleSelector::TugboatFor(excavator.clone())){
                                return Some((source, ExcavatorSyndrome::NoTugboatFor(excavator.clone())));
                        }

                        None
                    }).collect()
        }
    }

    pub fn any_problems(&self) -> bool {
        !self.any_trucks || !self.any_excavating_excavators || !self.excavators.is_empty()
    }

    pub fn tugged_order(&self) -> Option<Vec<Creep>> {
        if self.any_problems() {
            Some(
                if self.any_trucks {
                    self.excavators.values().filter_map(|excavator| {
                        match excavator {
                            ExcavatorSyndrome::NoExcavator => None,
                            ExcavatorSyndrome::NoTugboatFor(creep ) => Some(creep.resolve())
                        }
                    }).collect()
                } else {
                    Vec::new()
                }
            )
        } else {
            None
        }
    }
}
