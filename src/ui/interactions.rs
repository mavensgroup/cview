use crate::rendering::scene;
use crate::state::AppState;
use gtk4::glib;
use gtk4::{self as gtk, prelude::*};
use gtk4::{
  ApplicationWindow, EventControllerScroll, EventControllerScrollFlags, GestureClick, GestureDrag,
  PropagationPhase,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup_interactions(
  _window: &ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: &gtk::DrawingArea,
  console_view: &gtk::TextView, // <--- CHANGED TYPE
) {
  // 1. Mouse Drag
  let drag = GestureDrag::new();
  let s = state.clone();
  let da = drawing_area.clone();
  drag.connect_drag_update(move |_, x, y| {
    let mut st = s.borrow_mut();
    st.rot_y += x * 0.01;
    st.rot_x += y * 0.01;
    da.queue_draw();
  });
  drawing_area.add_controller(drag);

  // 2. Scroll
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

  // 3. Click (Pressed - Legacy feel)
  let click = GestureClick::new();
  click.set_button(0);
  click.set_propagation_phase(PropagationPhase::Capture);

  let s = state.clone();
  let da = drawing_area.clone();
  let console = console_view.clone();

  click.connect_pressed(move |gesture, _n_press, x, y| {
    // If you want to strictly prevent the "double toggle", uncomment this:
    // if n_press != 1 { return; }

    // let widget = gesture.widget().expect("No widget");
    let widget = gesture.widget();
    let w = widget.width() as f64;
    let h = widget.height() as f64;

    let mut st = s.borrow_mut();
    // let (atoms, _, bounds) = scene::calculate_scene(&st, w, h, false, None, None);
    let (atoms, _, _bounds) = scene::calculate_scene(&st, w, h, false, None, None);

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
    } else {
      st.selected_indices.clear();
    }

    // --- UPDATE CONSOLE ---
    let report = st.get_geometry_report();
    // console.buffer().set_text(&report);
    crate::ui::log_to_console(&console, &report);

    da.queue_draw();
  });

  drawing_area.add_controller(click);
}
