use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, gio};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::ui::geometry::GeometryWindow;

pub fn setup(
    _app: &Application, // Unused prefix
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    _drawing_area: &DrawingArea, // Unused
) {
    let action_geometry = gio::SimpleAction::new("geometry", None);

    // We need the Application to create a new window, but we can get it from the window passed in
    let app_opt = window.application();

    let state_clone = state.clone();
    action_geometry.connect_activate(move |_, _| {
        if let Some(app) = &app_opt {
            let geom_win = GeometryWindow::new(app);
            let s = state_clone.borrow();
            geom_win.update(&s);
            geom_win.window.present();
        }
    });
    window.add_action(&action_geometry);
}
