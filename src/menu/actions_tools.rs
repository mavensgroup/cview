use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::ui::dialogs::{supercell_dlg, miller_dlg};

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    // --- SUPERCELL ---
    let sc_action = gtk4::gio::SimpleAction::new("supercell", None);
    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    sc_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            if let Some(st) = state_weak.upgrade() {
                if let Some(da) = da_weak.upgrade() {
                    supercell_dlg::show(&win, st, &da);
                }
            }
        }
    });
    app.add_action(&sc_action);

    // --- MILLER ---
    let mil_action = gtk4::gio::SimpleAction::new("miller_planes", None);
    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    mil_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak.upgrade() {
            if let Some(st) = state_weak.upgrade() {
                if let Some(da) = da_weak.upgrade() {
                    miller_dlg::show(&win, st, &da);
                }
            }
        }
    });
    app.add_action(&mil_action);
}
