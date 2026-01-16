// src/ui/dialogs/supercell_dlg.rs

use crate::physics::operations::supercell;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Align, Dialog, Grid, Label, Notebook, ResponseType, SpinButton, Window};
use std::cell::RefCell;
use std::rc::Rc;

// CHANGE: Now accepts 'notebook' instead of 'drawing_area'
pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
  let dialog = Dialog::builder()
    .title("Supercell Generator")
    .transient_for(parent)
    .modal(true)
    .default_width(320)
    .build();

  let content = dialog.content_area();
  content.set_margin_top(20);
  content.set_margin_bottom(20);
  content.set_margin_start(20);
  content.set_margin_end(20);

  // --- Layout ---
  let grid = Grid::new();
  grid.set_row_spacing(10);
  grid.set_column_spacing(10);
  grid.set_halign(Align::Center);

  // Helper to create SpinButtons
  let make_spin = |row: i32, txt: &str| -> SpinButton {
    grid.attach(&Label::new(Some(txt)), 0, row, 1, 1);
    let s = SpinButton::with_range(1.0, 50.0, 1.0);
    s.set_value(1.0);
    grid.attach(&s, 1, row, 1, 1);
    s
  };

  let sx = make_spin(0, "X Multiplier:");
  let sy = make_spin(1, "Y Multiplier:");
  let sz = make_spin(2, "Z Multiplier:");

  content.append(&grid);

  // --- Buttons ---
  dialog.add_button("Reset to Original", ResponseType::Reject);
  dialog.add_button("Cancel", ResponseType::Cancel);
  dialog.add_button("Generate", ResponseType::Ok);

  // --- Signal Handling ---
  let state_weak = Rc::downgrade(&state);
  let notebook_weak = notebook.downgrade(); // Capture the notebook

  dialog.connect_response(move |d, resp| {
    if let Some(st) = state_weak.upgrade() {
      let mut s = st.borrow_mut();
      let tab = s.active_tab_mut(); // Modify active tab data

      match resp {
        ResponseType::Ok => {
          // GENERATE
          if let Some(orig) = &tab.original_structure {
            let nx = sx.value() as u32;
            let ny = sy.value() as u32;
            let nz = sz.value() as u32;

            let new_s = supercell::generate(orig, nx, ny, nz);
            tab.structure = Some(new_s);
            tab.interaction.selected_indices.clear();

            println!("Supercell generated: {}x{}x{}", nx, ny, nz);

            // FIX: Use helper to find and update the ACTIVE drawing area
            if let Some(nb) = notebook_weak.upgrade() {
              if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                da.queue_draw();
              }
            }
          }
        }
        ResponseType::Reject => {
          // RESET
          if let Some(orig) = &tab.original_structure {
            tab.structure = Some(orig.clone());
            tab.interaction.selected_indices.clear();
            println!("Structure reset to original.");

            // FIX: Refresh ACTIVE drawing area
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
    d.close();
  });

  dialog.show();
}
