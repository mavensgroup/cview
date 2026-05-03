// src/ui/interactions.rs

use crate::panels::sidebar::SidebarHandles;
use crate::rendering::scene;
use crate::state::{AppState, SelectedAtom};
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

/// Mouse-drag rotation sensitivity in degrees per pixel. Tuned to feel close to
/// VESTA — a swipe across the canvas gives a meaningful spin without overshooting.
const DRAG_ROTATE_DEG_PER_PX: f64 = 0.4;

pub fn setup_interactions(
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &gtk::DrawingArea,
    sidebar_handles: Rc<SidebarHandles>,
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
        tab.interaction.drag_prev_offset = (0.0, 0.0);
        if tab.interaction.is_shift_pressed {
            tab.interaction.selection_box = Some(((x, y), (x, y)));
        }
    });

    let s = state.clone();
    let da = drawing_area.clone();
    let handles_drag = sidebar_handles.clone();
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
            // GestureDrag delivers cumulative offset since drag-begin, so we
            // diff against the previous frame to get a per-frame delta and feed
            // that into the trackball. Without diffing, each frame would re-add
            // the entire offset and rotation would explode.
            let (prev_dx, prev_dy) = tab.interaction.drag_prev_offset;
            let dx = x - prev_dx;
            let dy = y - prev_dy;
            tab.interaction.drag_prev_offset = (x, y);

            tab.view
                .apply_screen_rotation_deg(dx * DRAG_ROTATE_DEG_PER_PX, dy * DRAG_ROTATE_DEG_PER_PX);
            handles_drag.sync_from_view(&st.active_tab().view);
            da.queue_draw();
        }
    });

    let s = state.clone();
    let da = drawing_area.clone();

    drag.connect_drag_end(move |_, x, y| {
        let mut st = s.borrow_mut();

        let is_shift = st.active_tab().interaction.is_shift_pressed;

        if is_shift {
            let selection_box = st.active_tab().interaction.selection_box;
            if let Some((start, _)) = selection_box {
                let end_x = start.0 + x;
                let end_y = start.1 + y;
                let min_x = start.0.min(end_x);
                let max_x = start.0.max(end_x);
                let min_y = start.1.min(end_y);
                let max_y = start.1.max(end_y);

                let w = da.width() as f64;
                let h = da.height() as f64;

                let show_ghosts = st.active_tab().view.show_full_unit_cell;
                let (atoms, _, _) =
                    scene::calculate_scene(st.active_tab(), &st.config, w, h, false, None, None);

                let tab_mut = st.active_tab_mut();
                let mut count = 0;
                for atom in atoms {
                    // Ghost shells used only for coordination math are never
                    // drawn, so they must not be selectable.
                    if atom.is_coord_only {
                        continue;
                    }
                    if atom.is_ghost && !show_ghosts {
                        continue;
                    }
                    let ax = atom.screen_pos[0];
                    let ay = atom.screen_pos[1];

                    if ax >= min_x && ax <= max_x && ay >= min_y && ay <= max_y {
                        use std::collections::hash_map::Entry;
                        if let Entry::Vacant(v) =
                            tab_mut.interaction.selected.entry(atom.unique_id)
                        {
                            v.insert(SelectedAtom {
                                unique_id: atom.unique_id,
                                original_index: atom.original_index,
                                cart_pos: atom.cart_pos,
                                element: atom.element.clone(),
                            });
                            count += 1;
                        }
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
    let handles_scroll = sidebar_handles.clone();
    scroll.connect_scroll(move |_, _, dy| {
        let mut st = s.borrow_mut();
        let tab = st.active_tab_mut();
        if dy > 0.0 {
            tab.view.zoom *= 0.9;
        } else {
            tab.view.zoom *= 1.1;
        }
        handles_scroll.sync_from_view(&st.active_tab().view);
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

        let show_ghosts = st.active_tab().view.show_full_unit_cell;

        let (atoms, _, _) =
            scene::calculate_scene(st.active_tab(), &st.config, w, h, false, None, None);

        let mut sorted_atoms: Vec<_> = atoms.iter().collect();
        sorted_atoms.sort_by(|a, b| {
            b.screen_pos[2]
                .partial_cmp(&a.screen_pos[2])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut clicked: Option<SelectedAtom> = None;
        for atom in &sorted_atoms {
            // Coordination-only ghosts are never drawn; never select them.
            if atom.is_coord_only {
                continue;
            }
            // Visible ghosts are only present on screen if "Show Full Unit Cell"
            // is on. When it's off, don't allow clicking through to a hidden one.
            if atom.is_ghost && !show_ghosts {
                continue;
            }
            let dx = atom.screen_pos[0] - x;
            let dy = atom.screen_pos[1] - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= atom.screen_radius + 4.0 {
                clicked = Some(SelectedAtom {
                    unique_id: atom.unique_id,
                    original_index: atom.original_index,
                    cart_pos: atom.cart_pos,
                    element: atom.element.clone(),
                });
                break;
            }
        }

        if let Some(sel) = clicked {
            st.toggle_selection(sel);

            let tab = st.active_tab();
            // Use the cart_pos captured at selection time — ghost copies have
            // positions distinct from structure.atoms[original_index].
            let mut selected_atoms: Vec<(usize, String, [f64; 3])> = tab
                .interaction
                .selected
                .values()
                .map(|s| (s.unique_id, s.element.clone(), s.cart_pos))
                .collect();
            selected_atoms.sort_by_key(|a| a.0);
            let text = report::geometry_analysis_from_positions(&selected_atoms);
            console::info(&text);

            da.queue_draw();
        }
    });

    drawing_area.add_controller(click);
}
