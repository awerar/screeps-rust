use std::{marker::PhantomData};

use derive_where::derive_where;
use screeps::{HasPosition, Position, SharedCreepProperties, Store, Structure, StructureObject};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, CheckFrom}, domain_traits::{HasId, HasHits, HasStore, IdResolutionError, ObjectId, Repairable, ResolvableId, Transferable, Withdrawable}, ids::{CheckState, Checked, Unchecked}};

#[derive_where(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash; ObjectId<Structure, S>)]
pub struct KindedStructure<K: StructureKind, S: CheckState = Checked>(ObjectId<Structure, S>, PhantomData<K>);

impl<T: StructureKind> CheckFrom for KindedStructure<T> {
    type Unchecked = KindedStructure<T, Unchecked>;
    type Err = IdResolutionError<Structure>;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(KindedStructure(us.0.check()?, PhantomData))
    }
}

impl<T: StructureKind> KindedStructure<T> {
    pub fn pos(&self) -> Position { self.0.resolve().pos() }

    fn structure_object(&self) -> StructureObject {
        StructureObject::from(self.0.resolve())
    }
}

trait HasStoreKind {}
impl<T: StructureKind + HasStoreKind> HasStore for KindedStructure<T> {
    fn store(&self) -> Store { 
        self.structure_object().as_has_store().unwrap().store() 
    }
}

trait TransferableKind: HasStoreKind {}
impl<T: StructureKind + TransferableKind> Transferable for KindedStructure<T> {
    fn transfer_from(&self, creep: &screeps::Creep, ty: screeps::ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::TransferErrorCode> {
        creep.transfer(self.structure_object().as_transferable().unwrap(), ty, amount)
    }
}

trait WithdrawableKind: HasStoreKind {}
impl<T: StructureKind + WithdrawableKind> Withdrawable for KindedStructure<T> {
    fn withdraw_to(&self, creep: &screeps::Creep, ty: screeps::ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::WithdrawErrorCode> {
        creep.withdraw(self.structure_object().as_withdrawable().unwrap(), ty, amount)
    }
}

trait RepairableKind {}
impl<T: StructureKind + RepairableKind> HasHits for KindedStructure<T> {
    fn hits(&self) -> u32 {
        self.structure_object().as_repairable().unwrap().hits()    
    }

    fn hits_max(&self) -> u32 {
        self.structure_object().as_repairable().unwrap().hits_max()
    }
}

impl<T: StructureKind + RepairableKind> Repairable for KindedStructure<T> {
    fn repair_by(&self, creep: &screeps::Creep) -> Result<(), screeps::action_error_codes::CreepRepairErrorCode> {
        creep.repair(self.structure_object().as_repairable().unwrap())
    }
}

pub trait StructureKind {}

macro_rules! def_structure_kind {
    ($kind_name:ident, $reqs_name:ident, $structure_name:ident $(, ($kind:path, $req:path))*) => {
        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] pub struct $kind_name;
        impl StructureKind for $kind_name {}
        pub type $structure_name<S = Checked> = KindedStructure<$kind_name, S>;

        $(
            impl $kind for $kind_name {}
        )*

        pub trait $reqs_name = Into<Structure> $(+ $req)*;

        #[allow(unused)]
        impl KindedStructure<$kind_name> {
            pub fn new<S: $reqs_name>(structure: S) -> Self {
                Self(structure.into().id(), PhantomData)
            }
        }
    };
}


def_structure_kind!(
    Consumer, 
    ConsumerReqs, 
    ConsumerStructure, 
        (HasStoreKind, HasStore), 
        (TransferableKind, Transferable)
);

def_structure_kind!(
    Provider, 
    ProviderReqs, 
    ProviderStructure, 
        (HasStoreKind, HasStore), 
        (WithdrawableKind, Withdrawable)
);

def_structure_kind!(
    RepairTarget,
    RepairTargetReqs,
    RepairableStructure,
        (RepairableKind, Repairable)
);

impl TryFrom<StructureObject> for RepairableStructure {
    type Error = ();

    fn try_from(value: StructureObject) -> Result<Self, Self::Error> {
        if value.as_repairable().is_some() {
            Ok(KindedStructure(value.as_structure().id(), PhantomData))
        } else {
            Err(())
        }
    }
}

impl TryFrom<Structure> for RepairableStructure {
    type Error = ();

    fn try_from(value: Structure) -> Result<Self, Self::Error> {
        RepairableStructure::try_from(StructureObject::from(value))
    }
}