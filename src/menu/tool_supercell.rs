use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, ResponseType, Align};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    let action = gtk4::gio::SimpleAction::new("supercell", None);

    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    action.connect_activate(move |_, _| {
        let win = match win_weak.upgrade() { Some(w) => w, None => return };

        let dialog = gtk4::Dialog::builder()
            .title("Supercell Generator")
            .transient_for(&win)
            .modal(true)
            .default_width(320)
            .build();

        let content_area = dialog.content_area();
        content_area.set_margin_top(20);
        content_area.set_margin_bottom(20);
        content_area.set_margin_start(20);
        content_area.set_margin_end(20);

        let grid = gtk4::Grid::new();
        grid.set_row_spacing(15);
        grid.set_column_spacing(15);
        grid.set_halign(Align::Center);

        // Helper for SpinButtons
        let create_row = |label_text: &str, row: i32| -> gtk4::SpinButton {
            let label = gtk4::Label::new(Some(label_text));
            label.set_halign(Align::End);
            grid.attach(&label, 0, row, 1, 1);
            let spin = gtk4::SpinButton::with_range(1.0, 50.0, 1.0);
            spin.set_value(1.0);
            grid.attach(&spin, 1, row, 1, 1);
            spin
        };

        let spin_x = create_row("X Repeat:", 0);
        let spin_y = create_row("Y Repeat:", 1);
        let spin_z = create_row("Z Repeat:", 2);

        content_area.append(&grid);

        // --- BUTTONS ---
        // "Reject" ID will act as our "Reset" button
        dialog.add_button("Reset to Unit Cell", ResponseType::Reject);
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Generate", ResponseType::Ok);

        let state_inner = state_weak.clone();
        let da_inner = da_weak.clone();

        dialog.connect_response(move |d, response| {
            if let Some(st) = state_inner.upgrade() {
                let mut s = st.borrow_mut();

                if response == ResponseType::Ok {
                    // --- GENERATE SUPERCELL ---
                    let nx = spin_x.value() as u32;
                    let ny = spin_y.value() as u32;
                    let nz = spin_z.value() as u32;

                    // Always generate from the ORIGINAL structure to avoid compounding errors
                    // (e.g. 2x2 of a 2x2 making a 4x4)
                    if let Some(orig) = &s.original_structure {
                        println!("Generating {}x{}x{} supercell...", nx, ny, nz);
                        let supercell = orig.make_supercell(nx, ny, nz);

                        s.structure = Some(supercell);
                        s.selected_indices.clear(); // Clear selection as indices changed
                    }
                }
                else if response == ResponseType::Reject {
                    // --- RESET ---
                    println!("Resetting to original unit cell.");
                    if let Some(orig) = &s.original_structure {
                        s.structure = Some(orig.clone());
                        s.selected_indices.clear();
                    }
                }

                // Redraw
                if let Some(da) = da_inner.upgrade() {
                    da.queue_draw();
                }
            }
            d.close();
        });

        dialog.show();
    });

    app.add_action(&action);
}
