// src/menu/actions_analysis.rs

use crate::state::AppState;
use crate::ui::analysis::window::{show_analysis_window, show_charge_density_window};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup(app: &Application, window: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    // --- Analysis Tools (Symmetry, XRD, Band Path, Voids, Slab) ---
    let action = gtk4::gio::SimpleAction::new("analysis", None);
    let win_weak = window.downgrade();
    let state_c = state.clone();

    action.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            show_analysis_window(&win, state_c.clone());
        }
    });
    app.add_action(&action);

    // --- Charge Density — opens its own dedicated window ---
    let chgcar_action = gtk4::gio::SimpleAction::new("open_chgcar", None);
    let win_weak2 = window.downgrade();

    chgcar_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak2.upgrade() {
            show_charge_density_window(&win);
        }
    });
    app.add_action(&chgcar_action);
}
