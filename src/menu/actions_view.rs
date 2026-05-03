// src/menu/actions_view.rs

use crate::config::RotationCenter;
use crate::panels::sidebar::SidebarHandles;
use crate::state::AppState;
use crate::ui::show_preferences_window;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, Notebook};
use std::cell::RefCell;
use std::rc::Rc;
// Note: std::f64::consts::PI is removed as we now use degrees (90.0) instead of radians (PI/2)

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,
    drawing_area: &DrawingArea,
    sidebar_handles: Rc<SidebarHandles>,
) {
    // Helper to get active DA from weak notebook ref
    let get_da = |nb_weak: &gtk4::glib::WeakRef<Notebook>| -> Option<DrawingArea> {
        if let Some(nb) = nb_weak.upgrade() {
            crate::ui::get_active_drawing_area(&nb)
        } else {
            None
        }
    };

    // 1. Restore View (Reset)
    let act_reset = gtk4::gio::SimpleAction::new("view_reset", None);
    let s_reset = state.clone();
    let nb_reset = notebook.downgrade();
    let h_reset = sidebar_handles.clone();

    act_reset.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_reset) {
            let mut st = s_reset.borrow_mut();
            let tab = st.active_tab_mut();
            tab.view.reset_rotation();
            tab.view.zoom = 1.0;
            h_reset.sync_from_view(&st.active_tab().view);
            da.queue_draw();
        }
    });
    app.add_action(&act_reset);

    // 2. View Along Axes
    // Along A -> Rotate Y by -90 degrees
    let act_a = gtk4::gio::SimpleAction::new("view_along_a", None);
    let s_a = state.clone();
    let nb_a = notebook.downgrade();
    let h_a = sidebar_handles.clone();

    act_a.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_a) {
            let mut st = s_a.borrow_mut();
            let tab = st.active_tab_mut();
            tab.view.set_euler_xyz_deg(0.0, -90.0, 0.0);
            h_a.sync_from_view(&st.active_tab().view);
            da.queue_draw();
        }
    });
    app.add_action(&act_a);

    // Along B -> Rotate X by 90 degrees
    let act_b = gtk4::gio::SimpleAction::new("view_along_b", None);
    let s_b = state.clone();
    let nb_b = notebook.downgrade();
    let h_b = sidebar_handles.clone();

    act_b.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_b) {
            let mut st = s_b.borrow_mut();
            let tab = st.active_tab_mut();
            tab.view.set_euler_xyz_deg(90.0, 0.0, 0.0);
            h_b.sync_from_view(&st.active_tab().view);
            da.queue_draw();
        }
    });
    app.add_action(&act_b);

    // Along C -> Reset Rotation (0, 0)
    let act_c = gtk4::gio::SimpleAction::new("view_along_c", None);
    let s_c = state.clone();
    let nb_c = notebook.downgrade();
    let h_c = sidebar_handles.clone();

    act_c.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_c) {
            let mut st = s_c.borrow_mut();
            let tab = st.active_tab_mut();
            tab.view.reset_rotation();
            h_c.sync_from_view(&st.active_tab().view);
            da.queue_draw();
        }
    });
    app.add_action(&act_c);

    // 3. Rotation Center Modes
    let act_centroid = gtk4::gio::SimpleAction::new("center_centroid", None);
    let s_cent = state.clone();
    let nb_cent = notebook.downgrade();

    act_centroid.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_cent) {
            s_cent.borrow_mut().config.rotation_mode = RotationCenter::Centroid;
            da.queue_draw();
        }
    });
    app.add_action(&act_centroid);

    let act_uc = gtk4::gio::SimpleAction::new("center_unitcell", None);
    let s_uc = state.clone();
    let nb_uc = notebook.downgrade();

    act_uc.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_uc) {
            s_uc.borrow_mut().config.rotation_mode = RotationCenter::UnitCell;
            da.queue_draw();
        }
    });
    app.add_action(&act_uc);

    // 4. Toggle Bonds
    let act_bonds = gtk4::gio::SimpleAction::new("toggle_bonds", None);
    let s_bond = state.clone();
    let nb_bond = notebook.downgrade();

    act_bonds.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_bond) {
            let mut st = s_bond.borrow_mut();
            let tab = st.active_tab_mut();
            tab.view.show_bonds = !tab.view.show_bonds;
            da.queue_draw();
        }
    });
    app.add_action(&act_bonds);

    // 5. Preferences
    let act_pref = gtk4::gio::SimpleAction::new("preferences", None);
    let s_pref = state.clone();
    let nb_pref = notebook.downgrade();
    let win_weak = window.downgrade();

    // Preserve the fallback logic
    let da_fallback = drawing_area.clone();

    act_pref.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            let da = get_da(&nb_pref).unwrap_or(da_fallback.clone());
            show_preferences_window(&win, s_pref.clone(), da);
        }
    });
    app.add_action(&act_pref);

    // 6. Toggle Full Unit Cell
    let act_boundary = gtk4::gio::SimpleAction::new("toggle_boundaries", None);
    let s_bound = state.clone();
    let nb_bound = notebook.downgrade();

    act_boundary.connect_activate(move |_, _| {
        if let Some(da) = get_da(&nb_bound) {
            let mut st = s_bound.borrow_mut();
            // Important: We access the index first to print it safely
            let idx = st.active_tab_index;
            let tab = st.active_tab_mut();

            tab.view.show_full_unit_cell = !tab.view.show_full_unit_cell;
            crate::utils::console::log_debug(&format!(
                "Tab {} Full Cell: {}",
                idx, tab.view.show_full_unit_cell
            ));

            da.queue_draw();
        }
    });
    app.add_action(&act_boundary);
}
