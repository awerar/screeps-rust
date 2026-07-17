use std::collections::HashSet;

use js_sys::Math::random;
use screeps::game;

use crate::creeps::CreepRole;

pub const FIRST_NAMES: &[&str] = &[
    "Alex",
    "Aaron",
    "Bailey",
    "Blake",
    "Casey",
    "Cameron",
    "Dakota",
    "Devon",
    "Emery",
    "Emerson",
    "Elliot",
    "Finley",
    "Frankie",
    "Grayson",
    "Greer",
    "Harper",
    "Hayden",
    "Indigo",
    "Ira",
    "Jamie",
    "Jules",
    "Kai",
    "Kelsey",
    "Kiran",
    "Lennon",
    "Logan",
    "Marley",
    "Marlow",
    "Morgan",
    "Nolan",
    "Nova",
    "Oakley",
    "Ollie",
    "Parker",
    "Peyton",
    "Quinn",
    "Quincy",
    "Reese",
    "Riley",
    "Rowan",
    "Sage",
    "Soren",
    "Skyler",
    "Tatum",
    "Taylor",
    "Uri",
    "Vesper",
    "Willow",
    "Wren",
    "Xander",
    "Xavi",
    "Yara",
    "Yancy",
    "Zion",
    "Zeke",
    "Dorian",
    "Briar",
];

pub const LAST_NAMES: &[&str] = &[
    "Stone",
    "Brooks",
    "Rivers",
    "Wells",
    "Fields",
    "Woods",
    "Hill",
    "Dale",
    "Cross",
    "Park",
    "Lane",
    "Marsh",
    "Ford",
    "Grove",
    "Crane",
    "Finch",
    "Raven",
    "Hawk",
    "Wolf",
    "Fox",
    "Bear",
    "Swift",
    "Drake",
    "Coleman",
    "Bishop",
    "Mercer",
    "Hale",
    "Sutton",
    "Thornton",
    "Monroe",
    "Archer",
    "Barrett",
    "Clayton",
    "Ellis",
    "Foster",
    "Gardner",
    "Iverson",
    "Jennings",
    "Knight",
    "Lambert",
    "Nash",
    "Ortega",
    "Pierce",
    "Reed",
    "Sterling",
    "Thatcher",
    "Underwood",
    "Voss",
    "Whitman",
    "York",
    "Zimmerman",
    "Palmer",
    "Rowe",
    "Sykes",
    "Valenzuela",
    "Vale",
];

pub struct UsedNames(HashSet<String>);

impl UsedNames {
    pub fn new() -> Self {
        UsedNames(game::creeps().keys().collect())
    }

    pub fn generate_new(&mut self, role: &CreepRole) -> String {
        for _ in 0..20 {
            let first_name = FIRST_NAMES[(random() * FIRST_NAMES.len() as f64) as usize];
            let last_name = LAST_NAMES[(random() * LAST_NAMES.len() as f64) as usize];
            let name = format!("{} {first_name} {last_name}", role.prefix());
            
            if self.0.insert(name.clone()) { return name; }
        }

        panic!("Unable to find a free name for creep");
    }
}
