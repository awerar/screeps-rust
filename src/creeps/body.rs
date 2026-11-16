use std::{collections::{HashMap, HashSet}, iter, ops::{Add, Mul}};

use itertools::Itertools;
use screeps::{CREEP_CLAIM_LIFE_TIME, CREEP_LIFE_TIME, Creep, MAX_CREEP_SIZE, Part};

#[derive(Clone)]
pub struct Body(Vec<Part>);

#[derive(Clone, Copy)]
pub enum ScaleStopCondition<'a> {
    PartCount(Part, u32),
    Drain { max_drain: f32, part_drains: &'a [(Part, f32)] }
}

impl Body {
    pub fn scaled(&self, energy: u32) -> Option<Body> {
        self.scaled_with_stop(energy, &[])
    }

    pub fn scaled_with_stop(&self, energy: u32, stop_conditions: &[ScaleStopCondition<'_>]) -> Option<Body> {
        let life_time = self.life_time();

        let mut part_type_counts: HashMap<_, u32> = HashMap::new();
        let mut part_types_left: HashSet<_> = self.parts().iter().unique().copied().collect();

        let mut counts: Vec<usize> = vec![0; self.0.len()];
        let mut cost = 0;
        let mut part_count = 0;

        loop {
            for (i, part )in self.0.iter().enumerate() {
                cost += part.cost();

                let stop_condition_trigged = stop_conditions.iter().any(|condition| match *condition {
                    ScaleStopCondition::PartCount(part, max_part_count) => part_type_counts.get(&part).copied().unwrap_or_default() >= max_part_count,
                    ScaleStopCondition::Drain { max_drain, part_drains } => {
                        let parts_drain = part_drains.iter()
                            .map(|(drain_part, part_drain)| {
                                let count_with_next = part_type_counts.get(drain_part).copied().unwrap_or_default() + u32::from(part == drain_part);
                                count_with_next as f32 * part_drain
                            })
                            .sum::<f32>();

                        cost as f32 / life_time as f32 + parts_drain > max_drain
                    }
                });

                if part_count >= MAX_CREEP_SIZE || energy < cost || stop_condition_trigged  {
                    return part_types_left.is_empty().then(|| {
                        Body(self.0.iter()
                        .zip(counts)
                        .flat_map(|(part, count)| vec![*part; count].into_iter())
                        .collect())
                    });
                }

                counts[i] += 1;
                part_count += 1;
                *part_type_counts.entry(*part).or_default() += 1;
                part_types_left.remove(part);
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

    pub fn energy_drain(&self) -> f32 {
        self.energy_required() as f32 / self.life_time() as f32
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

    pub fn spawn_ticks(&self) -> u32 {
        3 * self.0.len() as u32
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
