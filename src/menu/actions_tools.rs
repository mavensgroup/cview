use crate::physics::operations::conversion::{convert_structure, CellType};
use crate::state::AppState;
use crate::ui::dialogs::{miller_dlg, supercell_dlg};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup(
  app: &Application,
  window: &ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: &DrawingArea,
) {
  // --- SUPERCELL ---
  let sc_action = gtk4::gio::SimpleAction::new("supercell", None);
  let win_weak = window.downgrade();
  let state_weak = Rc::downgrade(&state);
  let da_weak = drawing_area.downgrade();

  sc_action.connect_activate(move |_, _| {
    if let Some(win) = win_weak.upgrade() {
      if let Some(st) = state_weak.upgrade() {
        if let Some(da) = da_weak.upgrade() {
          supercell_dlg::show(&win, st, &da);
        }
      }
    }
  });
  app.add_action(&sc_action);

  // --- MILLER ---
  let mil_action = gtk4::gio::SimpleAction::new("miller_planes", None);
  let win_weak = window.downgrade();
  let state_weak = Rc::downgrade(&state);
  let da_weak = drawing_area.downgrade();

  mil_action.connect_activate(move |_, _| {
    if let Some(win) = win_weak.upgrade() {
      if let Some(st) = state_weak.upgrade() {
        if let Some(da) = da_weak.upgrade() {
          miller_dlg::show(&win, st, &da);
        }
      }
    }
  });
  app.add_action(&mil_action);

  // --- TOGGLE CELL VIEW (Stateless Action) ---
  // We use SimpleAction::new (no state), so no radio/check UI appears.
  let toggle_action = gtk4::gio::SimpleAction::new("toggle_cell_view", None);

  let st_weak_t = Rc::downgrade(&state);
  let da_weak_t = drawing_area.downgrade();

  toggle_action.connect_activate(move |_, _| {
    // We don't need 'action' arg anymore
    if let Some(st) = st_weak_t.upgrade() {
      // 1. Determine the new target type based on YOUR internal state
      // We borrow state to check what the current mode is.
      let target_type = {
        let state_borrow = st.borrow();

        // Assuming you have a boolean or enum in State to track this.
        // If you don't, add `pub is_primitive: bool` to your State struct!
        if state_borrow.config.load_conventional {
          CellType::Conventional
        } else {
          CellType::Primitive
        }
      };

      // 2. Update the internal state tracker (so we know for next time)
      // We do this in a separate scope or before conversion to satisfy borrow checker
      {
        let mut state_mut = st.borrow_mut();
        state_mut.config.load_conventional = !state_mut.config.load_conventional;
      }

      // 3. Convert and Update UI
      if let Some(da) = da_weak_t.upgrade() {
        convert_and_update(&st, &da, target_type);
      }
    }
  });

  app.add_action(&toggle_action);
}

fn convert_and_update(state: &Rc<RefCell<AppState>>, da: &DrawingArea, cell_type: CellType) {
  let mut st = state.borrow_mut();

  // Always convert from 'original_structure' to ensure toggle works consistently
  // If 'original_structure' is missing, fallback to 'structure'
  let source = st
    .original_structure
    .as_ref()
    .or(st.structure.as_ref())
    .cloned();

  if let Some(structure) = source {
    match convert_structure(&structure, cell_type) {
      Ok(new_struct) => {
        let view_name = match cell_type {
          CellType::Primitive => "Primitive",
          CellType::Conventional => "Conventional",
        };
        println!(
          "Switched to {} View. Formula: {}",
          view_name, new_struct.formula
        );

        st.structure = Some(new_struct);
        da.queue_draw();
      }
      Err(e) => eprintln!("Conversion error: {}", e),
    }
  }
}
