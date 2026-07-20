use std::{iter, ops::{Add, Mul}};

use screeps::{Creep, MAX_CREEP_SIZE, Part, RoomName};

use crate::{creeps::{CreepData, CreepRole}, domain_traits::{CreepId, ResolvableId}};

#[derive(Clone)]
pub struct Body(Vec<Part>);

impl Body {
    pub fn scaled(&self, energy: u32, min_parts: Option<usize>) -> Body {
        let min_parts = min_parts.unwrap_or(self.0.len());

        let mut counts: Vec<usize> = vec![0; self.0.len()];
        let mut cost = 0;
        let mut part_count = 0;

        loop {
            for (i, part )in self.0.iter().enumerate() {
                cost += part.cost();

                if part_count >= MAX_CREEP_SIZE || (energy < cost && part_count as usize >= min_parts)  {
                    return Body(self.0.iter()
                        .zip(counts)
                        .flat_map(|(part, count)| vec![*part; count].into_iter())
                        .collect());
                }

                counts[i] += 1;
                part_count += 1;
            }
        }
    }

    pub fn energy_required(&self) -> u32 {
        self.0.iter().map(|part| part.cost()).sum()
    }

    pub fn part_count(&self, part: Part) -> usize {
        self.0.iter().filter(|p| **p == part).count()
    }

    pub fn parts(&self) -> &[Part] {
        &self.0
    }

    pub fn total_parts(&self) -> usize {
        self.0.len()
    }
}

impl Mul<usize> for Body {
    type Output = Self;

    fn mul(self, rhs: usize) -> Self::Output {
        Body(self.0.into_iter().flat_map(|part| iter::repeat_n(part, rhs)).collect())
    }
}

impl Add for Body {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Body(self.0.into_iter().chain(rhs.0).collect())
    }
}

impl From<Part> for Body {
    fn from(value: Part) -> Self {
        Body(vec![value])
    }
}

impl From<Vec<Part>> for Body {
    fn from(parts: Vec<Part>) -> Self {
        Body(parts)
    }
}

impl From<&Creep> for Body {
    fn from(value: &Creep) -> Self {
        Body(value.body().into_iter().map(|bodypart| bodypart.part()).collect())
    }
}

#[derive(Clone)]
pub struct RelativePrototype {
    body: Body,
    role: CreepRole,
}

impl RelativePrototype {
    pub fn new(body: Body, role: CreepRole) -> Self {
        Self { body, role }
    }

    pub fn from_creep(id: &CreepId, data: &CreepData) -> Self {
        Self {
            body: Body(id.resolve().body().into_iter().map(|part| part.part()).collect()),
            role: data.role.clone(),
        }
    }

    pub fn with_home(self, home: RoomName) -> AbsolutePrototype {
        AbsolutePrototype { proto: self, home }
    }

    pub fn body(&self) -> &Body {
        &self.body
    }

    pub fn role(&self) -> &CreepRole {
        &self.role
    }
}

#[derive(Clone)]
pub struct AbsolutePrototype {
    proto: RelativePrototype,
    home: RoomName
}

impl AbsolutePrototype {
    pub fn new(body: Body, role: CreepRole, home: RoomName) -> Self {
        Self { proto: RelativePrototype { body, role }, home }
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

    pub fn home(&self) -> RoomName {
        self.home
    }

    pub fn relative(&self) -> &RelativePrototype {
        &self.proto
    }
}

pub enum Prototype {
    Absolute(AbsolutePrototype),
    Relative(RelativePrototype)
}

impl Prototype {
    pub fn relative(body: Body, role: CreepRole) -> Self {
        Prototype::Relative(RelativePrototype::new(body, role))
    }

    pub fn absolute(body: Body, role: CreepRole, home: RoomName) -> Self {
        Prototype::Absolute(AbsolutePrototype::new(body, role, home))
    }

    pub fn with_default_home(self, home: RoomName) -> AbsolutePrototype {
        match self {
            Prototype::Absolute(proto) => proto,
            Prototype::Relative(proto) => AbsolutePrototype { proto, home },
        }
    }
}
