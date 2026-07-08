use std::{cell::RefCell, collections::HashSet, iter, mem};

use anyhow::anyhow;
use clap::Parser;
use log::info;
use screeps::{RoomName, StructureProperties, find, game};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{colony::planning::plan::ColonyPlan, visuals};

thread_local! {
    static COMMANDS: RefCell<HashSet<Command>> = RefCell::new(HashSet::new());
}


#[wasm_bindgen]
pub fn command(command: &str) {
    do_command(command).inspect_err(|err| info!("{err}")).ok();
}

fn do_command(command: &str) -> anyhow::Result<()> {
    let command = shlex::split(command).ok_or(anyhow!("Unable to lex command"))?;
    let tokens = iter::once("command".to_string()).chain(command);
    let command = Command::try_parse_from(tokens)?;

    match command {
        Command::ClearVisuals => visuals::clear_visuals(),
        Command::VisualizeNewPlan { room } => {
            let room = RoomName::new(&room).unwrap();
            ColonyPlan::create_for(&game::rooms().get(room).unwrap()).unwrap().draw_progression(room);
        },
        Command::CleanRoomStructures { room } => {
            game::rooms().get(RoomName::new(&room).unwrap())
                .unwrap()
                .find(find::STRUCTURES, None).into_iter()
                .for_each(|structure| { structure.destroy().ok(); });
        },
        Command::CleanRoomSites { room } => {
            game::rooms().get(RoomName::new(&room).unwrap())
                .unwrap()
                .find(find::MY_CONSTRUCTION_SITES, None).into_iter()
                .for_each(|site| { site.remove().ok(); });
        },
        _ => { COMMANDS.with_borrow_mut(|commands| commands.insert(command)); }
    }

    Ok(())
}

#[expect(clippy::needless_pass_by_value)]
pub fn pop_command(cmd: Command) -> bool {
    COMMANDS.with_borrow_mut(|commands| {
        let did_pop = commands.remove(&cmd);
        if did_pop { info!("Processing command {cmd:?}"); }
        did_pop
    })
}

pub fn handle_commands<F>(f: F) where F : FnMut(&Command) -> bool {
    COMMANDS.with_borrow_mut(|commands| {
        let (handled, unhandled): (Vec<_>, _) = mem::take(commands).into_iter().partition(f);
        *commands = unhandled.into_iter().collect();

        for cmd in &handled {
            info!("Processed command {cmd:?}");
        }
    });
}

#[derive(Parser, Debug, Hash, PartialEq, Eq, Clone)]
pub enum Command {
    ClearVisuals,
    VisualizeNewPlan { room: String },
    VisualizePlan { room: String, #[clap(long, short)] animate: bool },
    CleanRoomStructures { room: String },
    CleanRoomSites { room: String },
    ResetColonyStep { room: String },
    ResetColony { room: String },
    MigrateColony { room: String },
    DebugSpawn,
    VisualizeMovement { creep: String },
    Claim { room: String }
}