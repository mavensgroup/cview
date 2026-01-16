// src/main.rs

use gtk4::prelude::*;
use gtk4::Box as GtkBox;
use gtk4::{
    Application, ApplicationWindow, Frame, Label, Notebook, Orientation, Revealer,
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

    // Run with empty args so we can handle CLI args manually in build_ui
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

    // --- VIEW NOTEBOOK (TABBED INTERFACE) ---
    let view_notebook = Notebook::new();
    view_notebook.set_vexpand(true);
    view_notebook.set_scrollable(true);

    // --- CONSOLE NOTEBOOK ---
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

    if let Err(e) = utils::logger::init(&system_log_view) {
        eprintln!("Failed to initialize logger: {}", e);
    }
    log::info!("System started ready.");

    let scroll_logs = ScrolledWindow::builder().child(&system_log_view).build();
    console_notebook.append_page(&scroll_logs, Some(&Label::new(Some("System Logs"))));

    let info_frame = Frame::new(None);
    info_frame.set_child(Some(&console_notebook));

    right_vbox.append(&view_notebook);
    right_vbox.append(&info_frame);

    // --- INITIAL TAB SETUP ---
    // Create content
    let (first_da, first_tab_box) = ui::create_tab_content(state.clone(), 0);

    // NEW: Use helper to add a Closable Tab (with X button)
    ui::add_closable_tab(&view_notebook, &first_tab_box, "Untitled", state.clone());

    // --- LEFT PANEL (Sidebar) ---
    use panels::sidebar;
    let (sidebar_widget, atom_list_box) = sidebar::build(state.clone(), &view_notebook);

    let sidebar_revealer = Revealer::builder()
        .transition_type(RevealerTransitionType::SlideRight)
        .child(&sidebar_widget)
        .reveal_child(true)
        .build();

    main_hbox.append(&sidebar_revealer);
    main_hbox.append(&right_vbox);

    // --- MENU BAR ---
    let menu_bar = menu::build_menu_and_actions(
        app,
        &window,
        state.clone(),
        &view_notebook,
        &first_da,
        &system_log_view,
        &interactions_view,
        &atom_list_box,
    );

    // --- ACTIONS ---

    // 1. Toggle Sidebar (F9)
    let toggle_action = gtk4::gio::SimpleAction::new("toggle_sidebar", None);
    let rev_weak = sidebar_revealer.downgrade();
    toggle_action.connect_activate(move |_, _| {
        if let Some(rev) = rev_weak.upgrade() {
            rev.set_reveal_child(!rev.reveals_child());
        }
    });
    app.add_action(&toggle_action);
    app.set_accels_for_action("app.toggle_sidebar", &["F9"]);

    // 2. Quit
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

    // 3. Close Tab (Ctrl+W) - NEW
    let close_tab_action = gtk4::gio::SimpleAction::new("close_tab", None);
    let nb_close = view_notebook.downgrade();
    let st_close = state.clone();

    close_tab_action.connect_activate(move |_, _| {
        if let Some(nb) = nb_close.upgrade() {
            if let Some(page_idx) = nb.current_page() {
                // Remove from State
                st_close.borrow_mut().remove_tab(page_idx as usize);
                // Remove from UI
                nb.remove_page(Some(page_idx));
            }
        }
    });
    app.add_action(&close_tab_action);
    app.set_accels_for_action("app.close_tab", &["<Control>w"]);

    // --- ASSEMBLE ---
    root_vbox.append(&menu_bar);
    root_vbox.append(&main_hbox);

    // --- INTERACTIONS ---
    setup_interactions(&window, state.clone(), &first_da, &interactions_view);

    // --- TAB SWITCHING LOGIC ---
    let state_nb = state.clone();
    view_notebook.connect_switch_page(move |_, _, page_num| {
        let mut st = state_nb.borrow_mut();
        st.active_tab_index = page_num as usize;
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
                {
                    let mut st = state.borrow_mut();
                    let tab = st.active_tab_mut();
                    tab.original_structure = Some(structure.clone());
                    tab.structure = Some(structure);
                    tab.file_name = std::path::Path::new(path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // FIX: Update Label inside the Custom Closable Tab Box
                    if let Some(page) = view_notebook.nth_page(Some(0)) {
                        if let Some(lbl_widget) = view_notebook.tab_label(&page) {
                            // Since we used add_closable_tab, the label is inside a GtkBox
                            if let Some(bx) = lbl_widget.downcast_ref::<GtkBox>() {
                                // Assume first child is the Label
                                if let Some(first_child) = bx.first_child() {
                                    if let Some(l) = first_child.downcast_ref::<Label>() {
                                        l.set_text(&tab.file_name);
                                    }
                                }
                            }
                        }
                    }
                }

                log_msg(&system_log_view, &format!("File loaded via CLI: {}", path));

                let st = state.borrow();
                let tab = st.active_tab();
                if let Some(s) = &tab.structure {
                    let report = utils::report::structure_summary(s, &tab.file_name);
                    log_msg(&interactions_view, &report);
                }
                drop(st);

                panels::sidebar::refresh_atom_list(&atom_list_box, state.clone(), &view_notebook);
                first_da.queue_draw();
            }
            Err(e) => {
                log_msg(&system_log_view, &format!("Error loading CLI file: {}", e));
            }
        }
    }
}
