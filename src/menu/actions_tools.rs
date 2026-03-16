// src/menu/actions_tools.rs

use crate::physics::operations::conversion::{convert_structure, CellType};
use crate::state::AppState;
use crate::ui::dialogs::{basis_dlg, miller_dlg, supercell_dlg};
use crate::utils::console;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, Notebook};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,
    _drawing_area: &DrawingArea,
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
                    supercell_dlg::show(&win, st, &nb);
                }
            }
        }
    });
    app.add_action(&sc_action);

    // --- BASIS / CHEMISTRY ---
    let basis_action = gtk4::gio::SimpleAction::new("basis", None);
    let win_weak_b = window.downgrade();
    let state_weak_b = Rc::downgrade(&state);
    let nb_weak_b = notebook.downgrade();

    basis_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak_b.upgrade() {
            if let Some(st) = state_weak_b.upgrade() {
                if let Some(nb) = nb_weak_b.upgrade() {
                    basis_dlg::show(&win, st, &nb);
                }
            }
        }
    });
    app.add_action(&basis_action);

    // --- MILLER PLANES ---
    let mil_action = gtk4::gio::SimpleAction::new("miller_planes", None);
    let win_weak_m = window.downgrade();
    let state_weak_m = Rc::downgrade(&state);
    let nb_weak_m = notebook.downgrade();

    mil_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak_m.upgrade() {
            if let Some(st) = state_weak_m.upgrade() {
                if let Some(nb) = nb_weak_m.upgrade() {
                    miller_dlg::show(&win, st, &nb);
                }
            }
        }
    });
    app.add_action(&mil_action);

    // --- TOGGLE CELL VIEW (Ctrl+T) ---
    let toggle_action = gtk4::gio::SimpleAction::new("toggle_cell_view", None);
    let st_weak_t = Rc::downgrade(&state);
    let nb_weak_t = notebook.downgrade();

    toggle_action.connect_activate(move |_, _| {
        if let Some(st) = st_weak_t.upgrade() {
            if let Some(nb) = nb_weak_t.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    let target_type = {
                        let mut state_mut = st.borrow_mut();
                        state_mut.config.load_conventional = !state_mut.config.load_conventional;

                        if state_mut.config.load_conventional {
                            CellType::Conventional
                        } else {
                            CellType::Primitive
                        }
                    };

                    convert_and_update(&st, &da, target_type);
                }
            }
        }
    });

    app.add_action(&toggle_action);
}

// --- HELPER FUNCTION ---
fn convert_and_update(state: &Rc<RefCell<AppState>>, da: &DrawingArea, cell_type: CellType) {
    // Read the source structure (original if available, else current).
    // Use shared borrow — we only need to read here.
    let source = {
        let st = state.borrow();
        let tab = st.active_tab();
        tab.original_structure
            .as_ref()
            .or(tab.structure.as_ref())
            .cloned()
    };

    if let Some(structure) = source {
        match convert_structure(&structure, cell_type) {
            Ok(new_struct) => {
                let view_name = match cell_type {
                    CellType::Primitive => "Primitive",
                    CellType::Conventional => "Conventional",
                };
                console::log_info(&format!(
                    "Switched to {} cell — {} ({} atoms)",
                    view_name,
                    new_struct.formula,
                    new_struct.atoms.len()
                ));

                let mut st = state.borrow_mut();
                let tab = st.active_tab_mut();
                tab.structure = Some(new_struct);
                tab.invalidate_bvs_cache();

                da.queue_draw();
            }
            Err(e) => {
                console::log_error(&format!("Cell conversion failed: {}", e));
            }
        }
    }
}
