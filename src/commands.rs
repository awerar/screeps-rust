use std::{cell::RefCell, collections::HashSet, iter};

use clap::Parser;
use log::*;
use screeps::{RoomName, game};
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
        Command::VisualizePlan { room } => {
            ColonyPlan::create_for(game::rooms().get(RoomName::new(&room).unwrap()).unwrap()).unwrap();
        }
    }

    Ok(())
}

pub fn pop_command(cmd: Command) -> bool {
    COMMANDS.with_borrow_mut(|commands| {
        commands.remove(&cmd)
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
        }

        handled.len()
    })
}

#[derive(Parser, Debug, Hash, PartialEq, Eq, Clone)]
pub enum Command {
    ClearVisuals,
    VisualizePlan {
        room: String
    }
}