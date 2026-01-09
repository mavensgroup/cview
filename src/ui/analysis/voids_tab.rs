use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, Button, SpinButton, Align, Grid};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::physics::voids;

// Common ions for intercalation (Symbol, Ionic Radius in Angstroms)
const IONS: [(&str, f64); 8] = [
    ("Li⁺", 0.76),
    ("Mg²⁺", 0.72),
    ("Zn²⁺", 0.74),
    ("Na⁺", 1.02),
    ("Ca²⁺", 1.00),
    ("K⁺", 1.38),
    ("O²⁻", 1.40),
    ("Al³⁺", 0.54),
];

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    let root = Box::new(Orientation::Vertical, 10);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // --- Title ---
    let title = Label::new(Some("Void Space & Intercalation Analysis"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    root.append(&title);

    let desc = Label::new(Some("Calculates the largest spherical pore (cavity) diameter and suggests possible intercalating ions."));
    desc.set_wrap(true);
    desc.set_halign(Align::Start);
    root.append(&desc);

    // --- Controls ---
    let grid_box = Box::new(Orientation::Horizontal, 10);
    grid_box.append(&Label::new(Some("Grid Resolution (Å):")));

    let spin_res = SpinButton::with_range(0.1, 1.0, 0.1);
    spin_res.set_value(0.25);
    grid_box.append(&spin_res);

    let btn_calc = Button::with_label("Calculate");
    btn_calc.add_css_class("suggested-action");
    grid_box.append(&btn_calc);

    root.append(&grid_box);

    // --- Results Area ---
    let res_grid = Grid::new();
    res_grid.set_column_spacing(20);
    res_grid.set_row_spacing(10);
    res_grid.set_margin_top(20);

    // Labels
    let lbl_r = Label::builder().label("Max Pore Radius:").halign(Align::Start).build();
    let val_r = Label::builder().label("-").halign(Align::Start).build();
    val_r.add_css_class("title-3");

    let lbl_d = Label::builder().label("Max Pore Diameter:").halign(Align::Start).build();
    let val_d = Label::builder().label("-").halign(Align::Start).build();

    let lbl_pos = Label::builder().label("Center (x,y,z):").halign(Align::Start).build();
    let val_pos = Label::builder().label("-").halign(Align::Start).build();

    let lbl_vol = Label::builder().label("Void Volume %:").halign(Align::Start).build();
    let val_vol = Label::builder().label("-").halign(Align::Start).build();

    // Candidates
    let lbl_cand = Label::builder().label("Intercalation Candidates:").halign(Align::Start).build();
    let val_cand = Label::builder().label("-").halign(Align::Start).build();
    val_cand.set_wrap(true);
    val_cand.set_max_width_chars(40);

    // Layout
    res_grid.attach(&lbl_r, 0, 0, 1, 1);   res_grid.attach(&val_r, 1, 0, 1, 1);
    res_grid.attach(&lbl_d, 0, 1, 1, 1);   res_grid.attach(&val_d, 1, 1, 1, 1);
    res_grid.attach(&lbl_pos, 0, 2, 1, 1); res_grid.attach(&val_pos, 1, 2, 1, 1);
    res_grid.attach(&lbl_vol, 0, 3, 1, 1); res_grid.attach(&val_vol, 1, 3, 1, 1);

    let sep = gtk4::Separator::new(Orientation::Horizontal);
    res_grid.attach(&sep, 0, 4, 2, 1);
    res_grid.attach(&lbl_cand, 0, 5, 1, 1); res_grid.attach(&val_cand, 1, 5, 1, 1);

    root.append(&res_grid);

    // --- Logic ---
    let state_clone = state.clone();
    btn_calc.connect_clicked(move |_| {
        let st = state_clone.borrow();
        if let Some(structure) = &st.structure {
            let res = spin_res.value();

            let result = voids::calculate_voids(structure, res);
            let r_max = result.max_sphere_radius;

            // Update Metrics
            if r_max > 0.0 {
                val_r.set_text(&format!("{:.3} Å", r_max));
                val_d.set_text(&format!("{:.3} Å", r_max * 2.0));
                val_pos.set_text(&format!("{:.3}, {:.3}, {:.3}",
                    result.max_sphere_center[0],
                    result.max_sphere_center[1],
                    result.max_sphere_center[2]
                ));
                val_vol.set_text(&format!("{:.2} %", result.void_fraction));

                // Candidates
                let mut fits = Vec::new();
                for (symbol, r_ion) in IONS.iter() {
                    // Check if ion fits in pore (Radius vs Radius)
                    if *r_ion <= r_max {
                        fits.push(*symbol);
                    }
                }

                if fits.is_empty() {
                    val_cand.set_markup("<span color='red'>None (Too Dense)</span>");
                } else {
                    let s = fits.join(", ");
                    val_cand.set_markup(&format!("<span color='white'>{}</span>", s));
                }
            } else {
                val_r.set_text("0.00 Å (Dense)");
                val_d.set_text("0.00 Å");
                val_pos.set_text("-");
                val_vol.set_text("0.0 %");
                val_cand.set_markup("<span color='orange'>Structure is fully dense</span>");
            }

        } else {
            val_r.set_text("No Structure");
        }
    });

    root
}
