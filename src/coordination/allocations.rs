use std::hash::Hash;

use derive_where::derive_where;
use screeps::Creep;
use serde::{Deserialize, Serialize};

use crate::{check::{Check, CheckFrom, FilterCheck, FilterCheckFrom}, coordination::{tasks::UpdateableTaskData, expiring_map::{ExpiringEntryCheckError, LiveHandle, ExpiringMap}}, domain_traits::HasName, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

#[derive(Serialize, Deserialize)]
struct Allocation<AllocationData> {
    amount: u32,
    data: AllocationData
}

struct AllocationCheckErr<AllocationData: CheckFrom> {
    amount: u32,
    err: AllocationData::Err
}

impl<AD: CheckFrom> CheckFrom for Allocation<AD> {
    type Unchecked = Allocation<AD::Unchecked>;
    type Err = AllocationCheckErr<AD>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Self { 
            amount: uc.amount, 
            data: uc.data.check()
                .map_err(|err| 
                    AllocationCheckErr { 
                        amount: uc.amount, 
                        err
                    }
                )? 
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ResourceState {
    amount: u32,
    reserved: u32
}

#[derive_where(Serialize, Deserialize; ExpiringMap<Owner, Allocation<AllocationData>, 1, S>)]
pub struct Allocations<Owner, AllocationData = (), S: CheckState = Checked> {
    allocations: ExpiringMap<Owner, Allocation<AllocationData>, 1, S>,
    state: ResourceState
}

pub type CreepAllocations<AllocationData = (), S = Checked> = Allocations<Handle<WithId<Creep>>, AllocationData, S>;

impl<O, AD> Allocations<O, AD> {
    pub fn new(amount: u32) -> Self {
        Self { 
            allocations: ExpiringMap::new(),
            state: ResourceState { 
                amount, 
                reserved: 0
            }
        }
    }

    pub fn unreserved_amount(&self) -> u32 {
        self.state.amount.saturating_sub(self.state.reserved)
    }

    pub fn set_amount(&mut self, amount: u32) {
        self.state.amount = amount;
    }
}

impl<Owner: Hash + Eq, AllocationData> Allocations<Owner, AllocationData> {
    pub fn refresh(&mut self, owner: Owner) -> Option<AllocationHandle<'_, Owner, AllocationData>> {
        Some(AllocationHandle {
            live_handle: self.allocations.refresh(owner)?,
            resource_state: &mut self.state
        })
    }

    pub fn allocate(&mut self, owner: Owner, amount: u32, data: AllocationData) {
        if let Some(other_state) = self.allocations.insert(owner, Allocation { amount, data }) {
            self.state.reserved -= other_state.amount;
        }

        self.state.reserved += amount;
    }
}

pub struct ResourceAmount(pub u32);
impl<Owner, AllocationData> UpdateableTaskData for Allocations<Owner, AllocationData> {
    type Update = ResourceAmount;

    fn update(&mut self, update: Self::Update) {
        self.set_amount(update.0);
    }

    fn create(update: Self::Update) -> Self {
        Self::new(update.0)
    }
}

impl<Owner, AllocationData> FilterCheckFrom for Allocations<Owner, AllocationData> 
where 
    AllocationData: CheckFrom,
    Owner: CheckFrom + Hash + Eq + HasName
{
    type Unchecked = Allocations<Owner::Unchecked, AllocationData::Unchecked, Unchecked>;
    type Err = ExpiringEntryCheckError<Owner, AllocationData>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (allocations, errs) = uc.allocations.filter_check();

        let mut checked = Self { 
            allocations,
            state: uc.state
        };

        let mut new_errs = Vec::new();
        for err in errs {
            let (amount, new_err) = match err {
                ExpiringEntryCheckError::Key(worker_err, worker_state) => {
                    (worker_state.amount, ExpiringEntryCheckError::Key(worker_err, worker_state.data))
                },
                ExpiringEntryCheckError::Value(owner, worker_state_err) => {
                    (worker_state_err.amount, ExpiringEntryCheckError::Value(owner, worker_state_err.err))
                },
                ExpiringEntryCheckError::Expired(owner, worker_state) => 
                    (worker_state.amount, ExpiringEntryCheckError::Expired(owner, worker_state.data)),
            };

            checked.state.reserved -= amount;
            new_errs.push(new_err);
        }

        (checked, new_errs)
    }
}

pub struct AllocationHandle<'a, Owner = Handle<WithId<Creep>>, AllocationData = ()> {
    resource_state: &'a mut ResourceState,
    live_handle: LiveHandle<'a, Owner, Allocation<AllocationData>>
}

pub type CreepAllocationHandle<'a, AllocationData = ()> = AllocationHandle<'a, Handle<WithId<Creep>>, AllocationData>;

impl<Owner, AllocationData> AllocationHandle<'_, Owner, AllocationData> {
    pub fn consume(&mut self, amount: u32) {
        self.resource_state.reserved = self.resource_state.reserved.saturating_sub(amount);
        self.resource_state.amount = self.resource_state.amount.saturating_sub(amount);
        self.live_handle.get_mut().amount = self.live_handle.get().amount.saturating_sub(amount);
    }

    pub fn release(self) {
        self.resource_state.reserved -= self.live_handle.get().amount;
        self.live_handle.remove();
    }

    pub fn reserved(&self) -> u32 {
        self.live_handle.get().amount
    }

    #[expect(unused)]
    pub fn get(&self) -> &AllocationData {
        &self.live_handle.get().data
    }

    #[expect(unused)]
    pub fn get_mut(&mut self) -> &mut AllocationData {
        &mut self.live_handle.get_mut().data
    }
}