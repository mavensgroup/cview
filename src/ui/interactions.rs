// src/ui/interactions.rs

use crate::rendering::scene;
use crate::state::AppState;
use crate::utils::report;
use gtk4::gdk;
use gtk4::glib;
use gtk4::{self as gtk, prelude::*};
use gtk4::{
    ApplicationWindow, EventControllerKey, EventControllerScroll, EventControllerScrollFlags,
    GestureClick, GestureDrag, PropagationPhase,
};
use std::cell::RefCell;
use std::rc::Rc;

// Helper to append text to the TextView
fn append_to_console(view: &gtk::TextView, text: &str) {
    let buffer = view.buffer();
    let mut end_iter = buffer.end_iter();
    buffer.insert(&mut end_iter, &format!("{}\n", text));

    // Auto-scroll to bottom
    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
    view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}

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
            st.active_tab_mut().interaction.is_shift_pressed = true; // FIX: active_tab_mut
            return glib::Propagation::Proceed;
        }

        // B. Delete
        if keyval == gdk::Key::Delete {
            // delete_selected() is a helper on AppState that handles the active tab internally
            let msg = st.delete_selected();
            append_to_console(&console, &msg);
            da.queue_draw();
            return glib::Propagation::Stop;
        }

        // C. Undo (Ctrl+Z)
        if state_flags.contains(gdk::ModifierType::CONTROL_MASK) && keyval == gdk::Key::z {
            // undo() is a helper on AppState that handles the active tab internally
            let msg = st.undo();
            append_to_console(&console, &msg);
            da.queue_draw();
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });

    let s = state.clone();
    key_controller.connect_key_released(move |_, keyval, _, _| {
        if keyval == gdk::Key::Shift_L || keyval == gdk::Key::Shift_R {
            s.borrow_mut().active_tab_mut().interaction.is_shift_pressed = false;
            // FIX: active_tab_mut
        }
    });
    window.add_controller(key_controller);

    // 2. MOUSE DRAG
    let drag = GestureDrag::new();
    let s = state.clone();
    drag.connect_drag_begin(move |_, x, y| {
        let mut st = s.borrow_mut();
        let tab = st.active_tab_mut(); // FIX
        if tab.interaction.is_shift_pressed {
            tab.interaction.selection_box = Some(((x, y), (x, y)));
        }
    });

    let s = state.clone();
    let da = drawing_area.clone();
    drag.connect_drag_update(move |_, x, y| {
        let mut st = s.borrow_mut();
        let tab = st.active_tab_mut(); // FIX

        if tab.interaction.is_shift_pressed {
            if let Some((start, _)) = tab.interaction.selection_box {
                let current_x = start.0 + x;
                let current_y = start.1 + y;
                tab.interaction.selection_box = Some((start, (current_x, current_y)));
                da.queue_draw();
            }
        } else {
            tab.view.rot_y += x * 0.01;
            tab.view.rot_x += y * 0.01;
            da.queue_draw();
        }
    });

    let s = state.clone();
    let da = drawing_area.clone();
    let console = console_view.clone();

    drag.connect_drag_end(move |_, x, y| {
        let mut st = s.borrow_mut();

        // We need to check shift status first
        let is_shift = st.active_tab().interaction.is_shift_pressed;

        if is_shift {
            let tab = st.active_tab_mut();
            if let Some((start, _)) = tab.interaction.selection_box {
                let end_x = start.0 + x;
                let end_y = start.1 + y;
                let min_x = start.0.min(end_x);
                let max_x = start.0.max(end_x);
                let min_y = start.1.min(end_y);
                let max_y = start.1.max(end_y);

                let w = da.width() as f64;
                let h = da.height() as f64;

                // Temporarily drop mutable borrow to call calculate_scene (immutable)
                // or just pass references correctly if possible.
                // We need 'tab' and '&config'.
                // Since calculate_scene takes &TabState, we can re-borrow:

                // 1. Calculate Scene
                let (atoms, _, _) = scene::calculate_scene(
                    st.active_tab(),
                    &st.config, // FIX: Pass config
                    w,
                    h,
                    false,
                    None,
                    None,
                );

                // 2. Update Selection (Mutable)
                let tab_mut = st.active_tab_mut();
                let mut count = 0;
                for atom in atoms {
                    let ax = atom.screen_pos[0];
                    let ay = atom.screen_pos[1];

                    if ax >= min_x && ax <= max_x && ay >= min_y && ay <= max_y {
                        tab_mut
                            .interaction
                            .selected_indices
                            .insert(atom.original_index);
                        count += 1;
                    }
                }

                if count > 0 {
                    append_to_console(&console, &format!("Box selected {} atoms.", count));
                }
            }
            st.active_tab_mut().interaction.selection_box = None;
            da.queue_draw();
        }
    });

    drawing_area.add_controller(drag);

    // 3. SCROLL (ZOOM)
    let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
    let s = state.clone();
    let da = drawing_area.clone();
    scroll.connect_scroll(move |_, _, dy| {
        let mut st = s.borrow_mut();
        let tab = st.active_tab_mut(); // FIX
        if dy > 0.0 {
            tab.view.zoom *= 0.9;
        } else {
            tab.view.zoom *= 1.1;
        }
        da.queue_draw();
        glib::Propagation::Stop
    });
    drawing_area.add_controller(scroll);

    // 4. CLICK (SELECTION)
    let click = GestureClick::new();
    click.set_button(0);
    click.set_propagation_phase(PropagationPhase::Capture);

    let s = state.clone();
    let da = drawing_area.clone();
    let console = console_view.clone();

    click.connect_pressed(move |gesture, _n_press, x, y| {
        let mut st = s.borrow_mut();

        if st.active_tab().interaction.is_shift_pressed {
            // FIX
            return;
        }

        let widget = gesture.widget();
        let w = widget.width() as f64;
        let h = widget.height() as f64;

        let (atoms, _, _) = scene::calculate_scene(
            st.active_tab(),
            &st.config, // FIX
            w,
            h,
            false,
            None,
            None,
        );

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
            // toggle_selection is a helper on AppState
            st.toggle_selection(idx);

            // Re-borrow active tab for reporting
            let tab = st.active_tab();
            if let Some(structure) = &tab.structure {
                let text = report::geometry_analysis(structure, &tab.interaction.selected_indices);
                append_to_console(&console, &text);
            }

            da.queue_draw();
        }
    });

    drawing_area.add_controller(click);
}
