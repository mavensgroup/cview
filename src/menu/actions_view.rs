use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, gio};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::{AppState, RotationCenter};

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    let da_clone = drawing_area.clone();
    let queue_draw = move || da_clone.queue_draw();

    // Reset
    let action_reset = gio::SimpleAction::new("reset_view", None);
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_reset.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.rot_x = 0.0; st.rot_y = 0.0; st.zoom = 1.0;
        q_c();
    });
    window.add_action(&action_reset);
    app.set_accels_for_action("win.reset_view", &["r", "R"]);

    // Toggle Center
    let initial_state = matches!(state.borrow().rotation_mode, RotationCenter::UnitCell);
    let action_center = gio::SimpleAction::new_stateful("toggle_center", None, &initial_state.to_variant());
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_center.connect_activate(move |action, _| {
        let current_state: bool = action.state().unwrap().get().unwrap();
        let new_state = !current_state;
        action.set_state(&new_state.to_variant());
        let mut st = state_c.borrow_mut();
        st.rotation_mode = if new_state { RotationCenter::UnitCell } else { RotationCenter::Centroid };
        st.save_config();
        q_c();
    });
    window.add_action(&action_center);
    app.set_accels_for_action("win.toggle_center", &["c", "C"]);

    // Aligns
    let action_view_x = gio::SimpleAction::new("view_x", None);
    let state_c = state.clone(); let q_c = queue_draw.clone();
    action_view_x.connect_activate(move |_, _| { 
        state_c.borrow_mut().rot_x = 0.0; 
        state_c.borrow_mut().rot_y = std::f64::consts::PI / 2.0; 
        q_c(); 
    });
    window.add_action(&action_view_x);

    let action_view_y = gio::SimpleAction::new("view_y", None);
    let state_c = state.clone(); let q_c = queue_draw.clone();
    action_view_y.connect_activate(move |_, _| { 
        state_c.borrow_mut().rot_x = std::f64::consts::PI / 2.0; 
        state_c.borrow_mut().rot_y = 0.0; 
        q_c(); 
    });
    window.add_action(&action_view_y);

    let action_view_z = gio::SimpleAction::new("view_z", None);
    let state_c = state.clone(); let q_c = queue_draw.clone();
    action_view_z.connect_activate(move |_, _| { 
        state_c.borrow_mut().rot_x = 0.0; 
        state_c.borrow_mut().rot_y = 0.0; 
        q_c(); 
    });
    window.add_action(&action_view_z);
}
