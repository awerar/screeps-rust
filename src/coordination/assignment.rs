use option_entry::OptionEntry;
use screeps::Creep;

use crate::{check::Expiring, ids::{Handle, WithId}};

struct AssignmentState<Owner, Data> {
    owner: Owner,
    data: Data,
}

pub struct Assignment<Owner, Data, const LIFETIME: u32>(
    Option<Expiring<AssignmentState<Owner, Data>, LIFETIME>>
);

pub type CreepAssignment<Data, const LIFETIME: u32> = Assignment<Handle<WithId<Creep>>, Data, LIFETIME>;

pub struct AssignmentHandle<'a, Owner, Data, const LIFETIME: u32>(
    option_entry::OccupiedEntry<'a, Expiring<AssignmentState<Owner, Data>, LIFETIME>>
);

pub type CreepAssignmentHandle<'a, Data, const LIFETIME: u32> = AssignmentHandle<'a, Handle<WithId<Creep>>, Data, LIFETIME>;

impl<Owner, Data, const LT: u32> Assignment<Owner, Data, LT> {
    pub fn refresh(&mut self) -> Option<AssignmentHandle<'_, Owner, Data, LT>> {
        match self.0.entry() {
            option_entry::Entry::Vacant(_) => None,
            option_entry::Entry::Occupied(mut entry) => {
                entry.get_mut().refresh();
                Some(AssignmentHandle(entry))
            },
        }
    }

    pub fn assign(&mut self, owner: Owner, data: Data) {
        self.0 = Some(Expiring::new(AssignmentState { owner, data }));
    }

    pub fn is_free(&self) -> bool {
        self.0.is_none()
    }
}

impl<Owner, Data, const LT: u32> AssignmentHandle<'_, Owner, Data, LT> {
    pub fn release(self) {
        self.0.remove();
    }

    pub fn get(&self) -> &Data {
        &self.0.get().data
    }

    pub fn get_mut(&mut self) -> &mut Data {
        &mut self.0.get_mut().data
    }
}