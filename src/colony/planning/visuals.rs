use std::collections::HashSet;

use itertools::Itertools;
use screeps::{CircleStyle, LineStyle, RoomName, RoomVisual, RoomXY, StructureType, TextStyle};

use crate::{colony::planning::{steps::{ColonyState, State}, plan::ColonyPlan}, visuals::draw_in_room};

pub fn draw_roads(visuals: &RoomVisual, roads: &HashSet<RoomXY>) {
    let connections: HashSet<_> = roads.iter()
        .flat_map(|road| 
            road.neighbors().into_iter()
                .filter(|neigh| roads.contains(neigh))
                .map(|neigh| 
                    vec![*road, neigh].into_iter()
                        .sorted()
                        .collect_tuple::<(_, _)>()
                        .unwrap()
                )
        ).collect();

    for (a, b) in connections.into_iter() {
        let a = (a.x.u8() as f32, a.y.u8() as f32);
        let b = (b.x.u8() as f32, b.y.u8() as f32);

        visuals.line(a, b, Some(LineStyle::default().opacity(0.75).width(0.2).color("#335882")));
    }
}

impl ColonyPlan {
    pub fn draw_until(&self, visuals: &RoomVisual, stop_step: Option<ColonyState>) {
        let mut roads = HashSet::new();

        for step in ColonyState::iter() {
            if stop_step.as_ref().map_or(false, |stop_step| step > *stop_step) { break; }
            let Some(step) = self.steps.get(&step) else { continue; };

            for (pos, structure) in &step.new_structures {
                draw_structure(visuals, pos, *structure);
            }

            roads.extend(step.new_roads.iter().cloned());
        }

        draw_roads(visuals, &roads);
    }

    pub fn draw_progression(&self, room: RoomName) {
        let plan = self.clone();

        let mut step = ColonyState::default();
        draw_in_room(room, move |visuals| {
            plan.draw_until(visuals, Some(step));
            step = step.get_promotion().unwrap_or_default()
        });
    }
}

pub fn draw_structure(visuals: &RoomVisual, pos: &RoomXY, structure: StructureType) {
    match structure {
        StructureType::Extension => {
            visuals.circle(pos.x.u8() as f32, pos.y.u8() as f32, Some(CircleStyle::default().radius(0.3).opacity(0.75).fill("#b05836")));
        },
        _ => {
            visuals.circle(pos.x.u8() as f32, pos.y.u8() as f32, Some(CircleStyle::default().radius(0.45).opacity(0.75).fill("#b05836")));
            visuals.text(pos.x.u8() as f32, pos.y.u8() as f32, structure.to_string(), Some(TextStyle::default().custom_font("0.35 Consolas").opacity(0.75).align(screeps::TextAlign::Center)));
        }
    }
}