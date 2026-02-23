// src/ui/dialogs/basis_dlg.rs

use crate::physics::operations::basis;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{
    Align, Button, Dialog, Entry, Grid, Label, Notebook, Orientation, ResponseType, Window,
};
use std::cell::RefCell;
use std::rc::Rc;

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
    // Removed btn_delete definition here

    grid_edit.attach(&Label::new(Some("New Element:")), 0, 0, 1, 1);
    grid_edit.attach(&entry_el, 1, 0, 1, 1);
    grid_edit.attach(&btn_change, 2, 0, 1, 1);

    box_sel.append(&lbl_count);
    box_sel.append(&grid_edit);
    // Removed box_sel.append(&btn_delete) here

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

    // Update Label on Open
    {
        let s = state.borrow();
        let st = s.active_tab();
        let n = st.interaction.selected_indices.len();
        lbl_count.set_text(&format!("{} atoms selected", n));
        println!("BASIS: Opened with {} atoms selected.", n);

        let has_sel = n > 0;
        btn_change.set_sensitive(has_sel);
    }

    // 2. CHANGE ELEMENT BUTTON logic
    let entry_el_c1 = entry_el.clone();
    let lbl_count_c1 = lbl_count.clone();
    let btn_change_c1 = btn_change.clone();

    // Clone weak ref specifically for this closure
    let state_weak_c1 = state_weak.clone();
    let nb_weak_c1 = notebook_weak.clone();

    btn_change.connect_clicked(move |_| {
        println!("BASIS: 'Change Element' clicked.");
        if let Some(st) = state_weak_c1.upgrade() {
            let mut s = st.borrow_mut();
            let tab = s.active_tab_mut();

            let sel_indices: Vec<usize> =
                tab.interaction.selected_indices.iter().cloned().collect();
            let new_el = entry_el_c1.text().trim().to_string();

            println!(
                "BASIS: Modifying {} atoms to '{}'",
                sel_indices.len(),
                new_el
            );

            if !sel_indices.is_empty() && !new_el.is_empty() {
                if let Some(current_s) = &tab.structure {
                    let ns = basis::modify_selection(current_s, &sel_indices, &new_el);

                    // 1. Update structure
                    tab.structure = Some(ns);

                    // 2. Clear selection (Critical for visual update)
                    tab.interaction.selected_indices.clear();

                    // 3. Update UI
                    lbl_count_c1.set_text("0 atoms selected");
                    btn_change_c1.set_sensitive(false);

                    // 4. Force Redraw
                    if let Some(nb) = nb_weak_c1.upgrade() {
                        if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                            da.queue_draw();
                        }
                    }
                    println!("BASIS: Update applied successfully.");
                }
            }
        }
    });

    // 4. GLOBAL REPLACE
    let state_weak_sub = Rc::downgrade(&state);
    let nb_weak_sub = notebook.downgrade();
    btn_sub.connect_clicked(move |_| {
        if let Some(st) = state_weak_sub.upgrade() {
            let mut s = st.borrow_mut();
            let tab = s.active_tab_mut();

            let from = entry_find.text().trim().to_string();
            let to = entry_replace.text().trim().to_string();

            if !from.is_empty() && !to.is_empty() {
                if let Some(current_s) = &tab.structure {
                    let ns = basis::substitute_element(current_s, &from, &to);
                    tab.structure = Some(ns);
                    if let Some(nb) = nb_weak_sub.upgrade() {
                        if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                            da.queue_draw();
                        }
                    }
                }
            }
        }
    });

    // 5. STANDARDIZE
    let state_weak_std = Rc::downgrade(&state);
    let nb_weak_std = notebook.downgrade();
    btn_wrap.connect_clicked(move |_| {
        if let Some(st) = state_weak_std.upgrade() {
            let mut s = st.borrow_mut();
            let tab = s.active_tab_mut();

            if let Some(current_s) = &tab.structure {
                let ns = basis::standardize_positions(current_s);
                tab.structure = Some(ns);
                if let Some(nb) = nb_weak_std.upgrade() {
                    if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                        da.queue_draw();
                    }
                }
            }
        }
    });

    dialog.add_button("Close", ResponseType::Close);
    dialog.connect_response(|dlg, _| dlg.close());
    dialog.show();
}
