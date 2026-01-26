use js_sys::Math::random;
use screeps::game;

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
    "Jordan",
    "Kai",
    "Logan",
    "Morgan",
    "Nolan",
    "Ollie",
    "Parker",
    "Quinn",
    "Riley",
    "Sage",
    "Taylor",
    "Urchin",
    "Vale",
    "Wren",
    "Xander",
    "Yancy",
    "Zeke",
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
    "Vale",
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
    "Blake",
    "Stone",
];

pub fn get_new_creep_name() -> String {
    for _ in 0..20 {
        let first_name = FIRST_NAMES[(random() * FIRST_NAMES.len() as f64) as usize];
        let last_name = LAST_NAMES[(random() * LAST_NAMES.len() as f64) as usize];
        let name = format!("{} {}", first_name, last_name);
        
        if game::creeps().get(name.clone()).is_none() { return name; }
    }

    panic!("Unable to find a free name for creep");
}
