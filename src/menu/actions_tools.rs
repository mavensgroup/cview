// src/menu/actions_tools.rs

use crate::physics::operations::conversion::{convert_structure, CellType};
use crate::state::AppState;
use crate::ui::dialogs::miller_dlg;
use crate::ui::structure::window::{show_structure_window, StructureTab};
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
    // --- STRUCTURE WINDOW: Supercell, Basis, Atom Instances, Slab ---
    // One window with a tab per tool; each action opens it on its own tab.
    for (name, tab) in [
        ("supercell", StructureTab::Supercell),
        ("basis", StructureTab::Basis),
        ("atom_instances", StructureTab::Instances),
        ("slab", StructureTab::Slab),
    ] {
        let action = gtk4::gio::SimpleAction::new(name, None);
        let win_weak = window.downgrade();
        let state_weak = Rc::downgrade(&state);
        let nb_weak = notebook.downgrade();

        action.connect_activate(move |_, _| {
            if let (Some(win), Some(st), Some(nb)) =
                (win_weak.upgrade(), state_weak.upgrade(), nb_weak.upgrade())
            {
                show_structure_window(&win, st, &nb, tab);
            }
        });
        app.add_action(&action);
    }

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

    // --- TOGGLE CELL VIEW (T on canvas) ---
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
    // Convert the structure as it stands now, so edits made since load
    // (deletions, element swaps, a supercell) survive the toggle. Falls back to
    // the as-loaded cell only if the tab somehow holds no structure.
    // Use shared borrow — we only need to read here.
    let source = {
        let st = state.borrow();
        let tab = st.active_tab();
        tab.structure
            .as_ref()
            .or(tab.original_structure.as_ref())
            .cloned()
    };

    if let Some(structure) = source {
        match convert_structure(&structure, cell_type) {
            Ok(new_struct) => {
                let view_name = match cell_type {
                    CellType::Primitive => "Primitive",
                    CellType::Conventional => "Conventional",
                };
                let mut st = state.borrow_mut();
                let tab = st.active_tab_mut();
                console::structure_changed(&format!("Switched to {} cell", view_name), &new_struct);
                tab.structure = Some(new_struct);
                tab.invalidate_derived();

                da.queue_draw();
            }
            Err(e) => {
                console::log_error(&format!("Cell conversion failed: {}", e));
            }
        }
    }
}
