// src/menu/actions_trajectory.rs
//
// Structure → Trajectory Player: opens the GPU player on the frames of the
// active tab's file. Enabled only when that tab came from a multi-frame file.

use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Notebook};
use std::cell::RefCell;
use std::rc::Rc;

const ACTION: &str = "trajectory_player";

pub fn setup(app: &Application, window: &ApplicationWindow, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    let act = gtk4::gio::SimpleAction::new(ACTION, None);
    let win = window.downgrade();
    let nb = notebook.downgrade();
    let st = Rc::downgrade(&state);
    act.connect_activate(move |_, _| {
        let (Some(win), Some(nb), Some(st)) = (win.upgrade(), nb.upgrade(), st.upgrade()) else {
            return;
        };
        crate::ui::trajectory_player::show(&win, st, &nb);
    });
    act.set_enabled(false);
    app.add_action(&act);
}

/// Enable the menu entry when the active tab has frames to play. Call after
/// loading a file and on tab switches.
pub fn refresh_enabled(state: &AppState) {
    let has = state
        .tabs
        .get(state.active_tab_index)
        .is_some_and(|t| t.trajectory.is_some());
    if let Some(act) = gtk4::gio::Application::default()
        .and_then(|app| app.lookup_action(ACTION))
        .and_then(|a| a.downcast::<gtk4::gio::SimpleAction>().ok())
    {
        act.set_enabled(has);
    }
}
