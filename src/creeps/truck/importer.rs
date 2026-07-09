use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::RoomName;
use serde::Deserialize;

use crate::{check::Check, creeps::truck::{state::TruckTask, stop::ConsumerTruckStop}, ids::{CheckState, Checked, Unchecked}};

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; TruckTask<S>, ConsumerTruckStop<S>)]
pub enum ImporterTruckState<S: CheckState = Checked> {
    #[default] Idle,
    CollectingFrom(RoomName),
    GoingHome,
    ProvidingTo(ConsumerTruckStop<S>),
    StoringAway
}

impl<'de> Deserialize<'de> for ImporterTruckState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ImporterTruckState::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            ImporterTruckState::Idle => Self::Idle,
            ImporterTruckState::CollectingFrom(room) => Self::CollectingFrom(room),
            ImporterTruckState::GoingHome => Self::GoingHome,
            ImporterTruckState::ProvidingTo(consumer) => 
                consumer.check().map_or(Self::StoringAway, Self::ProvidingTo),
            ImporterTruckState::StoringAway => Self::StoringAway,
        })
    }
}