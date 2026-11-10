use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, StructureSpawn, find, game};
use serde::Deserialize;

use crate::{check::Check, colony::ColonyView, creeps::virtual_creep::VirtualCreep, domain_traits::{HasId, ObjectId, ResolvableId}, ids::{CheckState, Checked, Unchecked}, next, statemachine::{Step, step}};

#[derive(Debug, PartialEq, Eq, EnumDisplay, Default)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<StructureSpawn, S>, S)]
pub enum ScrapCreep<S: CheckState = Checked> {
    #[default]
    Idle,
    Going(ObjectId<StructureSpawn, S>)
}

impl<'de> Deserialize<'de> for ScrapCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ScrapCreep::<Unchecked>::deserialize(deserializer)? {
            ScrapCreep::Idle => Self::Idle,
            ScrapCreep::Going(spawn) => spawn.check().map_or_default(Self::Going),
        })
    }
}

impl ScrapCreep {
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>) {
        step(self, |state| state.transition(creep, home));
    }

    pub fn transition(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> Step<Self> {
        use ScrapCreep::*;

        match self {
            Idle => {
                if creep.pos().room_name() == home.name
                    && let Some(spawn) = creep.pos().find_closest_by_path(find::MY_SPAWNS, None) {
                        next!(Going(spawn.id()))
                }

                if let Some(spawn) = home.plan.center.spawn.resolve() {
                    next!(Going(spawn.id()))
                }

                if let Some(spawn) = game::spawns().values()
                    .min_by_key(|spawn| creep.pos().get_range_to(spawn.pos())) {
                        next!(Going(spawn.id()))
                }

                Step::Stay   
            },
            Going(spawn) => {
                let spawn = spawn.resolve();

                creep.approach(spawn.pos(), 1)?;
                creep.recycle(&spawn)?;

                Step::Stay
            }
        }
    }
}