use derive_deref::Deref;
use screeps::{Creep, MaybeHasId, ObjectId};
use serde::{Deserialize, Serialize};

pub trait Entity: MaybeHasId {}
impl Entity for Creep {}

pub trait IDMode {
    type Wrap<T: Entity>;
}

pub struct Unresolved;
impl IDMode for Unresolved {
    type Wrap<T: Entity> = ObjectId<T>;
}

pub struct Resolved;
impl IDMode for Resolved {
    type Wrap<T: Entity> = ResolvedId<T>;
}

#[derive(Deref)]
pub struct ResolvedId<T>(pub T);

impl<T: MaybeHasId> Serialize for ResolvedId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        self.0.try_id().unwrap().serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct TestMemory<M: IDMode> {
    creep: M::Wrap<Creep>,
    x: u32
}

impl TestMemory<Unresolved> {
    fn resolve(self) -> TestMemory<Resolved> {
        TestMemory::<Resolved> { 
            creep: self.creep.clone().resolve().map(ResolvedId).unwrap(),
            x: self.x
        }
    }
}

fn test() {
    let s: String = "".to_string();

    let unresolved_mem: TestMemory<Unresolved> = serde_json::from_str(&s).unwrap();
    let mem = unresolved_mem.resolve();
    mem.creep.move_direction(screeps::Direction::Bottom);

    let s = serde_json::to_string(&mem).unwrap();
}