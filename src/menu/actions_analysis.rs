use crate::state::AppState;
use crate::ui::analysis::window::show_analysis_window;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use std::cell::RefCell;
use std::rc::Rc;

pub fn setup(app: &Application, window: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    let action = gtk4::gio::SimpleAction::new("analysis", None);
    let win_weak = window.downgrade();

    action.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            show_analysis_window(&win, state.clone());
        }
    });
    app.add_action(&action);
}
