// src/ui/dialogs/supercell_dlg.rs

use crate::physics::operations::supercell;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Align, Dialog, DrawingArea, Grid, Label, ResponseType, SpinButton, Window};
use std::cell::RefCell;
use std::rc::Rc;

/// Shows the Supercell Generation Dialog
///
/// parent: The window that owns this dialog (usually the main ApplicationWindow)
/// state: Shared application state
/// drawing_area: The main 3D canvas (needed to trigger a redraw after update)
pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, drawing_area: &DrawingArea) {
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
  dialog.add_button("Reset to Original", ResponseType::Reject); // Maps to 'Reset'
  dialog.add_button("Cancel", ResponseType::Cancel);
  dialog.add_button("Generate", ResponseType::Ok);

  // --- Signal Handling ---
  let state_weak = Rc::downgrade(&state);

  // We need a weak reference to the drawing area to trigger the redraw inside the closure
  let da_weak = drawing_area.downgrade();

  dialog.connect_response(move |d, resp| {
    // Upgrade state reference
    if let Some(st) = state_weak.upgrade() {
      let mut s = st.borrow_mut();

      match resp {
        ResponseType::Ok => {
          // GENERATE SUPERCELL
          // We always generate from the 'original_structure' to avoid
          // exponential growth (applying 2x2 to an already 2x2 structure)
          if let Some(orig) = &s.original_structure {
            let nx = sx.value() as u32;
            let ny = sy.value() as u32;
            let nz = sz.value() as u32;

            let new_s = supercell::generate(orig, nx, ny, nz);

            s.structure = Some(new_s);
            s.selected_indices.clear(); // Clear selection as indices changed

            println!("Supercell generated: {}x{}x{}", nx, ny, nz);

            // FIX: Trigger immediate redraw
            if let Some(da) = da_weak.upgrade() {
              da.queue_draw();
            }
          } else {
            eprintln!("Error: No original structure found to generate supercell from.");
          }
        }
        ResponseType::Reject => {
          // RESET
          if let Some(orig) = &s.original_structure {
            s.structure = Some(orig.clone());
            s.selected_indices.clear();
            println!("Structure reset to original.");

            // Trigger redraw
            if let Some(da) = da_weak.upgrade() {
              da.queue_draw();
            }
          }
        }
        _ => {} // Cancel or Close
      }
    }
    d.close();
  });

  dialog.show();
}
