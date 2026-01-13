use crate::config::RotationCenter;
use crate::state::AppState;
use crate::ui::show_preferences_window;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

pub fn setup(
  app: &Application,
  window: &ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: &DrawingArea,
) {
  // 1. Restore View (Reset)
  let act_reset = gtk4::gio::SimpleAction::new("view_reset", None);
  let s_reset = state.clone();
  let da_reset = drawing_area.clone();
  act_reset.connect_activate(move |_, _| {
    let mut st = s_reset.borrow_mut();
    st.view.rot_x = 0.0;
    st.view.rot_y = 0.0;
    st.view.zoom = 1.0;
    da_reset.queue_draw();
  });
  app.add_action(&act_reset);

  // 2. View Along Axes
  // Along A (Look down X) -> Rotate Y by -90 deg
  let act_a = gtk4::gio::SimpleAction::new("view_along_a", None);
  let s_a = state.clone();
  let da_a = drawing_area.clone();
  act_a.connect_activate(move |_, _| {
    let mut st = s_a.borrow_mut();
    st.view.rot_x = 0.0;
    st.view.rot_y = -PI / 2.0;
    da_a.queue_draw();
  });
  app.add_action(&act_a);

  // Along B (Look down Y) -> Rotate X by 90 deg
  let act_b = gtk4::gio::SimpleAction::new("view_along_b", None);
  let s_b = state.clone();
  let da_b = drawing_area.clone();
  act_b.connect_activate(move |_, _| {
    let mut st = s_b.borrow_mut();
    st.view.rot_x = PI / 2.0;
    st.view.rot_y = 0.0;
    da_b.queue_draw();
  });
  app.add_action(&act_b);

  // Along C (Look down Z) -> Reset rotations
  let act_c = gtk4::gio::SimpleAction::new("view_along_c", None);
  let s_c = state.clone();
  let da_c = drawing_area.clone();
  act_c.connect_activate(move |_, _| {
    let mut st = s_c.borrow_mut();
    st.view.rot_x = 0.0;
    st.view.rot_y = 0.0;
    da_c.queue_draw();
  });
  app.add_action(&act_c);

  // 3. Rotation Center Modes
  let act_centroid = gtk4::gio::SimpleAction::new("center_centroid", None);
  let s_cent = state.clone();
  let da_cent = drawing_area.clone();
  act_centroid.connect_activate(move |_, _| {
    s_cent.borrow_mut().config.rotation_mode = RotationCenter::Centroid;
    da_cent.queue_draw();
  });
  app.add_action(&act_centroid);

  let act_uc = gtk4::gio::SimpleAction::new("center_unitcell", None);
  let s_uc = state.clone();
  let da_uc = drawing_area.clone();
  act_uc.connect_activate(move |_, _| {
    s_uc.borrow_mut().config.rotation_mode = RotationCenter::UnitCell;
    da_uc.queue_draw();
  });
  app.add_action(&act_uc);

  // 4. Toggle Bonds
  let act_bonds = gtk4::gio::SimpleAction::new("toggle_bonds", None);
  let s_bond = state.clone();
  let da_bond = drawing_area.clone();
  act_bonds.connect_activate(move |_, _| {
    let mut st = s_bond.borrow_mut();
    st.view.show_bonds = !st.view.show_bonds;
    da_bond.queue_draw();
  });
  app.add_action(&act_bonds);

  // 5. Preferences
  let act_pref = gtk4::gio::SimpleAction::new("preferences", None);
  let s_pref = state.clone();
  let da_pref = drawing_area.clone();
  // let da_pref = drawing_area.clone();
  let win_weak = window.downgrade();

  act_pref.connect_activate(move |_, _| {
    if let Some(win) = win_weak.upgrade() {
      show_preferences_window(&win, s_pref.clone(), da_pref.clone());
    }
  });
  app.add_action(&act_pref);
}
