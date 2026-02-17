use std::{cell::RefCell, collections::HashMap};

use screeps::{RoomName, RoomVisual};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub enum RoomDrawerType {
    Plan,
    Diff
}

pub type Drawer = Box<dyn FnMut(&RoomVisual)>;

#[derive(Default)]
struct StaticDrawers {
    rooms: HashMap<(RoomName, RoomDrawerType), Vec<Drawer>>,
    global: Vec<Drawer>
}

thread_local! {
    static STATIC_DRAWERS: RefCell<StaticDrawers> = RefCell::new(Default::default());
}

pub fn draw_in_room(room: RoomName, ty: RoomDrawerType, f: impl FnMut(&RoomVisual) + 'static) {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        static_drawers.rooms.entry((room, ty)).or_default().push(Box::new(f));
    });
}

pub fn clear_room_visual(room: RoomName, ty: RoomDrawerType) {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        static_drawers.rooms.remove(&(room, ty));
    });
}

pub fn draw_in_room_replaced(room: RoomName, ty: RoomDrawerType, f: impl FnMut(&RoomVisual) + 'static) {
    clear_room_visual(room, ty);
    draw_in_room(room, ty, f);
}

#[expect(unused)]
pub fn draw_globally(f: impl FnMut(&RoomVisual) + 'static) {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        static_drawers.global.push(Box::new(f));
    });
}

pub fn draw() {
    STATIC_DRAWERS.with_borrow_mut(|static_drawers| {
        let mut global = RoomVisual::new(None);
        for drawer in &mut static_drawers.global {
            drawer(&mut global);
        }

        for ((room, _), drawers) in &mut static_drawers.rooms {
            let mut room_visual = RoomVisual::new(Some(*room));
            for drawer in drawers {
                drawer(&mut room_visual);
            }
        }
    });
}

pub fn clear_visuals() {
    STATIC_DRAWERS.replace(Default::default());
}