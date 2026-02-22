use std::fmt::Display;

use screeps::{ConstructionSite, Creep, HasId, MaybeHasId, ObjectId, Part, ResourceType, SharedCreepProperties, Source};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyData, creeps::tugboat::TuggedCreep, messages::Messages, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum ExcavatorCreep {
    Going(TuggedCreep),
    Mining,
    Building(ObjectId<ConstructionSite>)
}

impl Default for ExcavatorCreep {
    fn default() -> Self {
        Self::Going(TuggedCreep::default())
    }
}

impl Display for ExcavatorCreep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

type Data = (ObjectId<Source>, ColonyData);
type Systems = Messages;
impl StateMachine<Creep, Data, Systems> for ExcavatorCreep {
    fn update(self, creep: &Creep, data: &Data, systems: &mut Systems) -> Result<Transition<Self>, ()> {
        use ExcavatorCreep::*;
        use Transition::*;

        let (source, home) = data;
        let messages = systems;

        let source = source.resolve().ok_or(())?;
        let plan = home.plan.sources.source_plans.get(&source.id()).ok_or(())?;

        let work_count = creep.body().iter().filter(|bodypart| bodypart.part() == Part::Work).count() as u32;

        match self {
            Going(mut tugged_state) => {
                let harvest_pos = plan.container.as_ref().ok_or(())?.pos;
                tugged_state.move_tugged_to(creep, messages, harvest_pos, 0);
                if tugged_state.is_finished() {
                    Ok(Continue(Mining))
                } else {
                    Ok(Break(Going(tugged_state)))
                }
            },
            Mining => {
                if creep.store().get_free_capacity(Some(ResourceType::Energy)) < (work_count * 2).try_into().unwrap() {
                    if let Some(site) = plan.get_construction_site() {
                        Ok(Continue(Building(site.try_id().ok_or(())?)))
                    } else {
                        let fillable = plan.get_fillable();
                        if let Some(fillable) = fillable {
                            creep.transfer(&*fillable, ResourceType::Energy, None).ok();
                            creep.harvest(&source).ok();
                        }

                        Ok(Break(self))
                    }
                } else {
                    creep.harvest(&source).ok();
                    Ok(Break(self))
                }
            },
            Building(site) => {
                // TODO: Should still fill extensions
                let Some(site) = site.resolve() else { return Ok(Continue(Mining)) };
                if creep.store().get(ResourceType::Energy).unwrap_or(0) < work_count * 5 { return Ok(Continue(Mining)) }

                creep.build(&site).ok();

                Ok(Break(self))
            }
        }
    }
}