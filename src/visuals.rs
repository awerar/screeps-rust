use std::{cell::RefCell, collections::HashMap};

use screeps::{RoomName, RoomVisual};

#[derive(Default)]
struct StaticDrawers {
    rooms: HashMap<RoomName, Vec<Box<dyn FnMut(&RoomVisual) -> ()>>>,
    global: Vec<Box<dyn FnMut(&RoomVisual) -> ()>>
}

thread_local! {
    static STATIC_DRAWERS: RefCell<StaticDrawers> = RefCell::new(Default::default());
}

pub fn draw_in_room(room: RoomName, f: impl FnMut(&RoomVisual) -> () + 'static) {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        static_drawers.rooms.entry(room).or_default().push(Box::new(f));
    })
}

#[expect(unused)]
pub fn draw_globally(f: impl FnMut(&RoomVisual) -> () + 'static) {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        static_drawers.global.push(Box::new(f));
    })
}

pub fn draw() {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        let mut global = RoomVisual::new(None);
        for drawer in &mut static_drawers.global {
            drawer(&mut global);
        }

        for (room, drawers) in &mut static_drawers.rooms {
            let mut room_visual = RoomVisual::new(Some(*room));
            for drawer in drawers {
                drawer(&mut room_visual);
            }
        }
    })
}

pub fn clear_visuals() {
    STATIC_DRAWERS.replace(Default::default());
}