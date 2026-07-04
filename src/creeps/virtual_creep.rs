use std::collections::HashMap;

use anyhow::Result;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasPosition, Part, Position, Repairable, Resource, ResourceType, SharedCreepProperties, Source};
use thiserror::Error;

use crate::{domain_traits::{HasStoreExt, Transferable, Withdrawable}, ids::{Handle, WithId}, movement::requests::{MoveToResult, MovementRequests}, spawn::Body};

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
    Other(#[from] anyhow::Error)
}

impl IntentError {
    pub fn is_deferable(&self) -> bool {
        matches!(self, IntentError::AlreadyScheduled(_) | IntentError::PipelineCollision { .. })
    }
}

#[macro_export]
macro_rules! break_deferable {
    ($expr:expr, $next:expr) => {
        match $expr {
            std::result::Result::Ok(val) => std::result::Result::Ok(val),
            std::result::Result::Err(e) if e.is_deferable() => return std::result::Result::Ok($crate::statemachine::Transition::Break($next)),
            std::result::Result::Err(e) => std::result::Result::Err(e)
        }
    };
}

#[macro_export]
macro_rules! break_move {
    ($expr:expr, $next:expr) => {
        match $expr {
            std::result::Result::Ok(val) if !val.in_range() => return std::result::Result::Ok($crate::statemachine::Transition::Break($next)),
            std::result::Result::Ok(_) => std::result::Result::Ok(()),
            std::result::Result::Err(e) => std::result::Result::Err(e)
        }
    };
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
    creep: WithId<Creep>,

    free_capacity: u32, // Free capacity left this tick
    total_resources: u32, // Used capacity left this tick

    total_incoming_resources: u32,
    total_outgoing_resources: u32,

    resources: HashMap<ResourceType, u32>, // Amount left this tick per resource. Lazily initialized
    incoming_resources: HashMap<ResourceType, u32>,
    
    intents: HashMap<IntentType, Intent>
}

enum IntentEffect {
    Incoming(ResourceType, u32),
    Outgoing(ResourceType, u32)
}

impl IntentEffect {
    pub fn incoming_energy(amount: u32) -> Self {
        IntentEffect::Incoming(ResourceType::Energy, amount)
    }

    pub fn outgoing_energy(amount: u32) -> Self {
        IntentEffect::Outgoing(ResourceType::Energy, amount)
    }
}

trait CommitFn = FnOnce(&Creep) -> Result<()>;
struct Intent {
    effect: Option<IntentEffect>,
    commit: Box<dyn CommitFn>
}

impl Intent {
    fn new(commit: impl CommitFn + 'static, effect: Option<IntentEffect>) -> Self {
        Intent { 
            effect,
            commit: Box::new(commit) 
        }
    }
}

impl VirtualCreep {
    pub fn new(creep: WithId<Creep>) -> Self {
        VirtualCreep { 
            free_capacity: creep.free_capacity(None),
            total_resources: creep.used_capacity(None),
            total_incoming_resources: 0,
            total_outgoing_resources: 0,
            resources: HashMap::new(),
            incoming_resources: HashMap::new(),
            creep,
            intents: HashMap::new()
        }
    }

    pub fn handle(&self) -> Handle<WithId<Creep>> {
        Handle::new(self.creep.clone())
    }

    pub fn pos(&self) -> Position {
        self.creep.pos()
    }

    pub fn name(&self) -> String {
        self.creep.name()
    }

    pub fn body(&self) -> Body {
        Body::from(&*self.creep)
    }

    pub fn has_intent(&self, intent: IntentType) -> bool {
        self.intents.contains_key(&intent)
    }

    #[expect(unused)]
    pub fn can_do(&self, intent: IntentType) -> bool {
        !self.has_intent(intent) &&
        self.check_pipeline(intent, PIPELINE_A).is_ok() &&
        self.check_pipeline(intent, PIPELINE_B).is_ok()
    }
    
    pub fn commit(self) -> Result<()> {
        for intent in self.intents.into_values() {
            (intent.commit)(&self.creep)?;
        }

        Ok(())
    }

    fn check_pipeline<const N: usize>(&self, intent: IntentType, pipeline: [IntentType; N]) -> Result<(), IntentError> {
        if !pipeline.contains(&intent) { return Ok(()) }
        let Some(other) = pipeline.iter().find(|other| self.has_intent(**other)) else { return Ok(()) };
        Err(IntentError::PipelineCollision { existing: *other, new: intent })
    }

    fn register_intent(&mut self, ty: IntentType, intent: Intent) -> Result<u32, IntentError> {
        if self.intents.contains_key(&ty) { return Err(IntentError::AlreadyScheduled(ty)) }

        self.check_pipeline(ty, PIPELINE_A)?;
        self.check_pipeline(ty, PIPELINE_B)?;

        match &intent.effect {
            Some(IntentEffect::Incoming(ty, amount)) => self.add_incoming(*ty, *amount)?,
            Some(IntentEffect::Outgoing(ty, amount)) => self.add_outgoing(*ty, *amount)?,
            None => { },
        }

        let amount = match &intent.effect {
            Some(IntentEffect::Incoming(_, amount) | IntentEffect::Outgoing(_, amount)) => *amount,
            None => 0,
        };

        self.intents.insert(ty, intent);

        Ok(amount)
    }

    fn add_outgoing(&mut self, ty: ResourceType, amount: u32) -> Result<(), IntentError> {
        let resource = self.resources.entry(ty).or_insert_with(|| self.creep.used_capacity(Some(ty)));
        if amount > *resource { return Err(IntentError::NotEnoughResource { resource: ty, target: amount, curr: *resource }); }

        *resource -= amount;
        self.total_resources -= amount;
        self.total_outgoing_resources += amount;

        Ok(())
    }

    fn add_incoming(&mut self, ty: ResourceType, amount: u32) -> Result<(), IntentError> {
        if amount > self.free_capacity { return Err(IntentError::NotEnoughCapacity { resource: ty, curr: self.free_capacity, target: amount }) }

        self.free_capacity -= amount;
        *self.incoming_resources.entry(ty).or_default() += amount;
        self.total_incoming_resources += amount;

        Ok(())
    }

    pub fn cancel_intent(&mut self, intent: IntentType) -> bool {
        let Some(intent) = self.intents.remove(&intent) else { return false };

        match intent.effect {
            Some(IntentEffect::Incoming(ty, amount)) => {
                self.free_capacity += amount;
                *self.incoming_resources.get_mut(&ty).unwrap() -= amount;
                self.total_incoming_resources -= amount;
            },
            Some(IntentEffect::Outgoing(ty, amount)) => {
                let resource = self.resources.get_mut(&ty).unwrap();

                *resource += amount;
                self.total_resources += amount;
                self.total_outgoing_resources -= amount;
            },
            None => { }
        }

        true
    }

    fn part_amount(&self, part: Part, per_part: u32) -> u32 {
        u32::from(self.creep.get_active_bodyparts(part)) * per_part
    }

    fn get_resource(&self, ty: ResourceType) -> u32 {
        self.resources.get(&ty).copied().unwrap_or_else(|| self.creep.used_capacity(Some(ty)))
    }

    fn get_energy(&self) -> u32 {
        self.get_resource(ResourceType::Energy)
    }

    pub fn capacity(&self) -> u32 {
        self.creep.capacity(None)
    }

    // Free capacity left this tick
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

    pub fn incoming(&self, ty: Option<ResourceType>) -> u32 {
        if let Some(ty) = ty {
            self.incoming_resources.get(&ty).map_or(0, |x| *x)
        } else {
            self.total_incoming_resources
        }
    }

    pub fn outgoing(&self) -> u32 {
        self.total_outgoing_resources
    }

    #[expect(unused)]
    pub fn curr_used_energy_capacity(&self) -> u32 { self.curr_used_capacity(Some(ResourceType::Energy)) }
    pub fn next_used_energy_capacity(&self) -> u32 { self.next_used_capacity(Some(ResourceType::Energy)) }
    pub fn incoming_energy(&self) -> u32 { self.incoming(Some(ResourceType::Energy)) }
    
    pub fn build(&mut self, target: ConstructionSite) -> Result<u32, IntentError> {
        let amount = self.part_amount(Part::Work, 5)
            .min(target.progress_total() - target.progress())
            .min(self.get_energy());

        self.register_intent(
            IntentType::Build,
            Intent::new(
                move |creep| creep.build(&target).map_err(anyhow::Error::new),
                Some(IntentEffect::outgoing_energy(amount))
            )
        )
    }

    #[expect(unused)]
    pub fn drop(&mut self, ty: ResourceType, amount: Option<u32>) -> Result<u32, IntentError> {
        let amount = amount.unwrap_or(self.get_resource(ty));

        self.register_intent(
            IntentType::Drop,
            Intent::new(
                move |creep| creep.drop(ty, Some(amount)).map_err(anyhow::Error::new),
                    Some(IntentEffect::Outgoing(ty, amount))
            )
        )
    }

    pub fn harvest_source(&mut self, source: Source) -> Result<u32, IntentError> {
        let amount = self.part_amount(Part::Work, 2)
            .min(source.energy())
            .min(self.free_capacity);

        self.register_intent(
            IntentType::Harvest,
            Intent::new(
                move |creep| creep.harvest(&source).map_err(anyhow::Error::new),
                Some(IntentEffect::incoming_energy(amount))
            )
        )
    }

    pub fn pickup(&mut self, target: Resource) -> Result<u32, IntentError> {
        let ty = target.resource_type();
        let amount = target.amount()
            .min(self.free_capacity);

        self.register_intent(
            IntentType::Pickup,
            Intent::new(
                move |creep| creep.pickup(&target).map_err(anyhow::Error::new),
                Some(IntentEffect::Incoming(ty, amount))
            )
        )
    }

    #[expect(unused)]
    pub fn repair(&mut self, target: impl Repairable + Sized + 'static) -> Result<u32, IntentError> {
        let amount = self.part_amount(Part::Work, 1)
            .min((target.hits_max() - target.hits()).div_ceil(100))
            .min(self.get_energy());

        self.register_intent(
            IntentType::Repair,
            Intent::new(
                move |creep| creep.repair(&target).map_err(anyhow::Error::new),
                Some(IntentEffect::outgoing_energy(amount))
            )
        )
    }

    // Transfer from other creep into this creep
    // TODO: Make this cancellable?
    pub fn transfer_from(&mut self, target: &WithId<Creep>, ty: ResourceType, amount: Option<u32>) -> Result<u32, IntentError> {
        let amount = amount.unwrap_or(self.free_capacity)
            .min(target.used_capacity(Some(ty)));

        self.add_incoming(ty, amount)?;
        target.transfer(&*self.creep, ty, Some(amount)).map_err(anyhow::Error::new)?;
        Ok(amount)
    }

    pub fn transfer(&mut self, target: impl Transferable + Sized + 'static, ty: ResourceType, amount: Option<u32>) -> Result<u32, IntentError> {
        let amount = amount.unwrap_or(self.get_resource(ty))
            .min(target.free_capacity(Some(ty)));
        
        self.register_intent(
            IntentType::Transfer,
            Intent::new(
                move |creep| creep.transfer(target.transferable(), ty, Some(amount)).map_err(anyhow::Error::new),
                Some(IntentEffect::Outgoing(ty, amount))
            )
        )
    }

    pub fn withdraw(&mut self, target: impl Withdrawable + Sized + 'static, ty: ResourceType, amount: Option<u32>) -> Result<u32, IntentError> {
        let amount = amount.unwrap_or(self.free_capacity)
            .min(target.used_capacity(Some(ty)));
        
        self.register_intent(
            IntentType::Withdraw,
            Intent::new(
                move |creep| creep.withdraw(target.withdrawable(), ty, Some(amount)).map_err(anyhow::Error::new),
                Some(IntentEffect::Incoming(ty, amount))
            )
        )
    }
}

impl MovementRequests {
    pub fn move_vcreep_to(&mut self, creep: &mut VirtualCreep, target: Position, range: u32) -> Result<MoveToResult, IntentError> {
        if creep.has_intent(IntentType::Move) { return Err(IntentError::AlreadyScheduled(IntentType::Move)) }

        let result = self.move_creep_to(&creep.creep, target, range);
        if !result.in_range() {
            creep.register_intent(IntentType::Move, Intent::new(|_| Ok(()), None))?;
        }
        
        Ok(result)
    }

    pub fn move_vtugged_to(&mut self, creep: &mut VirtualCreep, target: Position, range: u32) -> Result<MoveToResult, IntentError> {
        if creep.has_intent(IntentType::Move) { return Err(IntentError::AlreadyScheduled(IntentType::Move)) }
        
        let result = self.move_tugged_to(&creep.creep, target, range);
        if !result.in_range() {
            creep.register_intent(IntentType::Move, Intent::new(|_| Ok(()), None))?;
        }
        
        Ok(result)
    }
}
