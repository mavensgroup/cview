// src/ui/dialogs/supercell_dlg.rs

use crate::physics::operations::supercell;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Align, CheckButton, Dialog, Grid, Notebook, ResponseType, SpinButton, Window};
use std::cell::RefCell;
use std::rc::Rc;

pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    let dialog = Dialog::builder()
        .title("Matrix Transformation")
        .transient_for(parent)
        .modal(true)
        .default_width(340)
        .build();

    let content = dialog.content_area();
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);

    // --- Mode Toggle (Diagonal vs General) ---
    let box_mode = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    box_mode.set_halign(Align::Center);
    box_mode.set_margin_bottom(15);

    let check_general = CheckButton::with_label("General Matrix (Shear/Swap)");
    check_general.set_active(false);

    box_mode.append(&check_general);
    content.append(&box_mode);

    // --- 3x3 Integer Matrix Grid ---
    let grid = Grid::new();
    grid.set_row_spacing(5);
    grid.set_column_spacing(10);
    grid.set_halign(Align::Center);

    let mut spins_vec = Vec::new();

    for r in 0..3 {
        for c in 0..3 {
            let default_val = if r == c { 1.0 } else { 0.0 };

            // Integer steps only — no fractional cell transformations
            let spin = SpinButton::with_range(-20.0, 20.0, 1.0);
            spin.set_digits(0);
            spin.set_value(default_val);
            spin.set_width_chars(4);
            spin.set_snap_to_ticks(true);

            // Off-diagonals disabled until general mode is enabled
            if r != c {
                spin.set_sensitive(false);
            }

            grid.attach(&spin, c, r, 1, 1);
            spins_vec.push(spin);
        }
    }

    let spins = Rc::new(spins_vec);
    content.append(&grid);

    // --- Toggle Logic ---
    let spins_clone = spins.clone();
    check_general.connect_toggled(move |btn| {
        let is_general = btn.is_active();
        for (i, spin) in spins_clone.iter().enumerate() {
            let r = i / 3;
            let c = i % 3;
            if r != c {
                spin.set_sensitive(is_general);
                if !is_general {
                    spin.set_value(0.0);
                }
            }
        }
    });

    // --- Buttons ---
    dialog.add_button("Reset", ResponseType::Reject);
    dialog.add_button("Transform", ResponseType::Ok);

    // --- Response ---
    let state_weak = Rc::downgrade(&state);
    let notebook_weak = notebook.downgrade();
    let spins_final = spins.clone();

    dialog.connect_response(move |dlg, resp| {
        if let Some(st) = state_weak.upgrade() {
            let mut s = st.borrow_mut();
            let tab = s.active_tab_mut();

            match resp {
                ResponseType::Ok => {
                    let mut mat = [[0i32; 3]; 3];
                    for r in 0..3 {
                        for c in 0..3 {
                            // Round to nearest integer — spin already enforces step=1
                            // but rounding makes it bulletproof
                            mat[r][c] = spins_final[r * 3 + c].value().round() as i32;
                        }
                    }

                    if let Some(orig) = &tab.original_structure {
                        let new_s = supercell::transform(orig, mat);
                        tab.structure = Some(new_s);
                        tab.interaction.selected.clear();
                        tab.invalidate_bvs_cache();

                        if let Some(nb) = notebook_weak.upgrade() {
                            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                                da.queue_draw();
                            }
                        }
                    }
                }
                ResponseType::Reject => {
                    if let Some(orig) = &tab.original_structure {
                        tab.structure = Some(orig.clone());
                        tab.interaction.selected.clear();
                        tab.invalidate_bvs_cache();

                        for (i, spin) in spins_final.iter().enumerate() {
                            let r = i / 3;
                            let c = i % 3;
                            spin.set_value(if r == c { 1.0 } else { 0.0 });
                        }

                        if let Some(nb) = notebook_weak.upgrade() {
                            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                                da.queue_draw();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        dlg.close();
    });

    dialog.show();
}
