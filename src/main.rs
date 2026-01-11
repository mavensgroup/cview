// src/main.rs

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{
  Application, ApplicationWindow, DrawingArea, Frame, Notebook, Orientation, ScrolledWindow,
  TextView,
};
use gtk4::{Revealer, RevealerTransitionType};
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

// Helper function to append text and scroll (matches actions_file.rs)
fn log_msg(view: &TextView, text: &str) {
  let buffer = view.buffer();
  let mut end = buffer.end_iter();
  buffer.insert(&mut end, &format!("{}\n", text));

  // Auto-scroll to the end
  let mark = buffer.create_mark(None, &buffer.end_iter(), false);
  view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
  buffer.delete_mark(&mark);
}

fn main() {
  let app = Application::builder()
    .application_id("com.example.cview")
    .build();

  app.connect_activate(build_ui);

  // Fix for CLI args being interpreted by GTK
  app.run_with_args(&Vec::<String>::new());
}

fn build_ui(app: &Application) {
  // 1. Initialize State & Capture Startup Log
  let (mut initial_state, startup_log) = AppState::new_with_log();

  // CLI ARGUMENT PARSING
  let args: Vec<String> = std::env::args().collect();
  if args.len() > 1 {
    let path = &args[1];
    println!("CLI: Attempting to open '{}'", path);
    if let Ok(structure) = io::load_structure(path) {
      initial_state.structure = Some(structure);
      initial_state.file_name = std::path::Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    }
  }

  let state = Rc::new(RefCell::new(initial_state));

  let window = ApplicationWindow::builder()
    .application(app)
    .title("CView - Crystal Structure Viewer")
    .default_width(1200)
    .default_height(800)
    .build();

  // 1. TOP LEVEL
  let root_vbox = GtkBox::new(Orientation::Vertical, 0);
  window.set_child(Some(&root_vbox));

  // 2. MAIN CONTENT
  let main_hbox = GtkBox::new(Orientation::Horizontal, 0);

  // Right Panel
  let right_vbox = GtkBox::new(Orientation::Vertical, 0);
  right_vbox.set_hexpand(true);

  let drawing_area = DrawingArea::new();
  drawing_area.set_vexpand(true);

  // --- CONSOLE NOTEBOOK SETUP ---
  let console_notebook = Notebook::new();
  console_notebook.set_height_request(200);

  // Tab 1: Interactions View (Geometry, Selection)
  let interactions_view = TextView::builder()
    .editable(false)
    .cursor_visible(false)
    .monospace(true)
    .left_margin(10)
    .right_margin(10)
    .top_margin(10)
    .bottom_margin(10)
    .build();

  // --- POPULATE TAB 1 (CLI LOAD) ---
  {
    let st = state.borrow();
    if st.structure.is_some() {
      // Use the standard report method from state.rs
      let report = st.get_structure_report();
      log_msg(&interactions_view, &report);
    }
  }

  let scroll_interactions = ScrolledWindow::builder().child(&interactions_view).build();

  console_notebook.append_page(
    &scroll_interactions,
    Some(&gtk4::Label::new(Some("Interactions"))),
  );

  // Tab 2: System Logs View (Loading, Errors)
  let system_log_view = TextView::builder()
    .editable(false)
    .cursor_visible(false)
    .monospace(true)
    .left_margin(10)
    .right_margin(10)
    .top_margin(10)
    .bottom_margin(10)
    .build();

  // Write startup log
  log_msg(&system_log_view, &startup_log);

  // Confirm CLI Load in System Log
  {
    let st = state.borrow();
    if st.structure.is_some() {
      log_msg(
        &system_log_view,
        &format!("File loaded successfully via CLI: {}", st.file_name),
      );
    }
  }

  let scroll_logs = ScrolledWindow::builder().child(&system_log_view).build();

  console_notebook.append_page(&scroll_logs, Some(&gtk4::Label::new(Some("System Logs"))));

  let info_frame = Frame::new(None);
  info_frame.set_child(Some(&console_notebook));
  // ------------------------------

  right_vbox.append(&drawing_area);
  right_vbox.append(&info_frame);

  // Left Panel (Sidebar)
  use panels::sidebar;
  let (sidebar_widget, atom_list_box) = sidebar::build(state.clone(), &drawing_area);

  let sidebar_revealer = Revealer::builder()
    .transition_type(RevealerTransitionType::SlideRight)
    .child(&sidebar_widget)
    .reveal_child(true)
    .build();

  main_hbox.append(&sidebar_revealer);
  main_hbox.append(&right_vbox);

  // 3. Menu Bar
  // Pass both views here
  let menu_bar = menu::build_menu_and_actions(
    app,
    &window,
    state.clone(),
    &drawing_area,
    &system_log_view,
    &interactions_view,
    &atom_list_box,
  );

  let toggle_action = gtk4::gio::SimpleAction::new("toggle_sidebar", None);
  let rev_weak = sidebar_revealer.downgrade();
  toggle_action.connect_activate(move |_, _| {
    if let Some(rev) = rev_weak.upgrade() {
      rev.set_reveal_child(!rev.reveals_child());
    }
  });
  app.add_action(&toggle_action);
  app.set_accels_for_action("app.toggle_sidebar", &["F9"]);

  // Quit
  let quit_action = gtk4::gio::SimpleAction::new("quit", None);
  let win_weak_q = window.downgrade();
  let state_quit = state.clone();
  quit_action.connect_activate(move |_, _| {
    let msg = state_quit.borrow().save_config();
    println!("{}", msg);
    if let Some(win) = win_weak_q.upgrade() {
      win.close();
    }
  });
  app.add_action(&quit_action);

  root_vbox.append(&menu_bar);
  root_vbox.append(&main_hbox);

  // Interactions setup uses the interactions_view
  setup_interactions(&window, state.clone(), &drawing_area, &interactions_view);

  // Drawing Loop
  let s = state.clone();
  drawing_area.set_draw_func(move |_, cr, w, h| {
    let st = s.borrow();
    let (bg_r, bg_g, bg_b) = st.style.background_color;
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.paint().unwrap();

    let (atoms, lattice_corners, bounds) =
      rendering::scene::calculate_scene(&st, w as f64, h as f64, false, None, None);

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
}
