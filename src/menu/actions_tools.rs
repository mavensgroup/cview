// use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use super::{tool_supercell, tool_miller};

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    // Delegate the actual work to dedicated modules
    tool_supercell::setup(app, window, state.clone(), drawing_area);
    tool_miller::setup(app, window, state, drawing_area);
}
