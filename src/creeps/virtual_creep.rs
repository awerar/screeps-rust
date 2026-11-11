use std::{collections::HashMap, error::Error};

use anyhow::{Result, bail};
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasPosition, Part, Position, Resource, ResourceType, SharedCreepProperties, Source, StructureController, StructureSpawn};

use crate::{domain_traits::{HasStoreExt, Repairable, Transferable, Withdrawable}, ids::{GetHandle, Handle}, movement::{MoveTarget, requests::{MoveRequest, MoveRequestType, MovementRequests}}, spawn::prototype::Body, statemachine::Flow};

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
    creep: Creep,

    free_capacity: u32, // Free capacity left this tick
    total_resources: u32, // Used capacity left this tick

    total_incoming_resources: u32,
    total_outgoing_resources: u32,

    resources: HashMap<ResourceType, u32>, // Amount left this tick per resource. Lazily initialized
    incoming_resources: HashMap<ResourceType, u32>,
    
    intents: HashMap<IntentType, Intent>,
    move_request: Option<MoveRequest>
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

trait CommitFn<Err = anyhow::Error> = FnOnce(&Creep) -> Result<(), Err>;
struct Intent {
    effect: Option<IntentEffect>,
    commit: Box<dyn CommitFn>
}

impl Intent {
    fn new<Err>(commit: impl CommitFn<Err> + 'static, effect: Option<IntentEffect>) -> Self
    where 
        Err: Error + Send + Sync + 'static
    {
        Intent { 
            effect,
            commit: Box::new(|creep| commit(creep).map_err(anyhow::Error::new)) 
        }
    }

    fn empty() -> Self {
        Self { effect: None, commit: Box::new(|_| { Ok(()) }) }
    }
}

impl VirtualCreep {
    pub fn new(creep: Creep) -> Self {
        VirtualCreep { 
            free_capacity: creep.free_capacity(None),
            total_resources: creep.used_capacity(None),
            total_incoming_resources: 0,
            total_outgoing_resources: 0,
            resources: HashMap::new(),
            incoming_resources: HashMap::new(),
            creep,
            intents: HashMap::new(),
            move_request: None
        }
    }

    pub fn handle(&self) -> Handle<Creep> {
        self.creep.handle()
    }

    pub fn pos(&self) -> Position {
        self.creep.pos()
    }

    pub fn name(&self) -> String {
        self.creep.name()
    }

    pub fn body(&self) -> Body {
        Body::of_creep(&self.creep)
    }

    pub fn ticks_to_live(&self) -> u32 {
        self.creep.ticks_to_live().unwrap()
    }

    pub fn has_intent(&self, intent: IntentType) -> bool {
        self.intents.contains_key(&intent)
    }

    pub fn can_do(&self, intent: IntentType) -> bool {
        !self.has_intent(intent) &&
        self.pipeline_free(intent, PIPELINE_A) &&
        self.pipeline_free(intent, PIPELINE_B)
    }
    
    pub fn commit(self, movement: &mut MovementRequests) -> Result<()> {
        if let Some(request) = self.move_request {
            movement.request(&self.creep, request);
        }

        for intent in self.intents.into_values() {
            (intent.commit)(&self.creep)?;
        }

        Ok(())
    }

    fn pipeline_free<const N: usize>(&self, intent: IntentType, pipeline: [IntentType; N]) -> bool {
        !pipeline.contains(&intent) || !pipeline.iter().any(|other| self.has_intent(*other))
    }

    fn register_intent(&mut self, ty: IntentType, intent: Intent) -> Flow<u32> {
        if !self.can_do(ty) { return Flow::Break }

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

        Flow::Continue(amount)
    }

    fn add_outgoing(&mut self, ty: ResourceType, amount: u32) -> Result<()> {
        let resource = self.resources.entry(ty).or_insert_with(|| self.creep.used_capacity(Some(ty)));
        if amount > *resource { bail!("Not enough {ty}: {} out of {amount}", *resource) }

        *resource -= amount;
        self.total_resources -= amount;
        self.total_outgoing_resources += amount;

        Ok(())
    }

    fn add_incoming(&mut self, ty: ResourceType, amount: u32) -> Result<()> {
        if amount > self.free_capacity { bail!("Not enough free capacity for {ty}: {} out of {amount}", self.free_capacity) }

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

    pub fn curr_used_energy_capacity(&self) -> u32 { self.curr_used_capacity(Some(ResourceType::Energy)) }
    pub fn next_used_energy_capacity(&self) -> u32 { self.next_used_capacity(Some(ResourceType::Energy)) }
    pub fn incoming_energy(&self) -> u32 { self.incoming(Some(ResourceType::Energy)) }
    
    pub fn build(&mut self, target: ConstructionSite) -> Flow<u32> {
        let amount = self.part_amount(Part::Work, 5)
            .min(target.progress_total() - target.progress())
            .min(self.get_energy());

        self.register_intent(
            IntentType::Build,
            Intent::new(
                move |creep| creep.build(&target),
                Some(IntentEffect::outgoing_energy(amount))
            )
        )
    }

    pub fn claim_controller(&mut self, target: StructureController) -> Flow {
        self.register_intent(
            IntentType::ClaimController, 
            Intent::new(move |creep| creep.claim_controller(&target), None)
        ).map_continue(|_| ())
    }

    #[expect(unused)]
    pub fn drop(&mut self, ty: ResourceType, amount: Option<u32>) -> Flow<u32> {
        let amount = amount.unwrap_or(self.get_resource(ty));

        self.register_intent(
            IntentType::Drop,
            Intent::new(
                move |creep| creep.drop(ty, Some(amount)),
                    Some(IntentEffect::Outgoing(ty, amount))
            )
        )
    }

    pub fn harvest_source(&mut self, source: Source) -> Flow<u32> {
        let amount = self.part_amount(Part::Work, 2)
            .min(source.energy())
            .min(self.free_capacity);

        self.register_intent(
            IntentType::Harvest,
            Intent::new(
                move |creep| creep.harvest(&source),
                Some(IntentEffect::incoming_energy(amount))
            )
        )
    }

    pub fn pickup(&mut self, target: Resource) -> Flow<u32> {
        let ty = target.resource_type();
        let amount = target.amount()
            .min(self.free_capacity);

        self.register_intent(
            IntentType::Pickup,
            Intent::new(
                move |creep| creep.pickup(&target),
                Some(IntentEffect::Incoming(ty, amount))
            )
        )
    }

    pub fn upgrade_controller(&mut self, target: StructureController) -> Flow<u32> {
        let amount = self.part_amount(Part::Work, 1)
            .min(if target.level() == 8 { 15 } else { u32::MAX })
            .min(self.get_energy());

        self.register_intent(
            IntentType::UpgradeController, 
            Intent::new(
                move |creep| creep.upgrade_controller(&target), 
                Some(IntentEffect::outgoing_energy(amount))
            )
        )
    }

    pub fn repair(&mut self, target: impl Repairable + Sized + 'static) -> Flow<u32> {
        let amount = self.part_amount(Part::Work, 1)
            .min((target.hits_max() - target.hits()).div_ceil(100))
            .min(self.get_energy());

        self.register_intent(
            IntentType::Repair,
            Intent::new(
                move |creep| target.repair_by(creep),
                Some(IntentEffect::outgoing_energy(amount))
            )
        )
    }

    // Transfer from other creep into this creep
    // TODO: Make this cancellable?
    pub fn transfer_from(&mut self, target: &Creep, ty: ResourceType, amount: Option<u32>) -> Flow<u32> {
        let amount = amount.unwrap_or(self.free_capacity)
            .min(target.used_capacity(Some(ty)));

        self.add_incoming(ty, amount)?;
        target.transfer(&self.creep, ty, Some(amount))?;
        Flow::Continue(amount)
    }

    pub fn transfer(&mut self, target: impl Transferable + Sized + 'static, ty: ResourceType, amount: Option<u32>) -> Flow<u32> {
        let amount = amount.unwrap_or(self.get_resource(ty))
            .min(target.free_capacity(Some(ty)));
        
        self.register_intent(
            IntentType::Transfer,
            Intent::new(
                move |creep| target.transfer_from(creep, ty, Some(amount)),
                Some(IntentEffect::Outgoing(ty, amount))
            )
        )
    }

    pub fn withdraw(&mut self, target: impl Withdrawable + Sized + 'static, ty: ResourceType, amount: Option<u32>) -> Flow<u32> {
        let amount = amount.unwrap_or(self.free_capacity)
            .min(target.used_capacity(Some(ty)));
        
        self.register_intent(
            IntentType::Withdraw,
            Intent::new(
                move |creep| target.withdraw_to(creep, ty, Some(amount)),
                Some(IntentEffect::Incoming(ty, amount))
            )
        )
    }

    pub fn recycle(&self, spawn: &StructureSpawn) -> Result<(), anyhow::Error> {
        spawn.recycle_creep(&self.creep).map_err(Into::into)
    }

    pub fn approach(&mut self, target: Position, range: u32) -> Flow {
        self.request_move(target, range, MoveRequestType::Single)
    }

    pub fn approach_tugged(&mut self, target: Position, range: u32) -> Flow {
        self.request_move(target, range, MoveRequestType::Tugged)
    }

    fn request_move(&mut self, target: Position, range: u32, ty: MoveRequestType) -> Flow {
        if self.has_intent(IntentType::Move) { return Flow::Break }

        let target = MoveTarget { target, range };
        let in_range = target.in_range(self.pos());
        self.move_request = Some(MoveRequest { target, ty });
        if in_range { return Flow::Continue(()) }

        self.register_intent(IntentType::Move, Intent::empty())?;
        Flow::Break
    }
}
