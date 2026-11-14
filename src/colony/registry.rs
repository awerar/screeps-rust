//! A `ColonyName` proves that its colony exists, so lookups by name never fail.
//!
//! A name can only go stale if its colony is removed after the name was handed out. So `Colonies` has no
//! removal method: lost colonies and `ResetColony` commands are dropped while Memory is deserialized at the
//! start of the tick, and after that the map can only grow.
//!
//! Names are only minted in this file, by `filter_check_from` and `insert`; every other name is a copy of an existing
//! key. Checked names are never deserialized: persisted references are `ColonyName<Unchecked>` and must go
//! through `Colonies::verify`.
//!
//! Names are also only minted for rooms that are visible and whose controller is ours (`is_owned_room`). The game
//! state doesn't change within a tick and checked names can't outlive their tick, so `room()` and `controller()`
//! can't fail.
//!
//! Known gap: a name used with a different `Colonies` instance, e.g. one deserialized, filter-checked or
//! defaulted mid-tick.
//! Only lifetime branding would rule that out, which isn't worth the borrow friction.

use std::{collections::HashMap, fmt::Display, marker::PhantomData};

use derive_where::derive_where;
use log::warn;
use screeps::{OwnedStructureProperties, Room, RoomName, StructureController, game};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{check::{FilterCheck, FilterCheckFrom}, colony::{plan::ColonyPlan, steps::ColonyStep}, commands::{Command, handle_commands}, creeps::{fabricator::FabricatorCoordinator, servicer::ServicerCoordinator, truck::TruckCoordinator}, ids::{CheckState, Checked, Unchecked}};

#[derive_where(Clone, Copy, Debug, Eq, Hash)]
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound(serialize = "", deserialize = "S: Deserialize<'de>"))]
pub struct ColonyName<S: CheckState = Checked>(RoomName, #[serde(skip)] PhantomData<S>);

impl<S: CheckState> ColonyName<S> {
    fn new(room: RoomName) -> Self {
        Self(room, PhantomData)
    }

    pub fn room_name(self) -> RoomName {
        self.0
    }
}

impl ColonyName {
    pub fn room(self) -> Room {
        game::rooms().get(self.0).expect("ColonyName guarantees a visible room with our controller")
    }

    pub fn controller(self) -> StructureController {
        self.room().controller().expect("ColonyName guarantees a visible room with our controller")
    }
}

pub fn is_owned_room(room: RoomName) -> bool {
    game::rooms().get(room).and_then(|room| room.controller()).is_some_and(|controller| controller.my())
}

impl From<ColonyName> for ColonyName<Unchecked> {
    fn from(name: ColonyName) -> Self {
        Self::new(name.0)
    }
}

impl<A: CheckState, B: CheckState> PartialEq<ColonyName<B>> for ColonyName<A> {
    fn eq(&self, other: &ColonyName<B>) -> bool {
        self.0 == other.0
    }
}

impl<S: CheckState> Display for ColonyName<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = "S: Deserialize<'de>"))]
pub struct Colony<S: CheckState = Checked> {
    name: ColonyName<S>,
    pub plan: ColonyPlan,
    pub step: ColonyStep,
    pub coordinators: ColonyCoordinators
}

impl<S: CheckState> Colony<S> {
    pub fn name(&self) -> ColonyName<S> {
        self.name
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct ColonyCoordinators {
    pub trucks: TruckCoordinator,
    pub fabricators: FabricatorCoordinator,
    pub servicers: ServicerCoordinator
}

#[derive_where(Default)]
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound(serialize = "", deserialize = "S: Deserialize<'de>"))]
pub struct Colonies<S: CheckState = Checked>(#[serde(with = "colony_list")] HashMap<ColonyName<S>, Colony<S>>);

mod colony_list {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serializer};

    use crate::ids::CheckState;

    use super::{Colony, ColonyName};

    pub fn serialize<S: CheckState, Ser: Serializer>(colonies: &HashMap<ColonyName<S>, Colony<S>>, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        serializer.collect_seq(colonies.values())
    }

    pub fn deserialize<'de, S: CheckState, D: Deserializer<'de>>(deserializer: D) -> Result<HashMap<ColonyName<S>, Colony<S>>, D::Error>
    where
        Colony<S>: Deserialize<'de>
    {
        Ok(Vec::<Colony<S>>::deserialize(deserializer)?.into_iter().map(|colony| (colony.name, colony)).collect())
    }
}

impl FilterCheckFrom for Colonies {
    type Unchecked = Colonies<Unchecked>;
    type Err = ColonyName<Unchecked>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (owned, lost): (Vec<_>, Vec<_>) = uc.0.into_values()
            .partition(|colony| is_owned_room(colony.name.0));

        let owned = owned.into_iter()
            .map(|Colony { name, plan, step, coordinators }| {
                let name = ColonyName::new(name.0);
                (name, Colony { name, plan, step, coordinators })
            }).collect();

        (Self(owned), lost.into_iter().map(|colony| colony.name).collect())
    }
}

impl<'de> Deserialize<'de> for Colonies {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut unchecked = Colonies::<Unchecked>::deserialize(deserializer)?;

        handle_commands(|command| {
            let Command::ResetColony { room } = command else { return false; };
            if let Ok(room) = RoomName::new(room) { unchecked.0.remove(&ColonyName::new(room)); }
            true
        });

        let (colonies, lost) = unchecked.filter_check();
        for name in lost {
            warn!("Lost colony {name}");
        }

        Ok(colonies)
    }
}

impl Colonies {
    pub fn insert(&mut self, room: RoomName, plan: ColonyPlan, step: ColonyStep) -> Option<ColonyName> {
        if !is_owned_room(room) { return None; }

        let name = ColonyName::new(room);
        self.0.insert(name, Colony { name, plan, step, coordinators: ColonyCoordinators::default() });
        
        Some(name)
    }

    pub fn lookup(&self, room: RoomName) -> Option<ColonyName> {
        self.0.get_key_value(&ColonyName::new(room)).map(|(name, _)| *name)
    }

    pub fn verify(&self, name: ColonyName<Unchecked>) -> Option<ColonyName> {
        self.lookup(name.0)
    }

    pub fn get(&self, name: ColonyName) -> &Colony {
        self.0.get(&name).expect("ColonyName should always refer to an existing colony")
    }

    pub fn get_mut(&mut self, name: ColonyName) -> &mut Colony {
        self.0.get_mut(&name).expect("ColonyName should always refer to an existing colony")
    }

    pub fn get_disjoint_mut<const N: usize>(&mut self, names: [ColonyName; N]) -> [&mut Colony; N] {
        self.0.get_disjoint_mut(names.each_ref())
            .map(|colony| colony.expect("ColonyName should always refer to an existing colony"))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Colony> {
        self.0.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Colony> {
        self.0.values_mut()
    }
}
