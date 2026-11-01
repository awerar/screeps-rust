use std::collections::{HashMap, HashSet};

use itertools::Itertools;
use screeps::{HasPosition, Position, Room, RoomXY, StructureType, find};
use anyhow::{anyhow, bail};
use strum::IntoEnumIterator;

use crate::{colony::{plan::{CenterPlan, ColonyPlan, ColonyPlanStep, MineralPlan, SourcePlan, SourcesPlan, refs::{PlannedStructureRef, PlannedStructureRefs}}, planner::state::{ColonyPlanner, PathId, PlannedContainer, PlannedImpassableStructure, entities::{PlannedEntity, PlannedStructure}}, steps::ColonyStep}, domain_traits::HasId};

pub struct IntermediateColonyPlan {
    pub room: Room,

    pub new_structures: HashMap<ColonyStep, HashSet<(RoomXY, StructureType)>>,
    pub expired_structures: HashMap<ColonyStep, HashSet<(RoomXY, StructureType)>>,

    pub entities2pos: HashMap<PlannedEntity, Vec<RoomXY>>,
    pub paths: HashMap<PathId, Vec<RoomXY>>
}

impl IntermediateColonyPlan {
    fn compile(self) -> anyhow::Result<ColonyPlan> {
        let mut plan_steps = HashMap::new();

        let mut accumulated_structures = HashSet::new();
        for step in ColonyStep::iter() {
            let new_structures = self.new_structures.get(&step).cloned().unwrap_or_default();
            let expired_structures = self.expired_structures.get(&step).cloned().unwrap_or_default();

            accumulated_structures.extend(new_structures.iter().copied());
            accumulated_structures = accumulated_structures.difference(&expired_structures).copied().collect();

            plan_steps.insert(step, ColonyPlanStep {
                new_structures,
                expired_structures,
                accumulated_structures: accumulated_structures.clone()
            });
        }

        let center = self.compile_center()?;

        Ok(ColonyPlan {
            steps: plan_steps,
            sources: self.compile_sources()?,
            mineral: self.compile_mineral()?,
            center,
            upgrader_container: self.get_structure_ref(PlannedContainer::Upgrader.into())?,
            controller: PlannedStructureRef::new(self.room.controller().ok_or(anyhow!("No controller"))?.pos())
        })
    }


    fn compile_center(&self) -> anyhow::Result<CenterPlan> {
        use PlannedImpassableStructure::*;
        use PlannedContainer::*;

        let storage_ref = self.get_structure_ref(Storage.into())?;

        Ok(CenterPlan {
            pos: storage_ref.pos,
            spawn: self.get_structure_ref(MainSpawn.into())?,
            storage: storage_ref.into(),
            container_storage: self.get_structure_ref(Container.into())?.into(),
            link: self.get_structure_ref(CentralLink.into())?.into(),
            terminal: self.get_structure_ref(Terminal.into())?.into(),
            observer: self.get_structure_ref(Observer.into())?.into(),
            towers: self.get_structure_refs(Tower.into()),
            extensions: self.get_structure_refs(Extension.into())
        })
    }

    fn compile_sources(&self) -> anyhow::Result<SourcesPlan> {
        use PlannedImpassableStructure::*;
        use PlannedContainer::*;

        self.room.find(find::SOURCES, None).into_iter()
            .map(|source| screeps::HasId::id(&source))
            .map(|source| {
                let container = self.get_structure_ref(Source(source).into())?;
                let spawn = self.get_structure_ref(SourceSpawn(source).into())?;

                let plan = SourcePlan {
                    spawn_direction: spawn.pos.get_direction_to(container.pos).unwrap(),
                    distance: self.get_path_length(&PathId::Source(source))?,
                    spawn: spawn.into(),
                    container: container.into(),
                    link: self.get_structure_ref(SourceLink(source).into())?.into(),
                    extensions: self.get_structure_refs(SourceExtension(source).into()),
                };

                Ok((source, plan))
            }).collect::<anyhow::Result<_>>()
    }

    fn compile_mineral(&self) -> anyhow::Result<MineralPlan> {
        use PlannedContainer::*;
        use PlannedImpassableStructure::*;

        let container = self.get_structure_ref(Mineral.into())?;

        Ok(MineralPlan {
            distance: self.get_path_length(&PathId::Mineral)?,
            container: container.into(),
            extractor: self.get_structure_ref(Extractor.into())?.into(),
        })
    }

    fn get_pos(&self, entity: PlannedEntity) -> anyhow::Result<Position> {
        self.entities2pos.get(&entity)
            .ok_or(anyhow!("No {entity:?} was found"))
            .and_then(|positions| {
                if positions.is_empty() { Err(anyhow!("No {entity:?} was found")) }
                else if positions.len() > 1 { Err(anyhow!("Unable to determine unique {entity:?}")) }
                else { Ok(*positions.iter().next().unwrap()) }
            }).map(|pos| Position::new(pos.x, pos.y, self.room.name()))
    }

    fn get_structure_ref<T: HasId>(&self, structure: PlannedStructure) -> anyhow::Result<PlannedStructureRef<T>> {
        self.get_pos(structure.into())
            .map(|pos| PlannedStructureRef::new(pos))
    }

    fn get_structure_refs<T: HasId>(&self, structure: PlannedStructure) -> PlannedStructureRefs<T> {
        let Some(positions) = self.entities2pos.get(&structure.into()) else { return PlannedStructureRefs(Vec::new()) };

        PlannedStructureRefs(positions.iter()
            .copied()
            .map(|pos| PlannedStructureRef::new(Position::new(pos.x, pos.y, self.room.name())))
            .collect())
    }

    fn get_path_length(&self, id: &PathId) -> anyhow::Result<u32> {
        Ok(self.paths.get(id).ok_or(anyhow!("No path found"))?.len() as u32)
    }
}

impl ColonyPlanner {
    fn compile_intermediate(self) -> IntermediateColonyPlan {
        let mut new_structures: HashMap<_, HashSet<_>> = HashMap::new();
        let mut expired_structures: HashMap<_, HashSet<_>> = HashMap::new();

        for (pos, timeline) in &self.tiles {
            for (lt, structure) in timeline.structures() {
                new_structures.entry(lt.build_step()).or_default().insert((*pos, structure));
                if let Some(destroy_step) = lt.destroy_step() {
                    expired_structures.entry(destroy_step).or_default().insert((*pos, structure));
                }
            }
        }

        let entities2pos = self.tiles.iter()
            .flat_map(|(xy, timeline)| {
                timeline.entities().map(|entity| (entity, *xy))
            }).into_group_map();

        let controller = self.room.controller().unwrap().pos().xy();
        new_structures.entry(ColonyStep::BuildSpawn).or_default().insert((controller, StructureType::Controller));

        IntermediateColonyPlan {
            room: self.room,
            new_structures,
            expired_structures,
            entities2pos,
            paths: self.paths
        }
    }

    pub fn compile(self) -> anyhow::Result<ColonyPlan> {
        if !self.reserved_blocks.is_empty() { bail!("Reserved blocks were never planned: {:?}", self.reserved_blocks) }

        self.compile_intermediate().compile()
    }
}
