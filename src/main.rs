use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, Orientation, Frame, ScrolledWindow, TextView};
use gtk4::Box as GtkBox;
use std::cell::RefCell;
use std::rc::Rc;
use gtk4::{Revealer, RevealerTransitionType};

pub mod state;
pub mod rendering;
pub mod io;
pub mod menu;
pub mod config;
pub mod ui;
pub mod model;
pub mod utils;
pub mod panels;
pub mod physics;

use state::AppState;
use ui::interactions::setup_interactions;

fn main() {
    let app = Application::builder()
        .application_id("com.example.cview")
        .build();

    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    let mut initial_state = AppState::new();
    initial_state.load_config();
    let state = Rc::new(RefCell::new(initial_state));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("CView - Crystal Structure Viewer")
        .default_width(1200) // Increase width for sidebar
        .default_height(800)
        .build();

    // 1. TOP LEVEL: Vertical Box (Menu on top, Main Content below)
    let root_vbox = GtkBox::new(Orientation::Vertical, 0);
    window.set_child(Some(&root_vbox));

    // 2. MAIN CONTENT: Horizontal Box (Sidebar | Right_Panel)
    let main_hbox = GtkBox::new(Orientation::Horizontal, 0);

    // --- Right Panel (Drawing + Console) ---
    let right_vbox = GtkBox::new(Orientation::Vertical, 0);
    right_vbox.set_hexpand(true); // Allow this to take remaining width

    let drawing_area = DrawingArea::new();
    drawing_area.set_vexpand(true); // Take available height

    // Console
    let info_frame = Frame::new(None);
    let console_view = TextView::builder()
        .editable(false).cursor_visible(false).monospace(true)
        .left_margin(10).right_margin(10).top_margin(10).bottom_margin(10)
        .build();
    let scroll_win = ScrolledWindow::builder()
        .min_content_height(150)
        .child(&console_view)
        .build();
    info_frame.set_child(Some(&scroll_win));

    right_vbox.append(&drawing_area);
    right_vbox.append(&info_frame);

    // --- Left Panel (Sidebar) ---
    use panels::sidebar;
    let (sidebar_widget, atom_list_box) = sidebar::build(state.clone(), &drawing_area);
    // let sidebar_widget = sidebar::build(state.clone(), &drawing_area);

    // Wrap sidebar in Revealer for animation
    let sidebar_revealer = Revealer::builder()
        .transition_type(RevealerTransitionType::SlideRight)
        .child(&sidebar_widget)
        .reveal_child(true) // Open by default
        .build();

    main_hbox.append(&sidebar_revealer);
    main_hbox.append(&right_vbox);

    // 3. Menu Bar
    // We create the menu, which also needs access to widgets for actions
    let menu_bar = menu::build_menu_and_actions(app, &window, state.clone(), &drawing_area, &console_view, &atom_list_box);

    // 4. ACTION: Toggle Sidebar
    // Press 'T' or F9 to toggle
    let toggle_action = gtk4::gio::SimpleAction::new("toggle_sidebar", None);
    let rev_weak = sidebar_revealer.downgrade();
    toggle_action.connect_activate(move |_, _| {
        if let Some(rev) = rev_weak.upgrade() {
            rev.set_reveal_child(!rev.reveals_child());
        }
    });
    app.add_action(&toggle_action);
    app.set_accels_for_action("app.toggle_sidebar", &["F9"]);

    // Assemble Root
    root_vbox.append(&menu_bar);
    root_vbox.append(&main_hbox);

    // --- Setup Logic ---
    setup_interactions(&window, state.clone(), &drawing_area, &console_view);

    // Drawing Function
    let s = state.clone();
    drawing_area.set_draw_func(move |_, cr, w, h| {
        let st = s.borrow();

        // 1. Background
        let (bg_r, bg_g, bg_b) = st.style.background_color;
        cr.set_source_rgb(bg_r, bg_g, bg_b);
        cr.paint().unwrap();

        // 2. Calculate Scene (Atoms, Lattice, Bounds)
        let (atoms, lattice_corners, bounds) = rendering::scene::calculate_scene(
            &st,
            w as f64,
            h as f64,
            false,
            None,
            None
        );

        // 3. Draw Unit Cell (Box)
        rendering::painter::draw_unit_cell(cr, &lattice_corners, false);

        // 4. Draw Atoms & Bonds
        rendering::painter::draw_structure(cr, &atoms, &st, bounds.scale, false);

        // --- NEW: Draw Miller Planes ---
        // We pass 'bounds.scale' so it matches the zoom level of the atoms
       rendering::painter::draw_miller_planes(
    cr,
    &st,
    &lattice_corners, // This passes a slice &[ [f64;3] ]
    bounds.scale,
    w as f64,
    h as f64
);
        //
        // -------------------------------

        // 5. Draw Axes (Last, so they appear on top)
        rendering::painter::draw_axes(cr, &st, w as f64, h as f64);
    });

    window.present();
}
    // Drawing Function (Existing logic)
    // let s = state.clone();
    // drawing_area.set_draw_func(move |_, cr, w, h| {
        // let st = s.borrow();

        // let (bg_r, bg_g, bg_b) = st.style.background_color;
        // cr.set_source_rgb(bg_r, bg_g, bg_b);
        // cr.paint().unwrap();
        // let (atoms, lattice_corners, bounds) = rendering::scene::calculate_scene(&st, w as f64, h as f64, false, None, None);
        // rendering::painter::draw_unit_cell(cr, &lattice_corners, false);
        // rendering::painter::draw_structure(cr, &atoms, &st, bounds.scale, false);
        // rendering::painter::draw_axes(cr, &st, w as f64, h as f64);
    // });

    // window.present();
// }
