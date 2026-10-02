// src/ui/structure/supercell_tab.rs
//
// Integer 3x3 transformation matrix with a live preview of the result.

use super::preview::Preview;
use super::TabParts;
use crate::physics::operations::supercell;
use crate::state::AppState;
use crate::ui::style::{card_body, plain_field, surface};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, CheckButton, Grid, Label, Notebook, Orientation, SpinButton};
use std::cell::RefCell;
use std::rc::Rc;

/// Past this many atoms the preview is skipped (the transform and the draw
/// run on every spin change); the result is still shown as a count.
const PREVIEW_MAX_ATOMS: i64 = 4000;

fn matrix_of(spins: &[SpinButton]) -> [[i32; 3]; 3] {
    let mut m = [[0i32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            // Spin already enforces step 1; rounding makes it bulletproof.
            m[r][c] = spins[r * 3 + c].value().round() as i32;
        }
    }
    m
}

fn det(m: [[i32; 3]; 3]) -> i64 {
    let a = |r: usize, c: usize| m[r][c] as i64;
    a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
        - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0))
}

fn set_identity(spins: &[SpinButton]) {
    for (i, spin) in spins.iter().enumerate() {
        spin.set_value(if i / 3 == i % 3 { 1.0 } else { 0.0 });
    }
}

pub fn build(
    state: Rc<RefCell<AppState>>,
    main_notebook: &Notebook,
    preview: Preview,
) -> TabParts {
    let controls = GtkBox::new(Orientation::Vertical, 10);

    // --- Matrix card ---
    let check_general = CheckButton::with_label("General matrix (shear/swap)");
    check_general.set_halign(Align::Start);

    let grid = Grid::new();
    grid.set_row_spacing(5);
    grid.set_column_spacing(8);
    grid.set_column_homogeneous(true);

    let mut spins_vec = Vec::new();
    for r in 0..3 {
        for c in 0..3 {
            // Integer steps only: no fractional cell transformations.
            let spin = SpinButton::with_range(-20.0, 20.0, 1.0);
            spin.set_digits(0);
            spin.set_value(if r == c { 1.0 } else { 0.0 });
            spin.set_snap_to_ticks(true);
            // Off-diagonals stay disabled until general mode is on.
            spin.set_sensitive(r == c);
            grid.attach(&plain_field(&spin), c, r, 1, 1);
            spins_vec.push(spin);
        }
    }
    let spins = Rc::new(spins_vec);

    let matrix_body = card_body(12);
    matrix_body.append(&grid);
    matrix_body.append(&check_general);
    controls.append(&surface(&matrix_body));

    // --- Result readout ---
    let lbl_result = Label::new(None);
    lbl_result.set_halign(Align::Start);
    lbl_result.set_wrap(true);
    lbl_result.set_xalign(0.0);
    lbl_result.add_css_class("cview-caption");
    controls.append(&lbl_result);

    // --- Buttons ---
    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    btn_row.set_homogeneous(true);
    let btn_reset = Button::with_label("Reset to as-loaded");
    let btn_apply = Button::with_label("Transform");
    btn_apply.add_css_class("suggested-action");
    btn_row.append(&btn_reset);
    btn_row.append(&btn_apply);
    controls.append(&btn_row);

    // --- Live preview ---
    let update: Rc<dyn Fn()> = {
        let spins = spins.clone();
        let state = state.clone();
        let preview = preview.clone();
        let lbl = lbl_result.clone();
        let apply = btn_apply.clone();
        Rc::new(move || {
            let m = matrix_of(&spins);
            let d = det(m);
            // A highlight from another tab refers to the structure as it is
            // now, not to the candidate.
            preview.clear_highlight();

            let st = state.borrow();
            let tab = st.active_tab();
            let Some(src) = tab.structure.as_ref().or(tab.original_structure.as_ref()) else {
                lbl.set_text("No structure loaded.");
                apply.set_sensitive(false);
                preview.set_candidate(None);
                return;
            };

            if d == 0 {
                lbl.set_text("Singular matrix (determinant 0): choose independent rows.");
                apply.set_sensitive(false);
                preview.set_candidate(None);
                return;
            }
            apply.set_sensitive(true);

            let n_new = d.abs() * src.atoms.len() as i64;
            if d == 1 && m == [[1, 0, 0], [0, 1, 0], [0, 0, 1]] {
                lbl.set_text(&format!("Identity: {} atoms, no change.", src.atoms.len()));
                preview.set_candidate(None);
            } else if n_new > PREVIEW_MAX_ATOMS {
                lbl.set_text(&format!(
                    "Result: {} atoms (×{}). Too large to preview.",
                    n_new,
                    d.abs()
                ));
                preview.set_candidate(None);
            } else {
                let result = supercell::transform(src, m);
                lbl.set_text(&format!(
                    "Result: {} atoms (×{}), {}.",
                    result.atoms.len(),
                    d.abs(),
                    result.formula
                ));
                preview.set_candidate(Some(result));
            }
        })
    };

    for spin in spins.iter() {
        let u = update.clone();
        spin.connect_value_changed(move |_| u());
    }

    // General-matrix toggle: off-diagonals editable, or zeroed again.
    {
        let spins = spins.clone();
        check_general.connect_toggled(move |btn| {
            let general = btn.is_active();
            for (i, spin) in spins.iter().enumerate() {
                if i / 3 != i % 3 {
                    spin.set_sensitive(general);
                    if !general {
                        spin.set_value(0.0);
                    }
                }
            }
        });
    }

    // --- Apply / Reset ---
    {
        let state = state.clone();
        let nb = main_notebook.downgrade();
        let spins = spins.clone();
        let check = check_general.clone();
        btn_apply.connect_clicked(move |_| {
            let m = matrix_of(&spins);
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                // Transform the structure as it stands now, so edits made since
                // load (deletions, element swaps, an earlier transform) are
                // carried into the supercell. Reset goes back to the as-loaded cell.
                let source = tab.structure.as_ref().or(tab.original_structure.as_ref());
                if let Some(src) = source {
                    let new_s = supercell::transform(src, m);
                    tab.structure = Some(new_s);
                    tab.interaction.selected.clear();
                    tab.invalidate_derived();
                    if let Some(s) = &tab.structure {
                        crate::utils::console::structure_changed("Supercell", s);
                    }
                }
            }
            if let Some(nb) = nb.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
            // The matrix has been applied: back to identity so the preview
            // shows the structure as it is now, not a second application.
            check.set_active(false);
            set_identity(&spins);
        });
    }
    {
        let state = state.clone();
        let nb = main_notebook.downgrade();
        let spins = spins.clone();
        let check = check_general.clone();
        btn_reset.connect_clicked(move |_| {
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                if let Some(orig) = tab.original_structure.clone() {
                    tab.structure = Some(orig);
                    tab.interaction.selected.clear();
                    tab.invalidate_derived();
                    if let Some(s) = &tab.structure {
                        crate::utils::console::structure_changed("Reset to as-loaded cell", s);
                    }
                }
            }
            if let Some(nb) = nb.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
            check.set_active(false);
            set_identity(&spins);
        });
    }

    update();
    TabParts {
        controls,
        on_enter: Box::new(move || update()),
    }
}
