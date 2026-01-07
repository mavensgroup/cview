use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, Grid, Align, Separator, ScrolledWindow, PolicyType};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::model::elements::get_atomic_number;
use std::f64::consts::PI;

// Spglib 1.15.1 & Math
use spglib::{cell::Cell, dataset::Dataset};
use nalgebra::{Matrix3, Vector3};

/// Helper: Vector Magnitude
fn mag(v: [f64; 3]) -> f64 {
    (v[0].powi(2) + v[1].powi(2) + v[2].powi(2)).sqrt()
}

/// Helper: Angle between two vectors (degrees)
fn angle(v1: [f64; 3], v2: [f64; 3]) -> f64 {
    let dot = v1[0]*v2[0] + v1[1]*v2[1] + v1[2]*v2[2];
    let m1 = mag(v1);
    let m2 = mag(v2);
    let val = (dot / (m1 * m2)).clamp(-1.0, 1.0);
    val.acos() * 180.0 / PI
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    // Root Container
    let root = Box::new(Orientation::Vertical, 10);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // ============================================================
    // TOP SECTION: Split into Left (Symmetry) and Right (Lattice)
    // ============================================================
    let top_box = Box::new(Orientation::Horizontal, 20);
    top_box.set_hexpand(true);
    root.append(&top_box);

    // --- LEFT: Symmetry Info ---
    let left_frame = Box::new(Orientation::Vertical, 5);
    left_frame.set_hexpand(true); // Take half width

    let title_sym = Label::new(None);
    title_sym.set_markup("<span size='large' weight='bold'>Symmetry</span>");
    title_sym.set_halign(Align::Start);
    left_frame.append(&title_sym);

    let grid_sym = Grid::new();
    grid_sym.set_row_spacing(6);
    grid_sym.set_column_spacing(15);
    grid_sym.set_margin_top(5);

    let lbl_sg = Label::builder().label("Space Group:").halign(Align::Start).build();
    let lbl_num = Label::builder().label("Number:").halign(Align::Start).build();
    let lbl_sys = Label::builder().label("System:").halign(Align::Start).build();

    lbl_sg.set_markup("<b>Space Group:</b>");
    lbl_num.set_markup("<b>Number:</b>");
    lbl_sys.set_markup("<b>System:</b>");

    let val_sg = Label::new(Some("-"));
    let val_num = Label::new(Some("-"));
    let val_sys = Label::new(Some("-"));

    for w in [&val_sg, &val_num, &val_sys] { w.set_halign(Align::Start); }

    grid_sym.attach(&lbl_sg, 0, 0, 1, 1);  grid_sym.attach(&val_sg, 1, 0, 1, 1);
    grid_sym.attach(&lbl_num, 0, 1, 1, 1); grid_sym.attach(&val_num, 1, 1, 1, 1);
    grid_sym.attach(&lbl_sys, 0, 2, 1, 1); grid_sym.attach(&val_sys, 1, 2, 1, 1);

    left_frame.append(&grid_sym);
    top_box.append(&left_frame);

    // Separator between Left/Right
    top_box.append(&Separator::new(Orientation::Vertical));

    // --- RIGHT: Lattice Parameters ---
    let right_frame = Box::new(Orientation::Vertical, 5);
    right_frame.set_hexpand(true);

    let title_lat = Label::new(None);
    title_lat.set_markup("<span size='large' weight='bold'>Lattice Parameters</span>");
    title_lat.set_halign(Align::Start);
    right_frame.append(&title_lat);

    let grid_lat = Grid::new();
    grid_lat.set_row_spacing(6);
    grid_lat.set_column_spacing(15);
    grid_lat.set_margin_top(5);

    // Labels for Lattice
    let labels_lat = [
        ("a:", 0, 0), ("α:", 2, 0),
        ("b:", 0, 1), ("β:", 2, 1),
        ("c:", 0, 2), ("γ:", 2, 2),
        ("Vol:", 0, 3)
    ];

    for (t, c, r) in labels_lat {
        let l = Label::builder().label(t).halign(Align::Start).build();
        l.set_markup(&format!("<b>{}</b>", t));
        grid_lat.attach(&l, c, r, 1, 1);
    }

    let val_a = Label::new(Some("-"));
    let val_b = Label::new(Some("-"));
    let val_c = Label::new(Some("-"));
    let val_al = Label::new(Some("-"));
    let val_be = Label::new(Some("-"));
    let val_ga = Label::new(Some("-"));
    let val_vol = Label::new(Some("-"));

    for w in [&val_a, &val_b, &val_c, &val_al, &val_be, &val_ga, &val_vol] {
        w.set_halign(Align::Start);
    }

    grid_lat.attach(&val_a, 1, 0, 1, 1); grid_lat.attach(&val_al, 3, 0, 1, 1);
    grid_lat.attach(&val_b, 1, 1, 1, 1); grid_lat.attach(&val_be, 3, 1, 1, 1);
    grid_lat.attach(&val_c, 1, 2, 1, 1); grid_lat.attach(&val_ga, 3, 2, 1, 1);
    grid_lat.attach(&val_vol, 1, 3, 3, 1);

    right_frame.append(&grid_lat);
    top_box.append(&right_frame);

    // Separator before bottom section
    root.append(&Separator::new(Orientation::Horizontal));

    // ============================================================
    // BOTTOM SECTION: Atomic Coordinates Table
    // ============================================================
    let title_coords = Label::new(None);
    title_coords.set_markup("<span size='large' weight='bold'>Atomic Coordinates</span>");
    title_coords.set_halign(Align::Start);
    root.append(&title_coords);

    // Scrolled Window for the list
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(300)
        .vexpand(true)
        .build();

    let grid_atoms = Grid::new();
    grid_atoms.set_column_spacing(20);
    grid_atoms.set_row_spacing(8);
    grid_atoms.set_margin_top(10);
    grid_atoms.set_margin_start(10);
    grid_atoms.set_margin_end(10);
    grid_atoms.set_margin_bottom(10);

    // Table Headers
    let headers = ["El", "x (Å)", "y (Å)", "z (Å)", "x (frac)", "y (frac)", "z (frac)"];
    for (i, h) in headers.iter().enumerate() {
        let l = Label::new(None);
        l.set_markup(&format!("<b>{}</b>", h));
        grid_atoms.attach(&l, i as i32, 0, 1, 1);
    }
    // Add separator line below headers
    let sep = Separator::new(Orientation::Horizontal);
    grid_atoms.attach(&sep, 0, 1, 7, 1);

    scrolled.set_child(Some(&grid_atoms));
    root.append(&scrolled);


    // ============================================================
    // LOGIC & CALCULATIONS
    // ============================================================
    let st = state.borrow();
    if let Some(structure) = &st.structure {
        let lat = structure.lattice;

        // 1. LATTICE PARAMS
        let a_vec = lat[0]; let b_vec = lat[1]; let c_vec = lat[2];
        let a = mag(a_vec); let b = mag(b_vec); let c = mag(c_vec);
        let alpha = angle(b_vec, c_vec);
        let beta  = angle(a_vec, c_vec);
        let gamma = angle(a_vec, b_vec);

        // Volume
        let cx = b_vec[1]*c_vec[2] - b_vec[2]*c_vec[1];
        let cy = b_vec[2]*c_vec[0] - b_vec[0]*c_vec[2];
        let cz = b_vec[0]*c_vec[1] - b_vec[1]*c_vec[0];
        let vol = (a_vec[0]*cx + a_vec[1]*cy + a_vec[2]*cz).abs();

        val_a.set_text(&format!("{:.4}", a));
        val_b.set_text(&format!("{:.4}", b));
        val_c.set_text(&format!("{:.4}", c));
        val_al.set_text(&format!("{:.2}°", alpha));
        val_be.set_text(&format!("{:.2}°", beta));
        val_ga.set_text(&format!("{:.2}°", gamma));
        val_vol.set_text(&format!("{:.2} Å³", vol));

        // 2. COORDINATES & SYMMETRY PREP
        let types: Vec<i32> = structure.atoms.iter()
            .map(|at| get_atomic_number(&at.element))
            .collect();

        // Calculate Fractional Matrix
        let mat = Matrix3::new(
            lat[0][0], lat[0][1], lat[0][2],
            lat[1][0], lat[1][1], lat[1][2],
            lat[2][0], lat[2][1], lat[2][2],
        );

        if let Some(inv_mat) = mat.try_inverse() {
            let mut positions: Vec<[f64; 3]> = Vec::new();

            // Populate Atoms Table
            for (i, atom) in structure.atoms.iter().enumerate() {
                let v = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                let frac = inv_mat.transpose() * v; // Row-major basis correction
                positions.push([frac.x, frac.y, frac.z]);

                // Cartesian Display
                let el_lbl = Label::new(Some(&atom.element));
                let x_c = Label::new(Some(&format!("{:.4}", atom.position[0])));
                let y_c = Label::new(Some(&format!("{:.4}", atom.position[1])));
                let z_c = Label::new(Some(&format!("{:.4}", atom.position[2])));

                // Fractional Display
                let x_f = Label::new(Some(&format!("{:.4}", frac.x)));
                let y_f = Label::new(Some(&format!("{:.4}", frac.y)));
                let z_f = Label::new(Some(&format!("{:.4}", frac.z)));

                let row = (i as i32) + 2; // +2 because row 0 is header, row 1 is separator
                grid_atoms.attach(&el_lbl, 0, row, 1, 1);
                grid_atoms.attach(&x_c, 1, row, 1, 1);
                grid_atoms.attach(&y_c, 2, row, 1, 1);
                grid_atoms.attach(&z_c, 3, row, 1, 1);
                grid_atoms.attach(&x_f, 4, row, 1, 1);
                grid_atoms.attach(&y_f, 5, row, 1, 1);
                grid_atoms.attach(&z_f, 6, row, 1, 1);
            }

            // 3. SYMMETRY CALCULATION
            let mut cell = Cell::new(&lat, &positions, &types);
            let dataset = Dataset::new(&mut cell, 1e-3);

            if dataset.spacegroup_number > 0 {
                val_sg.set_text(&dataset.international_symbol);
                val_num.set_text(&format!("{}", dataset.spacegroup_number));

                let sys_name = match dataset.spacegroup_number {
                    1..=2 => "Triclinic",
                    3..=15 => "Monoclinic",
                    16..=74 => "Orthorhombic",
                    75..=142 => "Tetragonal",
                    143..=167 => "Trigonal",
                    168..=194 => "Hexagonal",
                    195..=230 => "Cubic",
                    _ => "Unknown"
                };
                val_sys.set_text(sys_name);
            } else {
                val_sg.set_text("Unknown");
            }
        }
    } else {
        val_sg.set_text("No Data");
    }

    root
}
