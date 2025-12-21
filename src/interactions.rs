use gtk4::{self as gtk, prelude::*};
use gtk4::{ApplicationWindow, EventControllerScroll, EventControllerScrollFlags, GestureDrag};
use gtk4::glib;
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;

pub fn setup_interactions(
    _window: &ApplicationWindow, // Kept for API consistency, but unused here now
    state: Rc<RefCell<AppState>>,
    drawing_area: &gtk::DrawingArea,
) {
    // 1. Mouse Drag (Rotation)
    let drag = GestureDrag::new();
    let s = state.clone();
    let da = drawing_area.clone();

    drag.connect_drag_update(move |_, x, y| {
        let mut st = s.borrow_mut();
        let sensitivity = 0.01;
        st.rot_y += x * sensitivity;
        st.rot_x += y * sensitivity;
        da.queue_draw();
    });
    drawing_area.add_controller(drag);

    // 2. Scroll (Zoom)
    let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
    let s = state.clone();
    let da = drawing_area.clone();

    scroll.connect_scroll(move |_, _, dy| {
        let mut st = s.borrow_mut();
        let zoom_speed = 0.1;
        if dy > 0.0 {
            st.zoom *= 1.0 - zoom_speed;
        } else {
            st.zoom *= 1.0 + zoom_speed;
        }
        da.queue_draw();
        glib::Propagation::Stop
    });
    drawing_area.add_controller(scroll);
}
