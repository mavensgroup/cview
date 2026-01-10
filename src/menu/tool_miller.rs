use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, ResponseType, Align};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::model::miller::MillerPlane;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    let action = gtk4::gio::SimpleAction::new("miller_planes", None);

    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    action.connect_activate(move |_, _| {
        let win = match win_weak.upgrade() { Some(w) => w, None => return };

        let dialog = gtk4::Dialog::builder()
            .title("Add Miller Plane")
            .transient_for(&win)
            .modal(true)
            .default_width(300)
            .build();

        let content = dialog.content_area();
        content.set_margin_top(20);
        content.set_margin_bottom(20);
        content.set_margin_start(20);
        content.set_margin_end(20);

        let grid = gtk4::Grid::new();
        grid.set_column_spacing(10);
        grid.set_row_spacing(10);
        grid.set_halign(Align::Center);

        // Inputs for H, K, L
        let h_spin = gtk4::SpinButton::with_range(-10.0, 10.0, 1.0); h_spin.set_value(1.0);
        let k_spin = gtk4::SpinButton::with_range(-10.0, 10.0, 1.0); k_spin.set_value(1.0);
        let l_spin = gtk4::SpinButton::with_range(-10.0, 10.0, 1.0); l_spin.set_value(0.0);

        grid.attach(&gtk4::Label::new(Some("h:")), 0, 0, 1, 1); grid.attach(&h_spin, 1, 0, 1, 1);
        grid.attach(&gtk4::Label::new(Some("k:")), 0, 1, 1, 1); grid.attach(&k_spin, 1, 1, 1, 1);
        grid.attach(&gtk4::Label::new(Some("l:")), 0, 2, 1, 1); grid.attach(&l_spin, 1, 2, 1, 1);

        content.append(&grid);

        dialog.add_button("Clear All Planes", ResponseType::Reject);
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Add", ResponseType::Ok);

        let state_inner = state_weak.clone();
        let da_inner = da_weak.clone();

        dialog.connect_response(move |d, resp| {
            if let Some(st) = state_inner.upgrade() {
                let mut s = st.borrow_mut();

                if resp == ResponseType::Ok {
                    let h = h_spin.value() as i32;
                    let k = k_spin.value() as i32;
                    let l = l_spin.value() as i32;

                    s.miller_planes.push(MillerPlane::new(h, k, l, 1.0));
                    println!("Added Miller Plane ({}, {}, {})", h, k, l);
                }
                else if resp == ResponseType::Reject {
                    s.miller_planes.clear();
                    println!("Cleared all planes");
                }

                if let Some(da) = da_inner.upgrade() { da.queue_draw(); }
            }
            d.close();
        });

        dialog.show();
    });

    app.add_action(&action);
}
