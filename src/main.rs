use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{
  Application, ApplicationWindow, DrawingArea, Frame, Orientation, ScrolledWindow, TextView,
};
use gtk4::{Revealer, RevealerTransitionType};
use std::cell::RefCell;
use std::rc::Rc;

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

fn main() {
  let app = Application::builder()
    .application_id("com.example.cview")
    .build();

  app.connect_activate(build_ui);

  // FIX: Pass empty arguments to GTK so it doesn't try to interpret 'POSCAR' as a GTK-file-open request.
  // We handle the arguments manually inside build_ui via std::env::args().
  app.run_with_args(&Vec::<String>::new());
}

fn build_ui(app: &Application) {
  let mut initial_state = AppState::new();
  initial_state.load_config();

  let state = Rc::new(RefCell::new(initial_state));

  // ============================================================
  // CLI ARGUMENT PARSING (Manual)
  // ============================================================
  // This still works because we read from the OS environment directly
  let args: Vec<String> = std::env::args().collect();
  if args.len() > 1 {
    let path = &args[1];
    println!("CLI: Attempting to open '{}'", path);

    let result = if path.to_lowercase().ends_with(".cif") {
      io::cif::parse(path)
    } else if path.to_uppercase().contains("POSCAR")
      || path.to_uppercase().contains("CONTCAR")
      || path.to_lowercase().contains(".vasp")
    {
      io::poscar::parse(path)
    } else if path.to_lowercase().ends_with(".pwo") || path.to_lowercase().ends_with(".in") {
      io::qe::parse(path)
    } else {
      Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Unknown file format",
      ))
    };

    match result {
      Ok(structure) => {
        println!("CLI: Successfully loaded {} atoms.", structure.atoms.len());
        state.borrow_mut().structure = Some(structure);
      }
      Err(e) => {
        eprintln!("CLI Error: Failed to load '{}': {}", path, e);
      }
    }
  }
  // ============================================================

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

  let info_frame = Frame::new(None);
  let console_view = TextView::builder()
    .editable(false)
    .cursor_visible(false)
    .monospace(true)
    .left_margin(10)
    .right_margin(10)
    .top_margin(10)
    .bottom_margin(10)
    .build();
  let scroll_win = ScrolledWindow::builder()
    .min_content_height(150)
    .child(&console_view)
    .build();
  info_frame.set_child(Some(&scroll_win));

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
  let menu_bar = menu::build_menu_and_actions(
    app,
    &window,
    state.clone(),
    &drawing_area,
    &console_view,
    &atom_list_box,
  );

  // 4. Action: Toggle Sidebar
  let toggle_action = gtk4::gio::SimpleAction::new("toggle_sidebar", None);
  let rev_weak = sidebar_revealer.downgrade();
  toggle_action.connect_activate(move |_, _| {
    if let Some(rev) = rev_weak.upgrade() {
      rev.set_reveal_child(!rev.reveals_child());
    }
  });
  app.add_action(&toggle_action);
  app.set_accels_for_action("app.toggle_sidebar", &["F9"]);

  root_vbox.append(&menu_bar);
  root_vbox.append(&main_hbox);

  setup_interactions(&window, state.clone(), &drawing_area, &console_view);

  let s = state.clone();
  drawing_area.set_draw_func(move |_, cr, w, h| {
    let st = s.borrow();

    // Background
    let (bg_r, bg_g, bg_b) = st.style.background_color;
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.paint().unwrap();

    // Scene
    let (atoms, lattice_corners, bounds) =
      rendering::scene::calculate_scene(&st, w as f64, h as f64, false, None, None);

    // Draw Components
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
  });

  window.present();
}
