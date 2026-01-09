use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, Grid, Align, Separator, ScrolledWindow, PolicyType};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::model::elements::get_atomic_number;
use std::f64::consts::PI;

// MOYO Imports
use moyo::base::{Cell, Lattice, AngleTolerance};
use moyo::MoyoDataset;
use moyo::data::Setting;
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
    left_frame.set_hexpand(true);

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

    val_sg.set_selectable(true);

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
    grid_atoms.set_margin_top(10);
    grid_atoms.set_margin_start(10);
    grid_atoms.set_margin_end(10);
    grid_atoms.set_margin_bottom(10);

    let headers = ["El", "x (Å)", "y (Å)", "z (Å)", "x (frac)", "y (frac)", "z (frac)"];
    for (i, h) in headers.iter().enumerate() {
        let l = Label::new(None);
        l.set_markup(&format!("<b>{}</b>", h));
        grid_atoms.attach(&l, i as i32, 0, 1, 1);
    }
    let sep = Separator::new(Orientation::Horizontal);
    grid_atoms.attach(&sep, 0, 1, 7, 1);

    scrolled.set_child(Some(&grid_atoms));
    root.append(&scrolled);

    // ============================================================
    // LOGIC
    // ============================================================
    let st = state.borrow();
    if let Some(structure) = &st.structure {
        let lat = structure.lattice;

        // 1. LATTICE DISPLAY
        let a_vec = lat[0]; let b_vec = lat[1]; let c_vec = lat[2];
        let a = mag(a_vec); let b = mag(b_vec); let c = mag(c_vec);
        let alpha = angle(b_vec, c_vec);
        let beta  = angle(a_vec, c_vec);
        let gamma = angle(a_vec, b_vec);

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

        // 2. COORDINATE CONVERSION (Cartesian -> Fractional)
        let mut numbers = Vec::new();
        for at in &structure.atoms {
            let z = get_atomic_number(&at.element);
            numbers.push(if z == 0 { 1 } else { z as i32 });
        }

        let lattice_mat = Matrix3::new(
            lat[0][0], lat[0][1], lat[0][2],
            lat[1][0], lat[1][1], lat[1][2],
            lat[2][0], lat[2][1], lat[2][2],
        );

        if let Some(inv_mat) = lattice_mat.try_inverse() {
            let mut positions: Vec<Vector3<f64>> = Vec::new();

            for (i, atom) in structure.atoms.iter().enumerate() {
                let v_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                let v_frac = inv_mat.transpose() * v_cart;
                positions.push(v_frac);

                let el_lbl = Label::new(Some(&atom.element));
                let x_c = Label::new(Some(&format!("{:.4}", atom.position[0])));
                let y_c = Label::new(Some(&format!("{:.4}", atom.position[1])));
                let z_c = Label::new(Some(&format!("{:.4}", atom.position[2])));
                let x_f = Label::new(Some(&format!("{:.4}", v_frac.x)));
                let y_f = Label::new(Some(&format!("{:.4}", v_frac.y)));
                let z_f = Label::new(Some(&format!("{:.4}", v_frac.z)));

                let row = (i as i32) + 2;
                grid_atoms.attach(&el_lbl, 0, row, 1, 1);
                grid_atoms.attach(&x_c, 1, row, 1, 1);
                grid_atoms.attach(&y_c, 2, row, 1, 1);
                grid_atoms.attach(&z_c, 3, row, 1, 1);
                grid_atoms.attach(&x_f, 4, row, 1, 1);
                grid_atoms.attach(&y_f, 5, row, 1, 1);
                grid_atoms.attach(&z_f, 6, row, 1, 1);
            }

            // 3. MOYO SYMMETRY
            let cell = Cell::new(Lattice::new(lattice_mat), positions, numbers);

            match MoyoDataset::new(&cell, 1e-4, AngleTolerance::Default, Setting::Spglib, true) {
                Ok(dataset) => {
                    // Number
                    val_num.set_text(&format!("{}", dataset.number));

                    // Lookup Symbol
                    let symbol = if dataset.number >= 1 && dataset.number <= 230 {
                        SG_SYMBOLS[dataset.number as usize]
                    } else {
                        "Unknown"
                    };
                    val_sg.set_text(symbol);

                    // System
                    let sys_name = match dataset.number {
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
                },
                Err(_) => {
                    val_sg.set_text("Error");
                }
            }
        } else {
            val_sg.set_text("Invalid Lattice");
        }
    } else {
        val_sg.set_text("No Data");
    }

    root
}

// =========================================================================
// SPACE GROUP LOOKUP TABLE (1-230)
// Verified complete list (231 elements including index 0)
// =========================================================================

const SG_SYMBOLS: [&str; 231] = ["","P1", "P-1", "P121", "P12_11", "C121", "P1m1", "P1c1", "C1m1", "C1c1", "P12/m1", "P12_1/m1", "C12/m1", "P12/c1", "P12_1/c1", "C12/c1", "P222", "P222_1", "P2_12_12", "P2_12_12_1", "C222_1", "C222", "F222", "I222", "I2_12_12_1", "Pmm2", "Pmc2_1", "Pcc2", "Pma2", "Pca2_1", "Pnc2", "Pmn2_1", "Pba2", "Pna2_1", "Pnn2", "Cmm2", "Cmc2_1", "Ccc2", "Amm2", "Aem2", "Ama2", "Aea2", "Fmm2", "Fdd2", "Imm2", "Iba2", "Ima2", "Pmmm", "Pnnn", "Pccm", "Pban", "Pmma", "Pnna", "Pmna", "Pcca", "Pbam", "Pccn", "Pbcm", "Pnnm", "Pmmn", "Pbcn", "Pbca", "Pnma", "Cmcm", "Cmce", "Cmmm", "Cccm", "Cmme", "Ccce", "Fmmm", "Fddd", "Immm", "Ibam", "Ibca", "Imma", "P4", "P4_1", "P4_2", "P4_3", "I4", "I4_1", "P-4", "I-4", "P4/m", "P4_2/m", "P4/n", "P4_2/n", "I4/m", "I4_1/a", "P422", "P42_12", "P4_122", "P4_12_12", "P4_222", "P4_22_12", "P4_322", "P4_32_12", "I422", "I4_122", "P4mm", "P4bm", "P4_2cm", "P4_2nm", "P4cc", "P4nc", "P4_2mc", "P4_2bc", "I4mm", "I4cm", "I4_1md", "I4_1cd", "P-42m", "P-42c", "P-42_1m", "P-42_1c", "P-4m2", "P-4c2", "P-4b2", "P-4n2", "I-4m2", "I-4c2", "I-42m", "I-42d", "P4/mmm", "P4/mcc", "P4/nbm", "P4/nnc", "P4/mbm", "P4/mnc", "P4/nmm", "P4/ncc", "P4_2/mmc", "P4_2/mcm", "P4_2/nbc", "P4_2/nnm", "P4_2/mbc", "P4_2/mnm", "P4_2/nmc", "P4_2/ncm", "I4/mmm", "I4/mcm", "I4_1/amd", "I4_1/acd", "P3", "P3_1", "P3_2", "R3", "P-3", "R-3", "P312", "P321", "P3_112", "P3_121", "P3_212", "P3_221", "R32", "P3m1", "P31m", "P3c1", "P31c", "R3m", "R3c", "P-31m", "P-31c", "P-3m1", "P-3c1", "R-3m", "R-3c", "P6", "P6_1", "P6_5", "P6_2", "P6_4", "P6_3", "P-6", "P6/m", "P6_3/m", "P622", "P6_122", "P6_522", "P6_222", "P6_422", "P6_322", "P6mm", "P6cc", "P6_3cm", "P6_3mc", "P-6m2", "P-6c2", "P-62m", "P-62c", "P6/mmm", "P6/mcc", "P6_3/mcm", "P6_3/mmc", "P23", "F23", "I23", "P2_13", "I2_13", "Pm-3", "Pn-3", "Fm-3", "Fd-3", "Im-3", "Pa-3", "Ia-3", "P432", "P4_232", "F432", "F4_132", "I432", "P4_332", "P4_132", "I4_132", "P-43m", "F-43m", "I-43m", "P-43n", "F-43c", "I-43d", "Pm-3m", "Pn-3n", "Pm-3n", "Pn-3m", "Fm-3m", "Fm-3c", "Fd-3m", "Fd-3c", "Im-3m", "Ia-3d"
];
