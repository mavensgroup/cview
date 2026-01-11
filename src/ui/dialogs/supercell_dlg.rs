use gtk4::prelude::*;
use gtk4::{Dialog, Grid, Label, SpinButton, ResponseType, Align, Window, DrawingArea};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::physics::operations::supercell;

// CHANGE: Accept 'impl IsA<Window>' instead of concrete '&Window'
pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, drawing_area: &DrawingArea) {
    let dialog = Dialog::builder()
        .title("Supercell Generator")
        .transient_for(parent) // This now works with ApplicationWindow
        .modal(true)
        .default_width(320)
        .build();

    let content = dialog.content_area();
    // CHANGE: Replace set_margin_all with specific margins
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);

    let grid = Grid::new();
    grid.set_row_spacing(10);
    grid.set_column_spacing(10);
    grid.set_halign(Align::Center);

    let make_spin = |row: i32, txt: &str| -> SpinButton {
        grid.attach(&Label::new(Some(txt)), 0, row, 1, 1);
        let s = SpinButton::with_range(1.0, 50.0, 1.0);
        s.set_value(1.0);
        grid.attach(&s, 1, row, 1, 1);
        s
    };

    let sx = make_spin(0, "X:");
    let sy = make_spin(1, "Y:");
    let sz = make_spin(2, "Z:");

    content.append(&grid);

    dialog.add_button("Reset", ResponseType::Reject);
    dialog.add_button("Cancel", ResponseType::Cancel);
    dialog.add_button("Generate", ResponseType::Ok);

    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    dialog.connect_response(move |d, resp| {
        if let Some(st) = state_weak.upgrade() {
            let mut s = st.borrow_mut();
            if resp == ResponseType::Ok {
                if let Some(orig) = &s.original_structure {
                    let new_s = supercell::generate(orig, sx.value() as u32, sy.value() as u32, sz.value() as u32);
                    s.structure = Some(new_s);
                    s.selected_indices.clear();
                    println!("Supercell generated.");
                }
            } else if resp == ResponseType::Reject {
                if let Some(orig) = &s.original_structure {
                    s.structure = Some(orig.clone());
                    s.selected_indices.clear();
                }
            }
        }
        if let Some(da) = da_weak.upgrade() { da.queue_draw(); }
        d.close();
    });

    dialog.show();
}
