use std::{collections::HashMap, iter, ops::{Add, Mul}};

use screeps::{CREEP_CLAIM_LIFE_TIME, CREEP_LIFE_TIME, Creep, MAX_CREEP_SIZE, Part, RoomName};

use crate::{creeps::{CreepData, CreepRole}, domain_traits::{CreepId, ResolvableId}};

#[derive(Clone)]
pub struct Body(Vec<Part>);

#[derive(Clone, Copy)]
pub enum ScaleStopCondition {
    PartCount(Part, u32),
    None
}

impl Body {
    pub fn scaled(&self, energy: u32, min_parts: Option<usize>) -> Option<Body> {
        self.scaled_with_stop(energy, min_parts, ScaleStopCondition::None)
    }

    pub fn scaled_with_stop(&self, energy: u32, min_parts: Option<usize>, stop_condition: ScaleStopCondition) -> Option<Body> {
        let min_parts = min_parts.unwrap_or(self.0.len());
   
        let mut part_type_counts: HashMap<_, u32> = HashMap::new();
        let mut counts: Vec<usize> = vec![0; self.0.len()];
        let mut cost = 0;
        let mut part_count = 0;

        loop {
            for (i, part )in self.0.iter().enumerate() {
                cost += part.cost();

                let stop_condition_trigged = match stop_condition {
                    ScaleStopCondition::PartCount(part, max_part_count) => part_type_counts.get(&part).is_some_and(|part_type_count| *part_type_count >= max_part_count),
                    ScaleStopCondition::None => false,
                };

                if part_count >= MAX_CREEP_SIZE || energy < cost || stop_condition_trigged  {
                    return (part_count as usize >= min_parts).then(|| {
                        Body(self.0.iter()
                        .zip(counts)
                        .flat_map(|(part, count)| vec![*part; count].into_iter())
                        .collect())
                    });
                }

                counts[i] += 1;
                part_count += 1;
                *part_type_counts.entry(*part).or_default() += 1;
            }
        }
    }

    pub fn of_creep(creep: &Creep) -> Self {
        Body(creep.body().into_iter().map(|bodypart| bodypart.part()).collect())
    }

    pub fn of_part(part: Part, count: usize) -> Self {
        Body(iter::repeat_n(part, count).collect())
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

    pub fn life_time(&self) -> u32 {
        if self.part_count(Part::Claim) > 0 { CREEP_CLAIM_LIFE_TIME } else { CREEP_LIFE_TIME }
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

impl From<Vec<Part>> for Body {
    fn from(parts: Vec<Part>) -> Self {
        Body(parts)
    }
}

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
            body: Body(id.resolve().body().into_iter().map(|part| part.part()).collect()),
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

    pub fn spawn_room(&self) -> RoomName {
        self.spawn_room
    }
}

#[derive(Clone)]
pub struct AbsolutePrototype {
    proto: RelativePrototype,
    home: RoomName
}

impl AbsolutePrototype {
    pub fn new(body: Body, role: CreepRole, home: RoomName, spawn_room: RoomName) -> Self {
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

    pub fn home(&self) -> RoomName {
        self.home
    }

    pub fn relative(&self) -> &RelativePrototype {
        &self.proto
    }
}
