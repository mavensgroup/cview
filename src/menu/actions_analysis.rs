// src/menu/actions_analysis.rs

use crate::state::AppState;
use crate::ui::analysis::window::{
    show_analysis_window, show_charge_density_window, AnalysisTab,
};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Notebook};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,
) {
    // --- One entry per Analysis tab; each opens the window on that tab ---
    for (name, tab) in [
        ("analysis_symmetry", AnalysisTab::Symmetry),
        ("analysis_xrd", AnalysisTab::Xrd),
        ("analysis_kpath", AnalysisTab::BandPath),
        ("analysis_voids", AnalysisTab::Voids),
    ] {
        let action = gtk4::gio::SimpleAction::new(name, None);
        let win_weak = window.downgrade();
        let state_c = state.clone();
        let nb_weak = notebook.downgrade();

        action.connect_activate(move |_, _| {
            if let (Some(win), Some(nb)) = (win_weak.upgrade(), nb_weak.upgrade()) {
                show_analysis_window(&win, state_c.clone(), &nb, tab);
            }
        });
        app.add_action(&action);
    }

    // --- Charge Density — opens its own dedicated window ---
    let chgcar_action = gtk4::gio::SimpleAction::new("open_chgcar", None);
    let win_weak2 = window.downgrade();
    let state_c2 = state.clone();

    chgcar_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak2.upgrade() {
            show_charge_density_window(&win, state_c2.clone());
        }
    });
    app.add_action(&chgcar_action);
}
