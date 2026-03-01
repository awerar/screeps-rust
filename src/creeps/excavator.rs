use anyhow::anyhow;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasId, Part, ResourceType, SharedCreepProperties, Source};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyView, creeps::tugboat::TuggedCreep, messages::Messages, safeid::{IDKind, SafeID, SafeIDs, TryGetSafeID, TryMakeSafe, UnsafeIDs}, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, EnumDisplay)]
pub enum ExcavatorCreep<I: IDKind = SafeIDs> {
    Going(TuggedCreep),
    Mining,
    Building(I::ID<ConstructionSite>)
}

impl<'de> Deserialize<'de> for ExcavatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ExcavatorCreep::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match us {
            ExcavatorCreep::Going(tugged_state) => Self::Going(tugged_state),
            ExcavatorCreep::Building(site) => 
                site.try_make_safe().map(Self::Building).unwrap_or(Self::Mining),
            ExcavatorCreep::Mining => Self::Mining,
        })
    }
}

impl Default for ExcavatorCreep {
    fn default() -> Self {
        Self::Going(TuggedCreep::default())
    }
}

type Args<'a> = (SafeID<Source>, ColonyView<'a>, &'a mut Messages);
impl StateMachine<Creep, Args<'_>> for ExcavatorCreep {
    fn update(self, creep: &Creep, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use ExcavatorCreep::*;
        use Transition::*;

        let (ref source, home, messages) = args;

        let plan = home.plan.sources.source_plans.get(&source.id()).ok_or(anyhow!("Plan doesn't exist"))?;

        let work_count = creep.body().iter().filter(|bodypart| bodypart.part() == Part::Work).count() as u32;

        match self {
            Going(mut tugged_state) => {
                let harvest_pos = plan.container.as_ref().ok_or(anyhow!("No container"))?.pos;
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
                        Ok(Continue(Building(site.try_safe_id().ok_or(anyhow!("Site has no id"))?)))
                    } else {
                        let fillable = plan.get_fillable();
                        if let Some(fillable) = fillable {
                            creep.transfer(&*fillable, ResourceType::Energy, None).ok();
                            creep.harvest(source.as_ref()).ok();
                        }

                        Ok(Break(self))
                    }
                } else {
                    creep.harvest(source.as_ref()).ok();
                    Ok(Break(self))
                }
            },
            Building(ref site) => {
                // TODO: Should still fill extensions
                if creep.store().get(ResourceType::Energy).unwrap_or(0) < work_count * 5 { return Ok(Continue(Mining)) }

                creep.build(&site).ok();

                Ok(Break(self))
            }
        }
    }
}