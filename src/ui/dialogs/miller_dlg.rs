// src/ui/dialogs/miller_dlg.rs

use crate::model::miller::MillerPlane;
use crate::state::AppState;
use gtk4::prelude::*;
// Changed DrawingArea to Notebook
use gtk4::{Align, Dialog, Grid, Label, Notebook, ResponseType, SpinButton, Window};
use std::cell::RefCell;
use std::rc::Rc;

// Signature updated: accepts &Notebook
pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
  let dialog = Dialog::builder()
    .title("Add Miller Plane")
    .transient_for(parent)
    .modal(true)
    .default_width(300)
    .build();

  let content = dialog.content_area();
  content.set_margin_top(20);
  content.set_margin_bottom(20);
  content.set_margin_start(20);
  content.set_margin_end(20);

  let grid = Grid::new();
  grid.set_column_spacing(10);
  grid.set_row_spacing(10);
  grid.set_halign(Align::Center);

  let h = SpinButton::with_range(-10.0, 10.0, 1.0);
  h.set_value(1.0);
  let k = SpinButton::with_range(-10.0, 10.0, 1.0);
  k.set_value(0.0);
  let l = SpinButton::with_range(-10.0, 10.0, 1.0);
  l.set_value(0.0);

  grid.attach(&Label::new(Some("h:")), 0, 0, 1, 1);
  grid.attach(&h, 1, 0, 1, 1);
  grid.attach(&Label::new(Some("k:")), 0, 1, 1, 1);
  grid.attach(&k, 1, 1, 1, 1);
  grid.attach(&Label::new(Some("l:")), 0, 2, 1, 1);
  grid.attach(&l, 1, 2, 1, 1);

  content.append(&grid);

  dialog.add_button("Clear All", ResponseType::Reject);
  dialog.add_button("Cancel", ResponseType::Cancel);
  dialog.add_button("Add", ResponseType::Ok);

  let state_weak = Rc::downgrade(&state);
  let nb_weak = notebook.downgrade(); // Capture Notebook weakly

  dialog.connect_response(move |d, resp| {
    // 1. Update State
    if let Some(st) = state_weak.upgrade() {
      let mut s = st.borrow_mut();

      // FIX: Access the active tab mutably
      let tab = s.active_tab_mut();

      if resp == ResponseType::Ok {
        tab.miller_planes.push(MillerPlane::new(
          h.value() as i32,
          k.value() as i32,
          l.value() as i32,
          1.0,
        ));
      } else if resp == ResponseType::Reject {
        tab.miller_planes.clear();
      }
    }

    // 2. Redraw currently visible Tab
    // This ensures the plane appears on the tab you are looking at
    if let Some(nb) = nb_weak.upgrade() {
      if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
        da.queue_draw();
      }
    }

    d.close();
  });

  dialog.show();
}
