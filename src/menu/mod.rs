// Declare the submodules
mod actions;
mod layout;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, PopoverMenuBar};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;

pub fn build_menu_and_actions(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) -> PopoverMenuBar {

    // 1. Setup Logic (Shortcuts & Actions)
    actions::setup_actions(app, window, state, drawing_area);

    // 2. Build UI Layout
    let menu_model = layout::build_menu_model();

    // 3. Return the Widget
    PopoverMenuBar::from_model(Some(&menu_model))
}
