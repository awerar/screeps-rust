use option_entry::OptionEntry;
use screeps::Creep;
use serde::{Deserialize, Serialize};

use crate::{check::{Check, CheckFrom, Expiring, ExpiringCheckError, FilterCheckFrom}, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

#[derive(Serialize, Deserialize)]
pub struct Assignment<Owner, Data, S: CheckState = Checked>(
    Option<Expiring<(Owner, Data), 1, S>>
);

pub type CreepAssignment<Data> = Assignment<Handle<WithId<Creep>>, Data>;

pub struct AssignmentHandle<'a, Owner, Data>(
    option_entry::OccupiedEntry<'a, Expiring<(Owner, Data), 1>>
);

pub type CreepAssignmentHandle<'a, Data> = AssignmentHandle<'a, Handle<WithId<Creep>>, Data>;

impl<Owner, Data> Assignment<Owner, Data> {
    pub fn new() -> Self {
        Assignment(None)
    }

    pub fn refresh(&mut self) -> Option<AssignmentHandle<'_, Owner, Data>> {
        match self.0.entry() {
            option_entry::Entry::Vacant(_) => None,
            option_entry::Entry::Occupied(mut entry) => {
                entry.get_mut().refresh();
                Some(AssignmentHandle(entry))
            },
        }
    }

    pub fn assign(&mut self, owner: Owner, data: Data) {
        self.0 = Some(Expiring::new((owner, data )));
    }

    pub fn is_free(&self) -> bool {
        self.0.is_none()
    }
}

impl<Owner, Data> AssignmentHandle<'_, Owner, Data> {
    pub fn release(self) {
        self.0.remove();
    }

    pub fn get(&self) -> &Data {
        &self.0.get().1
    }

    pub fn get_mut(&mut self) -> &mut Data {
        &mut self.0.get_mut().1
    }
}

impl<O: CheckFrom, D: CheckFrom> CheckFrom for Assignment<O, D> {
    type Unchecked = Assignment<O::Unchecked, D::Unchecked, Unchecked>;
    type Err = ExpiringCheckError<(O, D)>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Assignment(uc.0.check()?))
    }
}

impl<O: CheckFrom, D: CheckFrom> FilterCheckFrom for Assignment<O, D> {
    type Unchecked = Assignment<O::Unchecked, D::Unchecked, Unchecked>;
    type Err = ExpiringCheckError<(O, D)>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        match uc.check() {
            Ok(checked) => (checked, vec![]),
            Err(err) => (Assignment::new(), vec![err]),
        }
    }
}