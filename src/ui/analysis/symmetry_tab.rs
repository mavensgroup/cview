use crate::state::AppState;
use crate::utils::geometry;
use gtk4::prelude::*;
use gtk4::{Align, Box, Grid, Label, Orientation, PolicyType, ScrolledWindow, Separator};
use nalgebra::{Matrix3, Vector3};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

// Import the logic from Physics
use crate::physics::analysis::symmetry;

/// Helper: Vector Magnitude
fn mag(v: [f64; 3]) -> f64 {
    (v[0].powi(2) + v[1].powi(2) + v[2].powi(2)).sqrt()
}

/// Helper: Angle between two vectors (degrees)
fn angle(v1: [f64; 3], v2: [f64; 3]) -> f64 {
    let dot = v1[0] * v2[0] + v1[1] * v2[1] + v1[2] * v2[2];
    let m1 = mag(v1);
    let m2 = mag(v2);
    let val = (dot / (m1 * m2)).clamp(-1.0, 1.0);
    val.acos() * 180.0 / PI
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    // Root Container
    let root = Box::new(Orientation::Vertical, 10);

    // --- FIX: set_margin_all is not valid, use individual setters ---
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);
    // ----------------------------------------------------------------

    // ============================================================
    // TOP SECTION: Split into Left (Symmetry) and Right (Lattice)
    // ============================================================
    let top_box = Box::new(Orientation::Horizontal, 20);
    top_box.set_hexpand(true);
    root.append(&top_box);

    // --- LEFT: Symmetry Info ---
    let left_frame = Box::new(Orientation::Vertical, 5);
    left_frame.set_hexpand(true);

    let title_sym = Label::new(None);
    title_sym.set_markup("<span size='large' weight='bold'>Symmetry</span>");
    title_sym.set_halign(Align::Start);
    left_frame.append(&title_sym);

    let grid_sym = Grid::new();
    grid_sym.set_row_spacing(6);
    grid_sym.set_column_spacing(15);
    grid_sym.set_margin_top(5);

    let mk_row = |grid: &Grid, row: i32, txt: &str| -> Label {
        let lbl = Label::builder().label(txt).halign(Align::Start).build();
        lbl.set_markup(&format!("<b>{}</b>", txt));
        grid.attach(&lbl, 0, row, 1, 1);
        let val = Label::new(Some("-"));
        val.set_halign(Align::Start);
        grid.attach(&val, 1, row, 1, 1);
        val
    };

    let val_sg = mk_row(&grid_sym, 0, "Space Group:");
    let val_num = mk_row(&grid_sym, 1, "Number:");
    let val_sys = mk_row(&grid_sym, 2, "System:");
    val_sg.set_selectable(true);

    left_frame.append(&grid_sym);
    top_box.append(&left_frame);

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

    let mk_lat_row = |txt: &str, col: i32, row: i32| -> Label {
        let l = Label::builder().label(txt).halign(Align::Start).build();
        l.set_markup(&format!("<b>{}</b>", txt));
        grid_lat.attach(&l, col, row, 1, 1);
        let v = Label::new(Some("-"));
        v.set_halign(Align::Start);
        grid_lat.attach(&v, col + 1, row, 1, 1);
        v
    };

    let val_a = mk_lat_row("a:", 0, 0);
    let val_al = mk_lat_row("α:", 2, 0);
    let val_b = mk_lat_row("b:", 0, 1);
    let val_be = mk_lat_row("β:", 2, 1);
    let val_c = mk_lat_row("c:", 0, 2);
    let val_ga = mk_lat_row("γ:", 2, 2);

    let l_vol = Label::builder().label("Vol:").halign(Align::Start).build();
    l_vol.set_markup("<b>Vol:</b>");
    grid_lat.attach(&l_vol, 0, 3, 1, 1);
    let val_vol = Label::new(Some("-"));
    val_vol.set_halign(Align::Start);
    grid_lat.attach(&val_vol, 1, 3, 3, 1);

    right_frame.append(&grid_lat);
    top_box.append(&right_frame);

    root.append(&Separator::new(Orientation::Horizontal));

    // ============================================================
    // BOTTOM SECTION: Atomic Coordinates Table
    // ============================================================
    let title_coords = Label::new(None);
    title_coords.set_markup("<span size='large' weight='bold'>Atomic Coordinates</span>");
    title_coords.set_halign(Align::Start);
    root.append(&title_coords);

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(300)
        .vexpand(true)
        .build();

    let grid_atoms = Grid::new();
    grid_atoms.set_column_spacing(20);
    grid_atoms.set_row_spacing(8);

    // --- FIX: set_margin_all is not valid here either ---
    grid_atoms.set_margin_top(10);
    grid_atoms.set_margin_bottom(10);
    grid_atoms.set_margin_start(10);
    grid_atoms.set_margin_end(10);
    // ----------------------------------------------------

    let headers = [
        "El", "x (Å)", "y (Å)", "z (Å)", "x (frac)", "y (frac)", "z (frac)",
    ];
    for (i, h) in headers.iter().enumerate() {
        let l = Label::new(None);
        l.set_markup(&format!("<b>{}</b>", h));
        grid_atoms.attach(&l, i as i32, 0, 1, 1);
    }
    grid_atoms.attach(&Separator::new(Orientation::Horizontal), 0, 1, 7, 1);

    scrolled.set_child(Some(&grid_atoms));
    root.append(&scrolled);

    // ============================================================
    // LOGIC
    // ============================================================
    let st = state.borrow();
    if let Some(structure) = &st.active_tab().structure {
        let lat = structure.lattice;

        // 1. LATTICE DISPLAY
        let a_vec = lat[0];
        let b_vec = lat[1];
        let c_vec = lat[2];
        let origin = [0.0, 0.0, 0.0];

        // Use shared geometry utils
        let a = geometry::calculate_distance(origin, a_vec);
        let b = geometry::calculate_distance(origin, b_vec);
        let c = geometry::calculate_distance(origin, c_vec);

        let alpha = geometry::calculate_angle(b_vec, origin, c_vec);
        let beta = geometry::calculate_angle(a_vec, origin, c_vec);
        let gamma = geometry::calculate_angle(a_vec, origin, b_vec);

        let cx = b_vec[1] * c_vec[2] - b_vec[2] * c_vec[1];
        let cy = b_vec[2] * c_vec[0] - b_vec[0] * c_vec[2];
        let cz = b_vec[0] * c_vec[1] - b_vec[1] * c_vec[0];
        let vol = (a_vec[0] * cx + a_vec[1] * cy + a_vec[2] * cz).abs();

        val_a.set_text(&format!("{:.4}", a));
        val_b.set_text(&format!("{:.4}", b));
        val_c.set_text(&format!("{:.4}", c));
        val_al.set_text(&format!("{:.2}°", alpha));
        val_be.set_text(&format!("{:.2}°", beta));
        val_ga.set_text(&format!("{:.2}°", gamma));
        val_vol.set_text(&format!("{:.2} Å³", vol));

        // 2. COORDINATE DISPLAY
        let lattice_mat = Matrix3::new(
            lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
            lat[2][2],
        );

        if let Some(inv_mat) = lattice_mat.try_inverse() {
            for (i, atom) in structure.atoms.iter().enumerate() {
                let v_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                let v_frac = inv_mat.transpose() * v_cart;

                let row = (i as i32) + 2;
                grid_atoms.attach(&Label::new(Some(&atom.element)), 0, row, 1, 1);
                grid_atoms.attach(
                    &Label::new(Some(&format!("{:.4}", atom.position[0]))),
                    1,
                    row,
                    1,
                    1,
                );
                grid_atoms.attach(
                    &Label::new(Some(&format!("{:.4}", atom.position[1]))),
                    2,
                    row,
                    1,
                    1,
                );
                grid_atoms.attach(
                    &Label::new(Some(&format!("{:.4}", atom.position[2]))),
                    3,
                    row,
                    1,
                    1,
                );
                grid_atoms.attach(&Label::new(Some(&format!("{:.4}", v_frac.x))), 4, row, 1, 1);
                grid_atoms.attach(&Label::new(Some(&format!("{:.4}", v_frac.y))), 5, row, 1, 1);
                grid_atoms.attach(&Label::new(Some(&format!("{:.4}", v_frac.z))), 6, row, 1, 1);
            }
        }

        // 3. MOYO SYMMETRY (CALLING PHYSICS)
        match symmetry::analyze(structure) {
            Ok(info) => {
                val_num.set_text(&format!("{}", info.number));
                val_sg.set_text(&info.symbol);
                val_sys.set_text(&info.system);
            }
            Err(_) => {
                val_sg.set_text("Analysis Failed");
            }
        }
    } else {
        val_sg.set_text("No Data");
    }

    root
}
