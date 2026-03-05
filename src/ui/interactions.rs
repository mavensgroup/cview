// src/ui/interactions.rs

use crate::rendering::scene;
use crate::state::AppState;
use crate::utils::{console, report};
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
) {
    // 1. KEYBOARD CONTROLLER
    let key_controller = EventControllerKey::new();
    let s = state.clone();
    let da = drawing_area.clone();

    key_controller.connect_key_pressed(move |_, keyval, _keycode, state_flags| {
        let mut st = s.borrow_mut();

        // A. Shift Key
        if keyval == gdk::Key::Shift_L || keyval == gdk::Key::Shift_R {
            st.active_tab_mut().interaction.is_shift_pressed = true;
            return glib::Propagation::Proceed;
        }

        // B. Delete
        if keyval == gdk::Key::Delete {
            let msg = st.delete_selected();
            console::info(&msg);
            da.queue_draw();
            return glib::Propagation::Stop;
        }

        // C. Undo (Ctrl+Z)
        if state_flags.contains(gdk::ModifierType::CONTROL_MASK) && keyval == gdk::Key::z {
            let msg = st.undo();
            console::info(&msg);
            da.queue_draw();
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });

    let s = state.clone();
    key_controller.connect_key_released(move |_, keyval, _, _| {
        if keyval == gdk::Key::Shift_L || keyval == gdk::Key::Shift_R {
            s.borrow_mut().active_tab_mut().interaction.is_shift_pressed = false;
        }
    });
    window.add_controller(key_controller);

    // 2. MOUSE DRAG
    let drag = GestureDrag::new();
    let s = state.clone();
    drag.connect_drag_begin(move |_, x, y| {
        let mut st = s.borrow_mut();
        let tab = st.active_tab_mut();
        if tab.interaction.is_shift_pressed {
            tab.interaction.selection_box = Some(((x, y), (x, y)));
        }
    });

    let s = state.clone();
    let da = drawing_area.clone();
    drag.connect_drag_update(move |_, x, y| {
        let mut st = s.borrow_mut();
        let tab = st.active_tab_mut();

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

    drag.connect_drag_end(move |_, x, y| {
        let mut st = s.borrow_mut();

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

                let (atoms, _, _) =
                    scene::calculate_scene(st.active_tab(), &st.config, w, h, false, None, None);

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
                    console::info(&format!("Box selected {} atoms.", count));
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
        let tab = st.active_tab_mut();
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

    click.connect_pressed(move |gesture, _n_press, x, y| {
        let mut st = s.borrow_mut();

        if st.active_tab().interaction.is_shift_pressed {
            return;
        }

        let widget = gesture.widget();
        let w = widget.width() as f64;
        let h = widget.height() as f64;

        let (atoms, _, _) =
            scene::calculate_scene(st.active_tab(), &st.config, w, h, false, None, None);

        let mut sorted_atoms: Vec<_> = atoms.iter().collect();
        sorted_atoms.sort_by(|a, b| {
            b.screen_pos[2]
                .partial_cmp(&a.screen_pos[2])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut clicked_original_index: Option<usize> = None;
        for atom in &sorted_atoms {
            let dx = atom.screen_pos[0] - x;
            let dy = atom.screen_pos[1] - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= atom.screen_radius + 4.0 {
                clicked_original_index = Some(atom.original_index);
                break;
            }
        }

        if let Some(original_idx) = clicked_original_index {
            st.toggle_selection(original_idx);

            let tab = st.active_tab();
            if let Some(structure) = &tab.structure {
                let mut selected_atoms: Vec<(usize, String, [f64; 3])> = Vec::new();
                for &sel_idx in &tab.interaction.selected_indices {
                    if let Some(atom) = structure.atoms.get(sel_idx) {
                        selected_atoms.push((sel_idx, atom.element.clone(), atom.position));
                    }
                }
                selected_atoms.sort_by_key(|a| a.0);
                let text = report::geometry_analysis_from_positions(&selected_atoms);
                console::info(&text);
            }

            da.queue_draw();
        }
    });

    drawing_area.add_controller(click);
}
