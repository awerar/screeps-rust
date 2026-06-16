use std::collections::{HashMap, HashSet};

use anyhow::Result;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasPosition, HasStore, Part, Position, Repairable, Resource, ResourceType, SharedCreepProperties, Source, Store, Transferable, Withdrawable};
use thiserror::Error;

use crate::{movement::requests::{MoveToResult, MovementRequests}, safeid::{DumbID, SafeID}};

#[derive(Hash, PartialEq, Eq, Debug, Clone, Copy, EnumDisplay)]
#[expect(unused)]
pub enum IntentType {
    Attack,
    AttackController,
    Build,
    ClaimController,
    Dismantle,
    Drop, 
    Harvest,
    Heal, 
    Move,
    Pickup, 
    Pull,
    RangedAttack, 
    RangedHeal,
    RangedMassAttack, 
    Repair,
    ReserveController, 
    Transfer,
    UpgradeController, 
    Withdraw,
}

const PIPELINE_A: [IntentType; 8] = [
    IntentType::Harvest,
    IntentType::Attack,
    IntentType::Build,
    IntentType::Repair,
    IntentType::Dismantle,
    IntentType::AttackController,
    IntentType::RangedHeal,
    IntentType::Heal
];

const PIPELINE_B: [IntentType; 5] = [
    IntentType::RangedAttack,
    IntentType::RangedMassAttack,
    IntentType::Build,
    IntentType::Repair,
    IntentType::RangedHeal,
];

#[derive(Error, Debug)]
pub enum IntentError {
    #[error("Trying to register multiple {0} intents")]
    AlreadyScheduled(IntentType),
    #[error("Trying to schedule {new} from the same pipeline as {existing}")]
    PipelineCollision { existing: IntentType, new: IntentType },
    #[error("Not enough free capacity for {resource}: {curr} out of {target}")]
    NotEnoughCapacity { resource: ResourceType, target: u32, curr: u32 },
    #[error("Not enough {resource}: {curr} out of {target}")]
    NotEnoughResource { resource: ResourceType, target: u32, curr: u32 },
    #[error(transparent)]
    Execution(#[from] anyhow::Error)
}

impl IntentError {
    pub fn is_deferable(&self) -> bool {
        matches!(self, IntentError::AlreadyScheduled(_) | IntentError::PipelineCollision { .. })
    }
}

#[must_use]
pub enum ActionOutcome<T> {
    Executed(T),
    Deferred
}

impl<T> ActionOutcome<T> {
    pub fn is_deferred(&self) -> bool { matches!(self, ActionOutcome::Deferred) }
    #[expect(unused)] pub fn is_executed(&self) -> bool { matches!(self, ActionOutcome::Executed(_)) }
    pub fn result(self) -> Option<T> {
        match self {
            ActionOutcome::Executed(val) => Some(val),
            ActionOutcome::Deferred => None,
        }
    }
}

pub trait DeferrableExt<T> {
    fn ok_or_deferred(self) -> Result<ActionOutcome<T>>;
}

impl<T> DeferrableExt<T> for Result<T, IntentError> {
    fn ok_or_deferred(self) -> Result<ActionOutcome<T>> {
        match self {
            Ok(val) => Ok(ActionOutcome::Executed(val)),
            Err(err) if err.is_deferable() => Ok(ActionOutcome::Deferred),
            Err(err) => Err(err.into())
        }
    }
}

/*
vvv Summary by ChatGPT vvv

Creep actions are resolved against the state at the start of the tick, 
not step-by-step as the code is written. So a withdraw does not make energy 
available to a later transfer in the same tick, and when resource-using actions 
conflict, only the highest-priority one is applied; 
the action order in code mostly does not matter.

=====

Because of this there are two important values: free space and resource ammounts
If we distribute these two resources separately during the tick all intents will be able to be resolved

During this tick we will only remove up to [total_resources] resources and only add up to [free_capacity] resources
*/

pub struct VirtualCreep {
    creep: SafeID<Creep>,

    free_capacity: u32, // Free capacity left this tick
    total_resources: u32, // Used capacity left this tick

    total_incoming_resources: u32,
    total_outgoing_resources: u32,

    resources: HashMap<ResourceType, u32>, // Amount left this tick per resource. Lazily initialized
    incoming_resources: HashMap<ResourceType, u32>,
    
    intents: HashSet<IntentType>
}

impl VirtualCreep {
    pub fn new(creep: SafeID<Creep>) -> Self {
        VirtualCreep { 
            free_capacity: creep.store().get_free_capacity(None).try_into().unwrap_or(0),
            total_resources: creep.store().get_used_capacity(None),
            total_incoming_resources: 0,
            total_outgoing_resources: 0,
            resources: HashMap::new(),
            incoming_resources: HashMap::new(),
            creep,
            intents: HashSet::new()
        }
    }

    pub fn id(&self) -> DumbID<Creep> {
        DumbID::new(self.creep.clone())
    }

    pub fn pos(&self) -> Position {
        self.creep.pos()
    }

    pub fn has_intent(&self, intent: IntentType) -> bool {
        self.intents.contains(&intent)
    }

    #[expect(unused)]
    pub fn can_do(&self, intent: IntentType) -> bool {
        !self.has_intent(intent) &&
        self.check_pipeline(intent, PIPELINE_A).is_ok() &&
        self.check_pipeline(intent, PIPELINE_B).is_ok()
    }

    fn check_pipeline<const N: usize>(&self, intent: IntentType, pipeline: [IntentType; N]) -> Result<(), IntentError> {
        if !pipeline.contains(&intent) { return Ok(()) }
        let Some(other) = pipeline.iter().find(|other| self.has_intent(**other)) else { return Ok(()) };
        Err(IntentError::PipelineCollision { existing: *other, new: intent })
    }

    fn register_intent(&mut self, intent: IntentType) -> Result<(), IntentError> {
        if !self.intents.insert(intent) { return Err(IntentError::AlreadyScheduled(intent)) }

        self.check_pipeline(intent, PIPELINE_A)?;
        self.check_pipeline(intent, PIPELINE_B)?;

        
        Ok(())
    }

    fn part_amount(&self, part: Part, per_part: u32) -> u32 {
        u32::from(self.creep.get_active_bodyparts(part)) * per_part
    }

    fn get_resource(&self, ty: ResourceType) -> u32 {
        self.resources.get(&ty).copied().unwrap_or_else(|| self.creep.store().get_used_capacity(Some(ty)))
    }

    fn add_resource(&mut self, ty: ResourceType, amount: u32) -> Result<(), IntentError> {
        if amount > self.free_capacity { return Err(IntentError::NotEnoughCapacity { resource: ty, curr: self.free_capacity, target: amount }) }

        self.free_capacity -= amount;
        *self.incoming_resources.entry(ty).or_default() += amount;
        self.total_incoming_resources += amount;

        Ok(())
    }

    fn add_resource_capped(&mut self, ty: ResourceType, amount: u32) {
        let amount = amount.min(self.free_capacity);
        self.add_resource(ty, amount).unwrap();
    }

    fn remove_resource(&mut self, ty: ResourceType, amount: u32) -> Result<(), IntentError> {
        let resource = self.resources.entry(ty).or_insert_with(|| self.creep.store().get_used_capacity(Some(ty)));
        if amount > *resource { return Err(IntentError::NotEnoughResource { resource: ty, target: amount, curr: *resource }); }

        *resource -= amount;
        self.total_resources -= amount;
        self.total_outgoing_resources += amount;

        Ok(())
    }

    fn remove_resource_capped(&mut self, ty: ResourceType, amount: u32) {
        let amount = amount.min(self.get_resource(ty));
        self.remove_resource(ty, amount).unwrap();
    }

    #[expect(unused)]
    pub fn capacity(&self) -> u32 {
        self.creep.store().get_capacity(None)
    }

    // Free capacity left this tick
    #[expect(unused)]
    pub fn curr_free_capacity(&self) -> u32 {
        self.free_capacity
    }

    // Free capacity next tick
    pub fn next_free_capacity(&self) -> u32 {
        self.free_capacity + self.total_outgoing_resources
    }

    // Used capacity left this tick
    #[expect(unused)]
    pub fn curr_used_capacity(&self, ty: Option<ResourceType>) -> u32 {
        if let Some(ty) = ty {
            self.get_resource(ty)
        } else {
            self.total_resources
        }
    }

    // Used capacity next tick
    pub fn next_used_capacity(&self, ty: Option<ResourceType>) -> u32 {
        if let Some(ty) = ty {
            self.get_resource(ty) + self.incoming_resources.get(&ty).copied().unwrap_or(0)
        } else {
            self.total_resources + self.total_incoming_resources
        }
    }

    pub fn has_incoming(&self, ty: Option<ResourceType>) -> bool {
        if let Some(ty) = ty {
            self.incoming_resources.get(&ty).is_some_and(|x| *x > 0)
        } else {
            self.total_incoming_resources > 0
        }
    }

    #[expect(unused)]
    pub fn curr_used_energy_capacity(&self) -> u32 { self.curr_used_capacity(Some(ResourceType::Energy)) }
    pub fn next_used_energy_capacity(&self) -> u32 { self.next_used_capacity(Some(ResourceType::Energy)) }
    pub fn has_incoming_energy(&self) -> bool { self.has_incoming(Some(ResourceType::Energy)) }
    
    #[expect(unused)]
    pub fn build(&mut self, target: &ConstructionSite) -> Result<(), IntentError> {
        self.register_intent(IntentType::Build)?;

        let amount = self.part_amount(Part::Work, 5).min(target.progress_total() - target.progress());
        self.remove_resource_capped(ResourceType::Energy, amount);
        self.creep.build(target).map_err(anyhow::Error::new)?;
        Ok(())
    }

    #[expect(unused)]
    pub fn drop(&mut self, ty: ResourceType, amount: Option<u32>) -> Result<(), IntentError> {
        self.register_intent(IntentType::Drop)?;

        let amount = amount.unwrap_or(self.get_resource(ty));
        self.remove_resource(ty, amount)?;
        self.creep.drop(ty, Some(amount)).map_err(anyhow::Error::new)?;
        Ok(())
    }

    #[expect(unused)]
    pub fn harvest_source(&mut self, source: &Source) -> Result<(), IntentError> {
        self.register_intent(IntentType::Harvest)?;

        let amount = self.part_amount(Part::Work, 2).min(source.energy());
        self.add_resource_capped(ResourceType::Energy, amount);
        self.creep.harvest(source).map_err(anyhow::Error::new);
        Ok(())
    }

    pub fn pickup(&mut self, target: &Resource) -> Result<(), IntentError> {
        self.register_intent(IntentType::Pickup)?;

        self.add_resource_capped(target.resource_type(), target.amount());
        self.creep.pickup(target).map_err(anyhow::Error::new)?;
        Ok(())
    }

    #[expect(unused)]
    pub fn repair(&mut self, target: &(impl Repairable + ?Sized)) -> Result<(), IntentError> {
        self.register_intent(IntentType::Repair)?;

        let amount = self.part_amount(Part::Work, 1).min((target.hits_max() - target.hits()).div_ceil(100));
        self.remove_resource_capped(ResourceType::Energy, amount);
        self.creep.repair(target).map_err(anyhow::Error::new);
        Ok(())
    }

    // Transfer from other creep into this creep
    pub fn transfer_from(&mut self, target: &SafeID<Creep>, ty: ResourceType, amount: Option<u32>) -> Result<(), IntentError> {
        let amount = amount.unwrap_or(self.free_capacity)/*.min(target.store().get_used_capacity(Some(ty)))*/;
        self.add_resource(ty, amount)?;
        target.transfer(&*self.creep, ty, Some(amount)).map_err(anyhow::Error::new)?;
        Ok(())
    }
}

pub trait StoreTarget {
    fn store(&self) -> Store;
}

impl<T : HasStore> StoreTarget for T {
    fn store(&self) -> Store { self.store() }
}

pub trait TransferTarget: StoreTarget {
    fn transferable(&self) -> &dyn Transferable;
}

impl<T : Transferable + HasStore> TransferTarget for T {
    fn transferable(&self) -> &dyn Transferable { self }
}

pub trait WithdrawTarget: StoreTarget {
    fn withdrawable(&self) -> &dyn Withdrawable;
}

impl<T : Withdrawable + HasStore> WithdrawTarget for T {
    fn withdrawable(&self) -> &dyn Withdrawable { self }
}

impl VirtualCreep {
    pub fn transfer(&mut self, target: &impl TransferTarget, ty: ResourceType, amount: Option<u32>) -> Result<(), IntentError> {
        self.register_intent(IntentType::Transfer)?;

        let target_free_capacity = target.store().get_free_capacity(Some(ty)).try_into().unwrap_or(0);
        let amount = amount.unwrap_or(self.get_resource(ty)).min(target_free_capacity);
        self.remove_resource(ty, amount)?;
        self.creep.transfer(target.transferable(), ty, Some(amount)).map_err(anyhow::Error::new)?;
        Ok(())
    }

    pub fn withdraw(&mut self, target: &impl WithdrawTarget, ty: ResourceType, amount: Option<u32>) -> Result<(), IntentError> {
        self.register_intent(IntentType::Withdraw)?;

        let amount = amount.unwrap_or(self.free_capacity).min(target.store().get_used_capacity(Some(ty)));
        self.add_resource(ty, amount)?;
        self.creep.withdraw(target.withdrawable(), ty, Some(amount)).map_err(anyhow::Error::new)?;
        Ok(())
    }
}

impl MovementRequests {
    pub fn move_vcreep_to(&mut self, creep: &mut VirtualCreep, target: Position, range: u32) -> Result<MoveToResult, IntentError> {
        if creep.has_intent(IntentType::Move) { return Err(IntentError::AlreadyScheduled(IntentType::Move)) }

        let result = self.move_creep_to(&creep.creep, target, range);
        if !result.in_range() {
            creep.register_intent(IntentType::Move)?;
        }
        
        Ok(result)
    }

    #[expect(unused)]
    pub fn move_vtugged_to(&mut self, creep: &mut VirtualCreep, target: Position, range: u32) -> Result<MoveToResult, IntentError> {
        if creep.has_intent(IntentType::Move) { return Err(IntentError::AlreadyScheduled(IntentType::Move)) }
        
        let result = self.move_tugged_to(&creep.creep, target, range);
        if !result.in_range() {
            creep.register_intent(IntentType::Move)?;
        }
        
        Ok(result)
    }
}
