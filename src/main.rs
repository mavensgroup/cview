use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box, DrawingArea, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

// Change these to 'pub mod' so sub-folders can access them via 'crate::...'
pub mod state;
pub mod rendering;
pub mod structure;
pub mod io;
pub mod elements;
pub mod symmetry;
pub mod config;
pub mod interactions;
pub mod preferences;
pub mod menu;

use state::AppState;
use rendering::setup_drawing;
use interactions::setup_interactions;

fn main() {
    let app = Application::builder()
        .application_id("com.example.cview")
        .build();

    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    // 1. Init State
    let state = Rc::new(RefCell::new(AppState::new()));

    // 2. Main Window
    let window = ApplicationWindow::builder()
        .application(app)
        .title("CView - Crystal Structure Viewer")
        .default_width(1024)
        .default_height(768)
        .build();

    let vbox = Box::new(Orientation::Vertical, 0);
    window.set_child(Some(&vbox));

    // 3. Create Drawing Area
    let drawing_area = DrawingArea::new();
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);

    // 4. Build Menu & Actions (Refactored)
    let menu_bar = menu::build_menu_and_actions(app, &window, state.clone(), &drawing_area);
    vbox.append(&menu_bar);

    // 5. Setup Rendering & Interaction Logic
    setup_drawing(&drawing_area, state.clone());
    setup_interactions(&window, state.clone(), &drawing_area);

    // 6. Finish Layout
    vbox.append(&drawing_area);

    window.present();
}
