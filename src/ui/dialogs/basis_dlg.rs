// src/ui/dialogs/basis_dlg.rs

use crate::physics::operations::basis;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{
    Align, Button, Dialog, Entry, Grid, Label, Notebook, Orientation, ResponseType, Window,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Returns the selected original indices, sorted. Since selected_indices now stores
/// original_index directly (not unique_id), this is a simple sorted collect.
fn resolve_selected_original_indices(state: &AppState) -> Vec<usize> {
    let tab = state.active_tab();
    let mut result: Vec<usize> = tab.interaction.selected_indices.iter().cloned().collect();
    result.sort_unstable();
    result
}

pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    let dialog = Dialog::builder()
        .title("Basis Operations")
        .transient_for(parent)
        .modal(true)
        .default_width(360)
        .build();

    let content = dialog.content_area();
    content.set_margin_top(15);
    content.set_margin_bottom(15);
    content.set_margin_start(15);
    content.set_margin_end(15);

    let tabs = Notebook::new();

    // --- TAB 1: SELECTION ---
    let box_sel = gtk4::Box::new(Orientation::Vertical, 10);
    box_sel.set_margin_top(15);
    box_sel.set_halign(Align::Center);

    let lbl_count = Label::new(Some("No atoms selected"));

    let grid_edit = Grid::new();
    grid_edit.set_column_spacing(10);
    grid_edit.set_row_spacing(10);

    let entry_el = Entry::new();
    entry_el.set_placeholder_text(Some("e.g. Au"));
    entry_el.set_width_chars(6);

    let btn_change = Button::with_label("Change Element");

    grid_edit.attach(&Label::new(Some("New Element:")), 0, 0, 1, 1);
    grid_edit.attach(&entry_el, 1, 0, 1, 1);
    grid_edit.attach(&btn_change, 2, 0, 1, 1);

    box_sel.append(&lbl_count);
    box_sel.append(&grid_edit);

    tabs.append_page(&box_sel, Some(&Label::new(Some("Selection"))));

    // --- TAB 2: GLOBAL REPLACE ---
    let grid_sub = Grid::new();
    grid_sub.set_row_spacing(10);
    grid_sub.set_column_spacing(10);
    grid_sub.set_halign(Align::Center);
    grid_sub.set_margin_top(15);

    let entry_find = Entry::new();
    entry_find.set_placeholder_text(Some("e.g. Si"));
    let entry_replace = Entry::new();
    entry_replace.set_placeholder_text(Some("e.g. Ge"));
    let btn_sub = Button::with_label("Replace All");

    grid_sub.attach(&Label::new(Some("Find:")), 0, 0, 1, 1);
    grid_sub.attach(&entry_find, 1, 0, 1, 1);
    grid_sub.attach(&Label::new(Some("Replace:")), 0, 1, 1, 1);
    grid_sub.attach(&entry_replace, 1, 1, 1, 1);
    grid_sub.attach(&btn_sub, 0, 2, 2, 1);

    tabs.append_page(&grid_sub, Some(&Label::new(Some("Global"))));

    // --- TAB 3: TOOLS ---
    let box_std = gtk4::Box::new(Orientation::Vertical, 10);
    box_std.set_halign(Align::Center);
    box_std.set_margin_top(20);
    let btn_wrap = Button::with_label("Standardize Positions [0, 1)");
    box_std.append(&btn_wrap);
    tabs.append_page(&box_std, Some(&Label::new(Some("Tools"))));

    content.append(&tabs);

    // --- LOGIC ---
    let state_weak = Rc::downgrade(&state);
    let notebook_weak = notebook.downgrade();

    // 1. UPDATE LABEL ON OPEN
    {
        let s = state.borrow();
        let st = s.active_tab();
        let n = st.interaction.selected_indices.len();
        lbl_count.set_text(&format!("{} atoms selected", n));
        btn_change.set_sensitive(n > 0);
    }

    // 2. CHANGE ELEMENT (Selection)
    let entry_el_clone = entry_el.clone();
    let dialog_weak = dialog.downgrade();

    btn_change.connect_clicked(move |_| {
        if let Some(st) = state_weak.upgrade() {
            let new_el = entry_el_clone.text();
            if new_el.is_empty() {
                return;
            }

            let original_indices = {
                let s = st.borrow();
                resolve_selected_original_indices(&s)
            };

            if !original_indices.is_empty() {
                let mut s = st.borrow_mut();
                let tab = s.active_tab_mut();
                if let Some(current_s) = &tab.structure {
                    let new_s = basis::modify_selection(current_s, &original_indices, &new_el);
                    tab.structure = Some(new_s);
                    tab.invalidate_bvs_cache();

                    if let Some(nb) = notebook_weak.upgrade() {
                        if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                            da.queue_draw();
                        }
                    }

                    if let Some(dlg) = dialog_weak.upgrade() {
                        dlg.close();
                    }
                }
            }
        }
    });

    // 3. GLOBAL REPLACE
    let state_weak_sub = Rc::downgrade(&state);
    let nb_weak_sub = notebook.downgrade();
    let dialog_weak_sub = dialog.downgrade();

    btn_sub.connect_clicked(move |_| {
        if let Some(st) = state_weak_sub.upgrade() {
            let mut s = st.borrow_mut();
            let tab = s.active_tab_mut();
            if let Some(current_s) = &tab.structure {
                let from = entry_find.text();
                let to = entry_replace.text();
                if !from.is_empty() && !to.is_empty() {
                    let new_s = basis::substitute_element(current_s, &from, &to);
                    tab.structure = Some(new_s);
                    tab.invalidate_bvs_cache();

                    if let Some(nb) = nb_weak_sub.upgrade() {
                        if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                            da.queue_draw();
                        }
                    }

                    if let Some(dlg) = dialog_weak_sub.upgrade() {
                        dlg.close();
                    }
                }
            }
        }
    });

    // 4. STANDARDIZE
    let state_weak_std = Rc::downgrade(&state);
    let nb_weak_std = notebook.downgrade();
    let dialog_weak_std = dialog.downgrade();

    btn_wrap.connect_clicked(move |_| {
        if let Some(st) = state_weak_std.upgrade() {
            let mut s = st.borrow_mut();
            let tab = s.active_tab_mut();
            if let Some(current_s) = &tab.structure {
                let new_s = basis::standardize_positions(current_s);
                tab.structure = Some(new_s);
                tab.invalidate_bvs_cache();

                if let Some(nb) = nb_weak_std.upgrade() {
                    if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                        da.queue_draw();
                    }
                }

                if let Some(dlg) = dialog_weak_std.upgrade() {
                    dlg.close();
                }
            }
        }
    });

    dialog.add_button("Close", ResponseType::Close);
    dialog.connect_response(|dlg, _| dlg.close());
    dialog.show();
}
