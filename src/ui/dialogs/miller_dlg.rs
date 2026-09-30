// src/ui/dialogs/miller_dlg.rs

use crate::model::miller::MillerPlane;
use crate::state::AppState;
use gtk4::prelude::*;
// Changed DrawingArea to Notebook
use crate::ui::style::{card_body, prefixed_field, surface};
use gtk4::{Dialog, Notebook, ResponseType, SpinButton, Window};
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

  let h = SpinButton::with_range(-10.0, 10.0, 1.0);
  h.set_value(1.0);
  let k = SpinButton::with_range(-10.0, 10.0, 1.0);
  k.set_value(0.0);
  let l = SpinButton::with_range(-10.0, 10.0, 1.0);
  l.set_value(0.0);

  // h, k, l on one row with the letter inside each field (see ui::style).
  let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
  row.set_homogeneous(true);
  row.append(&prefixed_field("h", &h));
  row.append(&prefixed_field("k", &k));
  row.append(&prefixed_field("l", &l));
  let body = card_body(8);
  body.append(&row);
  content.append(&surface(&body));

  dialog.add_button("Clear All", ResponseType::Reject);
  dialog.add_button("Cancel", ResponseType::Cancel);
  dialog
    .add_button("Add", ResponseType::Ok)
    .add_css_class("suggested-action");

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
