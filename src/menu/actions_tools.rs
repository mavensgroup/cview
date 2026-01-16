// src/menu/actions_tools.rs

use crate::physics::operations::conversion::{convert_structure, CellType};
use crate::state::AppState;
use crate::ui::dialogs::{miller_dlg, supercell_dlg};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, Notebook}; // <--- Ensure Notebook is imported
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,        // <--- Input: Notebook
    drawing_area: &DrawingArea, // <--- Input: DrawingArea (Legacy/Fallback)
) {
    // --- SUPERCELL ---
    let sc_action = gtk4::gio::SimpleAction::new("supercell", None);
    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let nb_weak = notebook.downgrade();

    sc_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            if let Some(st) = state_weak.upgrade() {
                if let Some(nb) = nb_weak.upgrade() {
                    // Pass notebook so dialog updates the correct tab
                    supercell_dlg::show(&win, st, &nb);
                }
            }
        }
    });
    app.add_action(&sc_action);

    // --- MILLER PLANES ---
    // (Updated to use notebook as discussed)
    let mil_action = gtk4::gio::SimpleAction::new("miller_planes", None);
    let win_weak_m = window.downgrade();
    let state_weak_m = Rc::downgrade(&state);
    let nb_weak_m = notebook.downgrade();

    mil_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak_m.upgrade() {
            if let Some(st) = state_weak_m.upgrade() {
                if let Some(nb) = nb_weak_m.upgrade() {
                    // Pass notebook so dialog updates the correct tab
                    miller_dlg::show(&win, st, &nb);
                }
            }
        }
    });
    app.add_action(&mil_action);

    // --- TOGGLE CELL VIEW ---
    let toggle_action = gtk4::gio::SimpleAction::new("toggle_cell_view", None);
    let st_weak_t = Rc::downgrade(&state);
    let nb_weak_t = notebook.downgrade();

    toggle_action.connect_activate(move |_, _| {
        if let Some(st) = st_weak_t.upgrade() {
            if let Some(nb) = nb_weak_t.upgrade() {
                // FIX 1: Find the currently active drawing area
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    // Logic: Toggle config and determine target type
                    let target_type = {
                        let mut state_mut = st.borrow_mut();
                        // Toggle the boolean
                        state_mut.config.load_conventional = !state_mut.config.load_conventional;

                        // Decide conversion target
                        if state_mut.config.load_conventional {
                            CellType::Conventional
                        } else {
                            CellType::Primitive
                        }
                    };

                    // FIX 2: Convert the structure in the ACTIVE tab
                    convert_and_update(&st, &da, target_type);
                }
            }
        }
    });

    app.add_action(&toggle_action);
}

// --- HELPER FUNCTION ---
fn convert_and_update(state: &Rc<RefCell<AppState>>, da: &DrawingArea, cell_type: CellType) {
    let mut st = state.borrow_mut();

    // CRITICAL FIX: Access the ACTIVE tab, not the first one
    let tab = st.active_tab_mut();

    // Always convert from 'original_structure' (of the active tab)
    // to ensure the toggle is consistent and doesn't drift.
    let source = tab
        .original_structure
        .as_ref()
        .or(tab.structure.as_ref())
        .cloned();

    if let Some(structure) = source {
        match convert_structure(&structure, cell_type) {
            Ok(new_struct) => {
                let view_name = match cell_type {
                    CellType::Primitive => "Primitive",
                    CellType::Conventional => "Conventional",
                };
                println!(
                    "Switched to {} View. Formula: {}",
                    view_name, new_struct.formula
                );

                // Update the ACTIVE tab's structure
                // We must re-borrow mutably since 'tab' borrow ended above
                let tab_inner = st.active_tab_mut();
                tab_inner.structure = Some(new_struct);

                // Redraw the specific canvas we found earlier
                da.queue_draw();
            }
            Err(e) => eprintln!("Conversion error: {}", e),
        }
    }
}
