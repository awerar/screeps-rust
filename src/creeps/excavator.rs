use anyhow::anyhow;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasId, Part, ResourceType, SharedCreepProperties, Source};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyView, movement::Movement, safeid::{IDKind, SafeID, SafeIDs, TryGetSafeID, TryMakeSafe, UnsafeIDs}, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, EnumDisplay, Default)]
pub enum ExcavatorCreep<I: IDKind = SafeIDs> {
    #[default]
    Going,
    Mining,
    Building(I::ID<ConstructionSite>)
}

impl<'de> Deserialize<'de> for ExcavatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ExcavatorCreep::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match us {
            ExcavatorCreep::Going => Self::Going,
            ExcavatorCreep::Building(site) => 
                site.try_make_safe().map_or(Self::Mining, Self::Building),
            ExcavatorCreep::Mining => Self::Mining,
        })
    }
}

type Args<'a> = (SafeID<Source>, ColonyView<'a>, &'a mut Movement);
impl StateMachine<SafeID<Creep>, Args<'_>> for ExcavatorCreep {
    fn update(self, creep: &SafeID<Creep>, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use ExcavatorCreep::*;
        use Transition::*;

        let (ref source, home, movement) = args;

        let plan = home.plan.sources.source_plans.get(&source.id()).ok_or(anyhow!("Plan doesn't exist"))?;

        let work_count = creep.body().iter().filter(|bodypart| bodypart.part() == Part::Work).count() as u32;

        match self {
            Going => {
                let harvest_pos = plan.container.as_ref().ok_or(anyhow!("No container"))?.pos;
                if movement.move_tugged_to(creep, harvest_pos, 0).in_range() {
                    return Ok(Continue(Mining))
                }

                Ok(Break(self))
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

                creep.build(site).ok();

                Ok(Break(self))
            }
        }
    }
}