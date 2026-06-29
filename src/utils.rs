use std::ops::Deref;

use derive_where::derive_where;
use itertools::Itertools;
use screeps::{ObjectId, Position, Structure, StructureObject};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{check::{Check, CheckFrom}, domain_traits::screeps_objects::IdResolutionError, ids::{ById, CheckState, Checked, Unchecked}};

pub fn adjacent_positions(pos: Position) -> impl Iterator<Item = Position> {
    (-1..=1).cartesian_product(-1..=1)
        .filter(|(x, y)| !(*x == 0 && *y == 0))
        .map(move |offset| pos + offset)
}

#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone; S::Repr<Structure>)]
pub struct EasyStructure<S: CheckState = Checked>(S::Repr<Structure>, #[derive_where(skip)] Option<StructureObject>);

impl<S: CheckState> Serialize for EasyStructure<S> where S::Repr::<Structure> : Serialize {
    fn serialize<Se: Serializer>(&self, serializer: Se) -> Result<Se::Ok, Se::Error> {
        S::Repr::<Structure>::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for EasyStructure<Unchecked> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(ObjectId::<Structure>::deserialize(deserializer)?, None))
    }
}

impl CheckFrom for EasyStructure {
    type Unchecked = EasyStructure<Unchecked>;
    type Err = IdResolutionError<Structure>;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(EasyStructure::new(us.0.check()?))
    }
}

impl EasyStructure {
    pub fn new(structure: Structure) -> Self {
        let structure_object = StructureObject::from(structure.clone());
        Self(ById(structure), Some(structure_object))
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