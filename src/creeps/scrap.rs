use std::mem;

use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, StructureSpawn, find, game};
use serde::Deserialize;

use crate::{check::Check, colony::ColonyView, creeps::virtual_creep::VirtualCreep, domain_traits::{HasId, ObjectId, ResolvableId}, ids::{CheckState, Checked, Unchecked}, goto, statemachine::{Flow, Next, step}};

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
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> anyhow::Result<()> {
        *self = step(mem::take(self), |state| state.transition(creep, home))?;
        Ok(())
    }

    pub fn transition(self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> Flow<Next<Self>> {
        use ScrapCreep::*;

        match self {
            Idle => {
                goto!(if creep.pos().room_name() == home.name
                    && let Some(spawn) = creep.pos().find_closest_by_path(find::MY_SPAWNS, None) {
                        Going(spawn.id())
                });

                goto!(if let Some(spawn) = home.plan.center.spawn.resolve() { Going(spawn.id()) });

                let closest_spawn = game::spawns().values().min_by_key(|spawn| creep.pos().get_range_to(spawn.pos()));
                goto!(if let Some(spawn) = closest_spawn { Going(spawn.id()) });

                Flow::Break   
            },
            Going(spawn) => {
                let spawn = spawn.resolve();

                creep.approach(spawn.pos(), 1)?;
                creep.recycle(&spawn)?;

                Flow::Break
            }
        }
    }
}