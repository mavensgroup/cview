// src/main.rs

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{
    Application, ApplicationWindow, DrawingArea, Frame, Label, Notebook, Orientation, Revealer,
    RevealerTransitionType, ScrolledWindow, TextView,
};
use std::cell::RefCell;
use std::rc::Rc;

// Declare modules
pub mod config;
pub mod io;
pub mod menu;
pub mod model;
pub mod panels;
pub mod physics;
pub mod rendering;
pub mod state;
pub mod ui;
pub mod utils;

use state::AppState;
use ui::interactions::setup_interactions;

// Helper function to append text and scroll
fn log_msg(view: &TextView, text: &str) {
    let buffer = view.buffer();
    let mut end = buffer.end_iter();
    buffer.insert(&mut end, &format!("{}\n", text));

    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
    view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}

fn main() {
    let app = Application::builder()
        .application_id("com.example.cview")
        .build();

    app.connect_activate(build_ui);

    // Run with empty args to prevent GTK from swallowing the filename argument,
    // allowing us to parse std::env::args() manually in build_ui.
    app.run_with_args(&Vec::<String>::new());
}

fn build_ui(app: &Application) {
    // 1. Initialize State
    let (initial_state, _startup_log) = AppState::new_with_log();
    let state = Rc::new(RefCell::new(initial_state));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("CView - Crystal Structure Viewer")
        .default_width(1200)
        .default_height(800)
        .build();

    // --- LAYOUT ---
    let root_vbox = GtkBox::new(Orientation::Vertical, 0);
    window.set_child(Some(&root_vbox));

    let main_hbox = GtkBox::new(Orientation::Horizontal, 0);

    // Right Panel
    let right_vbox = GtkBox::new(Orientation::Vertical, 0);
    right_vbox.set_hexpand(true);

    let drawing_area = DrawingArea::new();
    drawing_area.set_vexpand(true);

    // Console Notebook
    let console_notebook = Notebook::new();
    console_notebook.set_height_request(200);

    // Tab 1: Interactions
    let interactions_view = TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .left_margin(10)
        .right_margin(10)
        .top_margin(10)
        .bottom_margin(10)
        .build();
    let scroll_interactions = ScrolledWindow::builder().child(&interactions_view).build();
    console_notebook.append_page(
        &scroll_interactions,
        Some(&Label::new(Some("Interactions"))),
    );

    // Tab 2: System Logs
    let system_log_view = TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();

    // --- Hook up the Logger ---
    // This connects the standard 'log' crate to this specific TextView.
    if let Err(e) = utils::logger::init(&system_log_view) {
        eprintln!("Failed to initialize logger: {}", e);
    }

    log::info!("System started ready.");

    let scroll_logs = ScrolledWindow::builder().child(&system_log_view).build();
    console_notebook.append_page(&scroll_logs, Some(&Label::new(Some("System Logs"))));

    let info_frame = Frame::new(None);
    info_frame.set_child(Some(&console_notebook));

    right_vbox.append(&drawing_area);
    right_vbox.append(&info_frame);

    // Left Panel (Sidebar)
    use panels::sidebar;
    // We capture atom_list_box here so we can refresh it later
    let (sidebar_widget, atom_list_box) = sidebar::build(state.clone(), &drawing_area);

    let sidebar_revealer = Revealer::builder()
        .transition_type(RevealerTransitionType::SlideRight)
        .child(&sidebar_widget)
        .reveal_child(true)
        .build();

    main_hbox.append(&sidebar_revealer);
    main_hbox.append(&right_vbox);

    // Menu Bar
    let menu_bar = menu::build_menu_and_actions(
        app,
        &window,
        state.clone(),
        &drawing_area,
        &system_log_view,
        &interactions_view,
        &atom_list_box,
    );

    // Actions
    let toggle_action = gtk4::gio::SimpleAction::new("toggle_sidebar", None);
    let rev_weak = sidebar_revealer.downgrade();
    toggle_action.connect_activate(move |_, _| {
        if let Some(rev) = rev_weak.upgrade() {
            rev.set_reveal_child(!rev.reveals_child());
        }
    });
    app.add_action(&toggle_action);
    app.set_accels_for_action("app.toggle_sidebar", &["F9"]);

    let quit_action = gtk4::gio::SimpleAction::new("quit", None);
    let win_weak_q = window.downgrade();
    let state_quit = state.clone();
    quit_action.connect_activate(move |_, _| {
        let msg = state_quit.borrow().save_config();
        log::info!("{}", msg);
        if let Some(win) = win_weak_q.upgrade() {
            win.close();
        }
    });
    app.add_action(&quit_action);

    root_vbox.append(&menu_bar);
    root_vbox.append(&main_hbox);

    setup_interactions(&window, state.clone(), &drawing_area, &interactions_view);

    // Drawing Function
    let s = state.clone();
    drawing_area.set_draw_func(move |_, cr, w, h| {
        let st = s.borrow();

        // 1. Background
        let (bg_r, bg_g, bg_b) = st.config.style.background_color;
        cr.set_source_rgb(bg_r, bg_g, bg_b);
        cr.paint().unwrap();

        // 2. Calculate Scene
        // Passes the entire state '&st'
        let (atoms, lattice_corners, bounds) =
            rendering::scene::calculate_scene(&st, w as f64, h as f64, false, None, None);

        // 3. Draw Structure
        rendering::painter::draw_unit_cell(cr, &lattice_corners, false);

        rendering::painter::draw_structure(cr, &atoms, &st, bounds.scale, false);

        rendering::painter::draw_miller_planes(
            cr,
            &st,
            &lattice_corners,
            bounds.scale,
            w as f64,
            h as f64,
        );

        rendering::painter::draw_axes(cr, &st, w as f64, h as f64);

        rendering::painter::draw_selection_box(cr, &st);
    });

    window.present();

    // --- CLI LATE LOAD ---
    // Allows opening a file by passing it as an argument: ./cview structure.pdb
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let path = &args[1];
        log::info!("CLI: Attempting to open '{}'", path);

        match io::load_structure(path) {
            Ok(structure) => {
                // 1. Update State
                {
                    let mut st = state.borrow_mut();

                    st.original_structure = Some(structure.clone());
                    st.structure = Some(structure);

                    st.file_name = std::path::Path::new(path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                }

                // 2. Log Success
                log_msg(&system_log_view, &format!("File loaded via CLI: {}", path));

                // 3. New Report Logic (Accessing state via borrow)
                let st = state.borrow();
                if let Some(s) = &st.structure {
                    let report = utils::report::structure_summary(s, &st.file_name);
                    log_msg(&interactions_view, &report);
                }
                drop(st); // Crucial: drop borrow before passing state to panels::sidebar

                // 4. Refresh Sidebar
                panels::sidebar::refresh_atom_list(&atom_list_box, state.clone(), &drawing_area);

                // 5. Trigger Draw
                drawing_area.queue_draw();
            }
            Err(e) => {
                log_msg(&system_log_view, &format!("Error loading CLI file: {}", e));
            }
        }
    }
}
