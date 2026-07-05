use std::{marker::PhantomData, ops::Deref};

use derive_where::derive_where;
use screeps::{HasPosition, ObjectId, Position, Store, Structure, StructureObject};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{check::{Check, CheckFrom}, domain_traits::{HasStore, Transferable, Withdrawable, screeps_objects::IdResolutionError}, ids::{ById, CheckState, Checked, Unchecked}};

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

#[derive_where(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash; EasyStructure<S>)]
pub struct KindedStructure<K: StructureKind, S: CheckState = Checked>(EasyStructure<S>, PhantomData<K>);

impl<T: StructureKind> CheckFrom for KindedStructure<T> {
    type Unchecked = KindedStructure<T, Unchecked>;
    type Err = IdResolutionError<Structure>;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(KindedStructure(us.0.check()?, PhantomData))
    }
}

impl<T: StructureKind> KindedStructure<T> {
    pub fn pos(&self) -> Position { self.0.pos() }
}

trait HasStoreKind {}
impl<T: StructureKind + HasStoreKind> HasStore for KindedStructure<T> {
    fn store(&self) -> Store { self.0.structure_object().as_has_store().unwrap().store() }
}

trait TransferableKind: HasStoreKind {}
impl<T: StructureKind + TransferableKind> Transferable for KindedStructure<T> {
    fn transferable(&self) -> &dyn screeps::Transferable { self.0.structure_object().as_transferable().unwrap() }
}

trait WithdrawableKind: HasStoreKind {}
impl<T: StructureKind + WithdrawableKind> Withdrawable for KindedStructure<T> {
    fn withdrawable(&self) -> &dyn screeps::Withdrawable { self.0.structure_object().as_withdrawable().unwrap() }
}

pub trait StructureKind {}

macro_rules! def_structure_kind {
    ($name:ident, $reqs_name:ident $(, ($kind:path, $req:path))*) => {
        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] pub struct $name;
        impl StructureKind for $name {}

        $(
            impl $kind for $name {}
        )*

        pub trait $reqs_name = Into<Structure> $(+ $req)*;

        impl KindedStructure<$name> {
            pub fn new<S: $reqs_name>(structure: S) -> Self {
                Self(EasyStructure::new(structure.into()), PhantomData)
            }
        }
    };
}


def_structure_kind!(Consumer, ConsumerReqs, (HasStoreKind, HasStore), (TransferableKind, Transferable));
pub type ConsumerStructure<S = Checked> = KindedStructure<Consumer, S>;

def_structure_kind!(Provider, ProviderReqs, (HasStoreKind, HasStore), (WithdrawableKind, Withdrawable));
pub type ProviderStructure<S = Checked> = KindedStructure<Provider, S>;