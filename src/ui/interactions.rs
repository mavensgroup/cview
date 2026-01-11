// src/ui/interactions.rs

use crate::rendering::scene;
use crate::state::AppState;
use gtk4::gdk;
use gtk4::glib;
use gtk4::{self as gtk, prelude::*};
use gtk4::{
  ApplicationWindow, EventControllerKey, EventControllerScroll, EventControllerScrollFlags,
  GestureClick, GestureDrag, PropagationPhase,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup_interactions(
  window: &ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: &gtk::DrawingArea,
  console_view: &gtk::TextView,
) {
  // 1. KEYBOARD CONTROLLER
  let key_controller = EventControllerKey::new();
  let s = state.clone();
  let da = drawing_area.clone();
  let console = console_view.clone();

  key_controller.connect_key_pressed(move |_, keyval, _keycode, state_flags| {
    let mut st = s.borrow_mut();

    // A. Shift Key
    if keyval == gdk::Key::Shift_L || keyval == gdk::Key::Shift_R {
      st.is_shift_pressed = true;
      return glib::Propagation::Proceed;
    }

    // B. Delete
    if keyval == gdk::Key::Delete {
      let msg = st.delete_selected();
      console.buffer().set_text(&msg);
      da.queue_draw();
      return glib::Propagation::Stop;
    }

    // C. Undo (Ctrl+Z)
    if state_flags.contains(gdk::ModifierType::CONTROL_MASK) && keyval == gdk::Key::z {
      let msg = st.undo();
      console.buffer().set_text(&msg);
      da.queue_draw();
      return glib::Propagation::Stop;
    }

    glib::Propagation::Proceed
  });

  let s = state.clone();
  key_controller.connect_key_released(move |_, keyval, _, _| {
    if keyval == gdk::Key::Shift_L || keyval == gdk::Key::Shift_R {
      s.borrow_mut().is_shift_pressed = false;
    }
  });

  window.add_controller(key_controller);

  // 2. MOUSE DRAG
  let drag = GestureDrag::new();
  let s = state.clone();

  // REMOVED UNUSED 'da' CLONE HERE

  drag.connect_drag_begin(move |_, x, y| {
    let mut st = s.borrow_mut();
    if st.is_shift_pressed {
      st.selection_box = Some(((x, y), (x, y)));
    }
  });

  let s = state.clone();
  let da = drawing_area.clone();
  drag.connect_drag_update(move |_, x, y| {
    let mut st = s.borrow_mut();
    if st.is_shift_pressed {
      if let Some((start, _)) = st.selection_box {
        let current_x = start.0 + x;
        let current_y = start.1 + y;
        st.selection_box = Some((start, (current_x, current_y)));
        da.queue_draw();
      }
    } else {
      st.rot_y += x * 0.01;
      st.rot_x += y * 0.01;
      da.queue_draw();
    }
  });

  let s = state.clone();
  let da = drawing_area.clone();
  let console = console_view.clone();
  drag.connect_drag_end(move |_, x, y| {
    let mut st = s.borrow_mut();
    if st.is_shift_pressed {
      if let Some((start, _)) = st.selection_box {
        let end_x = start.0 + x;
        let end_y = start.1 + y;
        let min_x = start.0.min(end_x);
        let max_x = start.0.max(end_x);
        let min_y = start.1.min(end_y);
        let max_y = start.1.max(end_y);

        let w = da.width() as f64;
        let h = da.height() as f64;
        let (atoms, _, _) = scene::calculate_scene(&st, w, h, false, None, None);

        let mut count = 0;
        for atom in atoms {
          let ax = atom.screen_pos[0];
          let ay = atom.screen_pos[1];
          if ax >= min_x && ax <= max_x && ay >= min_y && ay <= max_y {
            st.selected_indices.insert(atom.original_index);
            count += 1;
          }
        }

        if count > 0 {
          console
            .buffer()
            .set_text(&format!("Box selected {} atoms.", count));
        }
      }
      st.selection_box = None;
      da.queue_draw();
    }
  });

  drawing_area.add_controller(drag);

  // 3. SCROLL
  let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
  let s = state.clone();
  let da = drawing_area.clone();
  scroll.connect_scroll(move |_, _, dy| {
    let mut st = s.borrow_mut();
    if dy > 0.0 {
      st.zoom *= 0.9;
    } else {
      st.zoom *= 1.1;
    }
    da.queue_draw();
    glib::Propagation::Stop
  });
  drawing_area.add_controller(scroll);

  // 4. CLICK
  let click = GestureClick::new();
  click.set_button(0);
  click.set_propagation_phase(PropagationPhase::Capture);
  let s = state.clone();
  let da = drawing_area.clone();
  let console = console_view.clone();

  click.connect_pressed(move |gesture, _n_press, x, y| {
    let mut st = s.borrow_mut();
    if st.is_shift_pressed {
      return;
    }

    let widget = gesture.widget();
    let w = widget.width() as f64;
    let h = widget.height() as f64;
    let (atoms, _, _) = scene::calculate_scene(&st, w, h, false, None, None);

    let mut clicked_index = None;
    let mut min_dist = 40.0;
    for atom in &atoms {
      let dx = atom.screen_pos[0] - x;
      let dy = atom.screen_pos[1] - y;
      let dist = (dx * dx + dy * dy).sqrt();
      if dist < min_dist && dist < 30.0 {
        min_dist = dist;
        clicked_index = Some(atom.original_index);
      }
    }

    if let Some(idx) = clicked_index {
      st.toggle_selection(idx);
      let report = st.get_geometry_report();
      console.buffer().set_text(&report);
    } else {
      if !st.selected_indices.is_empty() {
        st.selected_indices.clear();
        console.buffer().set_text("Selection cleared.");
      }
    }
    da.queue_draw();
  });

  drawing_area.add_controller(click);
}
