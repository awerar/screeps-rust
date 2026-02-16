use std::{cell::RefCell, collections::HashSet, iter};

use clap::Parser;
use log::*;
use screeps::{RoomName, StructureProperties, find, game};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{colony::planning::plan::ColonyPlan, visuals};

thread_local! {
    static COMMANDS: RefCell<HashSet<Command>> = RefCell::new(HashSet::new());
}


#[wasm_bindgen]
pub fn command(command: String) {
    do_command(command).inspect_err(|err| info!("{err}")).ok();
}

fn do_command(command: String) -> Result<(), String> {
    let command = shlex::split(&command).ok_or("Unable to lex command")?;
    let tokens = iter::once("command".to_string()).chain(command.into_iter());
    let command = Command::try_parse_from(tokens).map_err(|e| e.to_string())?;

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

pub fn pop_command(cmd: Command) -> bool {
    COMMANDS.with_borrow_mut(|commands| {
        let did_pop = commands.remove(&cmd);
        if did_pop { info!("Processing command {cmd:?}"); }
        did_pop
    })
}

pub fn handle_commands<F, R>(f: F) -> usize where F : Fn(&Command) -> bool {
    COMMANDS.with_borrow_mut(|commands| {
        let mut handled = Vec::new();

        for cmd in commands.iter() {
            if f(cmd) {
                handled.push(cmd.clone());
            }
        }

        for cmd in &handled {
            commands.remove(cmd);
            info!("Processing command {cmd:?}");
        }

        handled.len()
    })
}

#[derive(Parser, Debug, Hash, PartialEq, Eq, Clone)]
pub enum Command {
    ClearVisuals,
    VisualizeNewPlan { room: String },
    VisualizePlan { room: String },
    CleanRoomStructures { room: String },
    CleanRoomSites { room: String },
    ResetColonyStep { room: String },
    MigrateRoom { room: String }
}