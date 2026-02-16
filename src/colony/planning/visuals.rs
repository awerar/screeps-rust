use std::collections::HashSet;

use itertools::Itertools;
use screeps::{CircleStyle, LineStyle, RoomName, RoomVisual, RoomXY, StructureType, TextAlign, TextStyle};

use crate::{colony::{planning::plan::{ColonyPlan, ColonyPlanDiff}, steps::{ColonyStep, ColonyStepStateMachine}}, visuals::{RoomDrawerType, draw_in_room_replaced}};

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
    pub fn draw_until(&self, visuals: &RoomVisual, stop_step: Option<ColonyStep>) {
        let mut roads = HashSet::new();

        for step in ColonyStep::iter() {
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

        let mut step = ColonyStep::default();
        draw_in_room_replaced(room, RoomDrawerType::Plan, move |visuals| {
            plan.draw_until(visuals, Some(step));
            step = step.get_promotion().unwrap_or_default()
        });
    }
}

impl ColonyPlanDiff {
    const CROSS_RADIUS: f32 = 0.35;
    pub fn draw(&self, room: RoomName) {
        let losses = self.get_removal_losses();

        draw_in_room_replaced(room, RoomDrawerType::Diff, move |visuals| {
            for (pos, loss) in &losses {
                let a1 = (pos.x.u8() as f32 + Self::CROSS_RADIUS, pos.y.u8() as f32 + Self::CROSS_RADIUS);
                let a2 = (pos.x.u8() as f32 - Self::CROSS_RADIUS, pos.y.u8() as f32 - Self::CROSS_RADIUS);

                let b1 = (pos.x.u8() as f32 + Self::CROSS_RADIUS, pos.y.u8() as f32 - Self::CROSS_RADIUS);
                let b2 = (pos.x.u8() as f32 - Self::CROSS_RADIUS, pos.y.u8() as f32 + Self::CROSS_RADIUS);

                let cross_style = LineStyle::default().color("#ff4747").width(0.1);

                visuals.line(a1, a2, Some(cross_style.clone()));
                visuals.line(b1, b2, Some(cross_style));

                let mut text_style = TextStyle::default().color("#ff4747").background_color("#ffffff").background_padding(0.1).align(TextAlign::Center).custom_font("0.3 Consolas");
                if *loss > StructureType::Road.construction_cost().unwrap() {
                    text_style = text_style.custom_font("0.5 Consolas");
                };

                let label = if *loss < 1000 { loss.to_string() } else if *loss < 1000000 { format!("{}k", loss / 1000) } else { format!("{}M", loss / 1000000) };
                visuals.text(pos.x.u8() as f32, pos.y.u8() as f32 + 0.3, label, Some(text_style));
            }
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