use std::ops::Deref;

use derive_where::derive_where;
use itertools::Itertools;
use screeps::{ObjectId, Position, Structure, StructureObject};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{check::{TryCheck, TryFromUnchecked}, ids::{CheckedID, CheckedIDs, IDKind, UncheckedIDs}};

pub fn adjacent_positions(pos: Position) -> impl Iterator<Item = Position> {
    (-1..=1).cartesian_product(-1..=1)
        .filter(|(x, y)| !(*x == 0 && *y == 0))
        .map(move |offset| pos + offset)
}

#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
pub struct EasyStructure<I: IDKind = CheckedIDs>(I::ID<Structure>, #[derive_where(skip)] Option<StructureObject>);

impl<I: IDKind> Serialize for EasyStructure<I> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        I::ID::<Structure>::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for EasyStructure<UncheckedIDs> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(ObjectId::<Structure>::deserialize(deserializer)?, None))
    }
}

impl TryFromUnchecked for EasyStructure {
    type Unchecked = EasyStructure<UncheckedIDs>;
    type Err = ();

    fn try_from_unchecked(us: Self::Unchecked) -> Result<Self, ()> {
        Ok(EasyStructure::new(us.0.try_check()?))
    }
}

impl EasyStructure {
    pub fn new(structure: CheckedID<Structure>) -> Self {
        let structure_object = StructureObject::from(structure.as_ref().clone());
        Self(structure, Some(structure_object))
    }

    pub fn structure_object(&self) -> &StructureObject {
        self.1.as_ref().unwrap()
    }
}

impl Deref for EasyStructure {
    type Target = Structure;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}