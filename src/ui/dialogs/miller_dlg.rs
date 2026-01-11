use gtk4::prelude::*;
use gtk4::{Dialog, Grid, Label, SpinButton, ResponseType, Align, Window, DrawingArea};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::model::miller::MillerPlane;

// CHANGE: Accept 'impl IsA<Window>'
pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, drawing_area: &DrawingArea) {
    let dialog = Dialog::builder()
        .title("Add Miller Plane")
        .transient_for(parent)
        .modal(true)
        .default_width(300)
        .build();

    let content = dialog.content_area();
    // CHANGE: Specific margins
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);

    let grid = Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);
    grid.set_halign(Align::Center);

    let h = SpinButton::with_range(-10.0, 10.0, 1.0); h.set_value(1.0);
    let k = SpinButton::with_range(-10.0, 10.0, 1.0); k.set_value(0.0);
    let l = SpinButton::with_range(-10.0, 10.0, 1.0); l.set_value(0.0);

    grid.attach(&Label::new(Some("h:")), 0, 0, 1, 1); grid.attach(&h, 1, 0, 1, 1);
    grid.attach(&Label::new(Some("k:")), 0, 1, 1, 1); grid.attach(&k, 1, 1, 1, 1);
    grid.attach(&Label::new(Some("l:")), 0, 2, 1, 1); grid.attach(&l, 1, 2, 1, 1);

    content.append(&grid);

    dialog.add_button("Clear All", ResponseType::Reject);
    dialog.add_button("Cancel", ResponseType::Cancel);
    dialog.add_button("Add", ResponseType::Ok);

    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    dialog.connect_response(move |d, resp| {
        if let Some(st) = state_weak.upgrade() {
            let mut s = st.borrow_mut();
            if resp == ResponseType::Ok {
                s.miller_planes.push(MillerPlane::new(h.value() as i32, k.value() as i32, l.value() as i32, 1.0));
            } else if resp == ResponseType::Reject {
                s.miller_planes.clear();
            }
        }
        if let Some(da) = da_weak.upgrade() { da.queue_draw(); }
        d.close();
    });

    dialog.show();
}
