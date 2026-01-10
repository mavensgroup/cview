use gtk4::{Application, ApplicationWindow, DrawingArea, TextView};
use gtk4::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;

pub mod actions_file;
pub mod actions_view;
pub mod actions_tools;
pub mod actions_analysis;
pub mod actions_help;
pub mod tool_supercell;
pub mod tool_miller;

pub fn build_menu_and_actions(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
    console_view: &TextView,
    atom_list_box: &gtk4::Box,
) -> gtk4::Box {
    // Register Actions
    actions_file::setup(app, window, state.clone(), drawing_area, console_view, atom_list_box);
    actions_view::setup(app, window, state.clone(), drawing_area);
    actions_tools::setup(app, window, state.clone(), drawing_area); // Note: If your tools setup needs console, add it here
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
    // Help shortcuts (F1 is standard for help)
    app.set_accels_for_action("app.help", &["F1"]);

    // Build Menu Bar
    let menu_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    let bar_widget = gtk4::PopoverMenuBar::builder().build();
    let root_model = gtk4::gio::Menu::new();

    // --- FILE MENU ---
    let file_menu = gtk4::gio::Menu::new();
    file_menu.append(Some("Open Structure..."), Some("app.open"));
    file_menu.append(Some("Save As..."), Some("app.save_as"));
    file_menu.append(Some("Export Image..."), Some("app.export"));
    file_menu.append(Some("Preferences..."), Some("app.preferences"));
    file_menu.append(Some("Quit"), Some("app.quit"));
    root_model.append_submenu(Some("File"), &file_menu);

    // --- VIEW MENU ---
    let view_menu = gtk4::gio::Menu::new();
    view_menu.append(Some("Restore View"), Some("app.view_reset"));

    // View Along Submenu
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

    // --- HELP MENU (NEW) ---
    let help_menu = gtk4::gio::Menu::new();
    help_menu.append(Some("Documentation"), Some("app.help"));
    help_menu.append(Some("About cview"), Some("app.about"));
    root_model.append_submenu(Some("Help"), &help_menu);

    bar_widget.set_menu_model(Some(&root_model));
    menu_bar.append(&bar_widget);

    menu_bar
}
