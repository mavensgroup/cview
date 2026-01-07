use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, ResponseType, Align};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::model::miller::MillerPlane; // <--- ADDED IMPORT

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    // ============================================================
    // 1. SUPERCELL ACTION
    // ============================================================
    let supercell_action = gtk4::gio::SimpleAction::new("supercell", None);

    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();

    supercell_action.connect_activate(move |_, _| {
        let win = match win_weak.upgrade() { Some(w) => w, None => return };

        let dialog = gtk4::Dialog::builder()
            .title("Generate Supercell")
            .transient_for(&win)
            .modal(true)
            .default_width(300)
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

        // -- X Input --
        let label_x = gtk4::Label::new(Some("X Repeat:"));
        label_x.set_halign(Align::End);
        grid.attach(&label_x, 0, 0, 1, 1);
        let spin_x = gtk4::SpinButton::with_range(1.0, 50.0, 1.0);
        spin_x.set_value(1.0);
        grid.attach(&spin_x, 1, 0, 1, 1);

        // -- Y Input --
        let label_y = gtk4::Label::new(Some("Y Repeat:"));
        label_y.set_halign(Align::End);
        grid.attach(&label_y, 0, 1, 1, 1);
        let spin_y = gtk4::SpinButton::with_range(1.0, 50.0, 1.0);
        spin_y.set_value(1.0);
        grid.attach(&spin_y, 1, 1, 1, 1);

        // -- Z Input --
        let label_z = gtk4::Label::new(Some("Z Repeat:"));
        label_z.set_halign(Align::End);
        grid.attach(&label_z, 0, 2, 1, 1);
        let spin_z = gtk4::SpinButton::with_range(1.0, 50.0, 1.0);
        spin_z.set_value(1.0);
        grid.attach(&spin_z, 1, 2, 1, 1);

        content_area.append(&grid);

        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Generate", ResponseType::Ok);

        let state_inner = state_weak.clone();
        let da_inner = da_weak.clone();

        dialog.connect_response(move |d, response| {
            if response == ResponseType::Ok {
                let nx = spin_x.value() as u32;
                let ny = spin_y.value() as u32;
                let nz = spin_z.value() as u32;

                if let Some(st) = state_inner.upgrade() {
                    let mut s = st.borrow_mut();
                    if let Some(structure) = &s.structure {
                        let report = format!("Generating {}x{}x{} supercell...", nx, ny, nz);
                        println!("{}", report); // Keep generic logging

                        // Use our new logger if you want, or just log to terminal
                        // crate::ui::log_to_console(...);

                        let new_structure = structure.make_supercell(nx, ny, nz);
                        s.structure = Some(new_structure);
                        s.selected_indices.clear();

                        if let Some(da) = da_inner.upgrade() {
                            da.queue_draw();
                        }
                    }
                }
            }
            d.close();
        });

        dialog.show();
    });
    app.add_action(&supercell_action);


    // ============================================================
    // 2. MILLER INDICES ACTION
    // ============================================================
    let miller_action = gtk4::gio::SimpleAction::new("miller_planes", None);

    let win_weak_m = window.downgrade();
    let state_weak_m = Rc::downgrade(&state);
    let da_weak_m = drawing_area.downgrade();

    miller_action.connect_activate(move |_, _| {
        let win = match win_weak_m.upgrade() { Some(w) => w, None => return };

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
        let h_spin = gtk4::SpinButton::with_range(-10.0, 10.0, 1.0);
        h_spin.set_value(1.0);
        let k_spin = gtk4::SpinButton::with_range(-10.0, 10.0, 1.0);
        k_spin.set_value(1.0);
        let l_spin = gtk4::SpinButton::with_range(-10.0, 10.0, 1.0);
        l_spin.set_value(0.0); // Default (1 1 0)

        grid.attach(&gtk4::Label::new(Some("h:")), 0, 0, 1, 1);
        grid.attach(&h_spin, 1, 0, 1, 1);

        grid.attach(&gtk4::Label::new(Some("k:")), 0, 1, 1, 1);
        grid.attach(&k_spin, 1, 1, 1, 1);

        grid.attach(&gtk4::Label::new(Some("l:")), 0, 2, 1, 1);
        grid.attach(&l_spin, 1, 2, 1, 1);

        content.append(&grid);

        // Buttons
        dialog.add_button("Clear All Planes", ResponseType::Reject);
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Add", ResponseType::Ok);

        let state_inner = state_weak_m.clone();
        let da_inner = da_weak_m.clone();

        dialog.connect_response(move |d, resp| {
            if let Some(st) = state_inner.upgrade() {
                let mut s = st.borrow_mut();

                if resp == ResponseType::Ok {
                    let h = h_spin.value() as i32;
                    let k = k_spin.value() as i32;
                    let l = l_spin.value() as i32;

                    // Add new plane
                    s.miller_planes.push(MillerPlane::new(h, k, l, 1.0));
                    println!("Added Plane ({}, {}, {})", h, k, l);
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
    app.add_action(&miller_action);
}
