use std::collections::HashSet;

use js_sys::Math::random;
use screeps::game;

use crate::creeps::CreepRole;

pub const FIRST_NAMES: &[&str] = &[
    "Alex",
    "Bailey",
    "Casey",
    "Dakota",
    "Emery",
    "Finley",
    "Grayson",
    "Harper",
    "Indigo",
    "Jamie",
    "Kai",
    "Lennon",
    "Marley",
    "Nolan",
    "Oakley",
    "Parker",
    "Quinn",
    "Reese",
    "Sage",
    "Tatum",
    "Uri",
    "Vesper",
    "Willow",
    "Xander",
    "Yara",
    "Zion",
];

pub const LAST_NAMES: &[&str] = &[
    "Archer",
    "Brooks",
    "Cross",
    "Dale",
    "Ellis",
    "Fields",
    "Grove",
    "Hill",
    "Iverson",
    "Jennings",
    "Knight",
    "Lane",
    "Marsh",
    "Nash",
    "Ortega",
    "Park",
    "Quade",
    "Rivers",
    "Stone",
    "Thornton",
    "Underwood",
    "Voss",
    "Wells",
    "Xu",
    "York",
    "Zimmerman",
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
