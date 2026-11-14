use screeps::RoomName;

use crate::{colony::ColonyName, creeps::{CreepData, CreepRole, body::Body}, domain_traits::{CreepId, ResolvableId}, ids::Unchecked};

#[derive(Clone)]
pub struct RelativePrototype {
    body: Body,
    role: CreepRole,
    spawn_room: RoomName
}

impl RelativePrototype {
    pub fn new(body: Body, role: CreepRole, spawn_room: RoomName) -> Self {
        Self { body, role, spawn_room }
    }

    pub fn from_creep(id: &CreepId, data: &CreepData) -> Self {
        Self {
            body: Body::of_creep(&id.resolve()),
            role: data.role.clone(),
            spawn_room: data.spawn_room
        }
    }

    pub fn body(&self) -> &Body {
        &self.body
    }

    pub fn role(&self) -> &CreepRole {
        &self.role
    }

    #[expect(unused)]
    pub fn spawn_room(&self) -> RoomName {
        self.spawn_room
    }
}

#[derive(Clone)]
pub struct AbsolutePrototype {
    proto: RelativePrototype,
    home: ColonyName<Unchecked>
}

impl AbsolutePrototype {
    pub fn new(body: Body, role: CreepRole, home: ColonyName<Unchecked>, spawn_room: RoomName) -> Self {
        Self { proto: RelativePrototype::new(body, role, spawn_room), home }
    }

    pub fn from_creep(id: &CreepId, data: &CreepData) -> Self {
        Self {
            proto: RelativePrototype::from_creep(id, data),
            home: data.home
        }
    }

    pub fn body(&self) -> &Body {
        &self.proto.body
    }

    pub fn role(&self) -> &CreepRole {
        &self.proto.role
    }

    pub fn home(&self) -> ColonyName<Unchecked> {
        self.home
    }

    pub fn spawn_room(&self) -> RoomName {
        self.proto.spawn_room
    }

    pub fn relative(&self) -> &RelativePrototype {
        &self.proto
    }
}
