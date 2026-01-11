// src/menu/mod.rs

use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, TextView};
use std::cell::RefCell;
use std::rc::Rc;

pub mod actions_analysis;
pub mod actions_file;
pub mod actions_help;
pub mod actions_tools;
pub mod actions_view;

pub fn build_menu_and_actions(
  app: &Application,
  window: &ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: &DrawingArea,
  system_log_view: &TextView,   // Arg 5: System Logs
  interactions_view: &TextView, // Arg 6: Interactions
  atom_list_box: &gtk4::Box,    // Arg 7: Sidebar
) -> gtk4::Box {
  // Register Actions
  actions_file::setup(
    app,
    window,
    state.clone(),
    drawing_area,
    system_log_view,
    interactions_view,
    atom_list_box,
  );

  actions_view::setup(app, window, state.clone(), drawing_area);
  actions_tools::setup(app, window, state.clone(), drawing_area);
  actions_analysis::setup(app, window, state.clone());
  actions_help::setup(app, window);

  // Keyboard Shortcuts
  app.set_accels_for_action("app.open", &["<Primary>o"]);
  app.set_accels_for_action("app.save_as", &["<Primary><Shift>s"]);
  app.set_accels_for_action("app.export", &["<Primary>e"]);
  app.set_accels_for_action("app.preferences", &["<Primary>p"]);
  app.set_accels_for_action("app.quit", &["<Primary>q"]);
  app.set_accels_for_action("app.view_reset", &["<Primary>r"]);
  app.set_accels_for_action("app.toggle_bonds", &["<Primary>b"]);
  app.set_accels_for_action("app.supercell", &["<Primary><Shift>c"]);
  app.set_accels_for_action("app.miller_planes", &["<Primary>m"]);

  // --- BUILD MENU BAR ---
  let menu_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
  let root_model = gtk4::gio::Menu::new();

  // --- FILE MENU ---
  let file_menu = gtk4::gio::Menu::new();
  file_menu.append(Some("Open..."), Some("app.open"));

  // Ungrouped Export/Save
  file_menu.append(Some("Save Structure As..."), Some("app.save_as"));
  file_menu.append(Some("Export Image/PDF..."), Some("app.export"));

  // Preferences Added Back
  file_menu.append(Some("Preferences..."), Some("app.preferences"));

  file_menu.append(Some("Quit"), Some("app.quit"));
  root_model.append_submenu(Some("File"), &file_menu);

  // --- VIEW MENU ---
  let view_menu = gtk4::gio::Menu::new();
  view_menu.append(Some("Restore View"), Some("app.view_reset"));

  let view_along_submenu = gtk4::gio::Menu::new();
  view_along_submenu.append(Some("Along a-axis (X)"), Some("app.view_along_a"));
  view_along_submenu.append(Some("Along b-axis (Y)"), Some("app.view_along_b"));
  view_along_submenu.append(Some("Along c-axis (Z)"), Some("app.view_along_c"));
  view_menu.append_submenu(Some("View Along"), &view_along_submenu);

  view_menu.append(Some("Toggle Bonds"), Some("app.toggle_bonds"));
  root_model.append_submenu(Some("View"), &view_menu);

  // --- TOOLS MENU ---
  let tools_menu = gtk4::gio::Menu::new();
  tools_menu.append(Some("Supercell..."), Some("app.supercell"));
  tools_menu.append(Some("Miller Indices..."), Some("app.miller_planes"));
  root_model.append_submenu(Some("Tools"), &tools_menu);

  // --- ANALYSIS MENU ---
  let analysis_menu = gtk4::gio::Menu::new();
  analysis_menu.append(Some("Symmetry Analysis"), Some("app.analysis"));
  root_model.append_submenu(Some("Analysis"), &analysis_menu);

  // --- HELP MENU ---
  let help_menu = gtk4::gio::Menu::new();
  help_menu.append(Some("Controls & Shortcuts"), Some("app.help_controls"));
  help_menu.append(Some("About"), Some("app.help_about"));
  root_model.append_submenu(Some("Help"), &help_menu);

  let popover_bar = gtk4::PopoverMenuBar::from_model(Some(&root_model));
  menu_bar.append(&popover_bar);

  menu_bar
}
