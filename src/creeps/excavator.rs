use screeps::{ConstructionSite, Creep, HasId, MaybeHasId, ObjectId, Part, ResourceType, SharedCreepProperties};
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepData, CreepRole, tugboat::TuggedCreep}, memory::Memory, statemachine::StateMachine};

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

impl StateMachine<Creep> for ExcavatorCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use ExcavatorCreep::*;

        let Some(CreepData { role: CreepRole::Excavator(_, source), .. }) = mem.creep(creep) else { return Err(()) };
        let source = source.resolve().ok_or(())?;

        let plan = mem.creep_home(creep).ok_or(())?
            .plan.sources.source_plans
            .get(&source.id()).ok_or(())?;

        let work_count = creep.body().iter().filter(|bodypart| bodypart.part() == Part::Work).count() as u32;

        match self.clone() {
            Going(mut tugged_state) => {
                let harvest_pos = plan.container.as_ref().ok_or(())?.pos;
                tugged_state.move_tugged_to(creep, mem, harvest_pos, 0);
                if tugged_state.is_finished() {
                    Ok(Mining)
                } else {
                    Ok(Going(tugged_state))
                }
            },
            Mining => {
                if creep.store().get_free_capacity(Some(ResourceType::Energy)) < (work_count * 2).try_into().unwrap() {
                    if let Some(site) = plan.get_construction_site() {
                        Ok(Building(site.try_id().ok_or(())?))
                    } else {
                        let fillable = plan.get_fillable();
                        if let Some(fillable) = fillable {
                            creep.transfer(&*fillable, ResourceType::Energy, None).ok();
                        } else {
                            creep.drop(ResourceType::Energy, None).ok();
                        }
                        
                        creep.harvest(&source).ok();
                        Ok(Mining)
                    }
                } else {
                    creep.harvest(&source).ok();
                    Ok(Mining)
                }
            },
            Building(site) => {
                let Some(site) = site.resolve() else { return Ok(Mining) };
                if creep.store().get(ResourceType::Energy).unwrap_or(0) < work_count * 5 { return Ok(Mining) }

                creep.build(&site).ok();

                Ok(self.clone())
            }
        }
    }
}