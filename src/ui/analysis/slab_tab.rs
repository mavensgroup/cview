// src/ui/analysis/slab_tab.rs

use crate::model::structure::Structure;
use crate::physics::operations::miller_algo::MillerMath;
use crate::physics::operations::slab;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Box, Button, DrawingArea, Frame, Grid, Label, Orientation, SpinButton};
use nalgebra::{Matrix3, Vector3};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

struct VisState {
    h: f64,
    k: f64,
    l: f64,
    thickness: f64,
    structure: Option<Structure>,
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    // --- Layout Setup ---
    let root = Box::new(Orientation::Horizontal, 15);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // Left Pane: Visualization
    let left_pane = Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);
    let frame = Frame::new(Some("Cutting Plane Visualization"));

    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(400);
    drawing_area.set_content_height(400);
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);

    frame.set_child(Some(&drawing_area));
    left_pane.append(&frame);
    root.append(&left_pane);

    // Right Pane: Controls
    let right_pane = Box::new(Orientation::Vertical, 10);
    right_pane.set_width_request(250);

    let title = Label::new(Some("Slab Generator"));
    title.add_css_class("title-2");
    right_pane.append(&title);

    let grid = Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);

    grid.attach(&Label::new(Some("Miller Indices:")), 0, 0, 3, 1);
    let spin_h = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_h.set_value(1.0);
    let spin_k = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_k.set_value(1.0);
    let spin_l = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_l.set_value(0.0);

    let row_hkl = Box::new(Orientation::Horizontal, 5);
    row_hkl.append(&Label::new(Some("h")));
    row_hkl.append(&spin_h);
    grid.attach(&row_hkl, 0, 1, 1, 1);

    let row_k = Box::new(Orientation::Horizontal, 5);
    row_k.append(&Label::new(Some("k")));
    row_k.append(&spin_k);
    grid.attach(&row_k, 1, 1, 1, 1);

    let row_l = Box::new(Orientation::Horizontal, 5);
    row_l.append(&Label::new(Some("l")));
    row_l.append(&spin_l);
    grid.attach(&row_l, 2, 1, 1, 1);

    grid.attach(&Label::new(Some("Thickness:")), 0, 2, 2, 1);
    let spin_thick = SpinButton::with_range(1.0, 50.0, 1.0);
    spin_thick.set_value(1.0);
    grid.attach(&spin_thick, 2, 2, 1, 1);

    grid.attach(&Label::new(Some("Vacuum (Å):")), 0, 3, 2, 1);
    let spin_vac = SpinButton::with_range(0.0, 100.0, 1.0);
    spin_vac.set_value(10.0);
    grid.attach(&spin_vac, 2, 3, 1, 1);

    right_pane.append(&grid);

    // Buttons
    let btn_box = Box::new(Orientation::Vertical, 5);
    btn_box.set_margin_top(20);
    let btn_gen = Button::with_label("Generate Slab");
    btn_gen.add_css_class("suggested-action");
    let btn_undo = Button::with_label("Undo Last Cut");
    btn_undo.set_sensitive(false);
    btn_box.append(&btn_gen);
    btn_box.append(&btn_undo);
    right_pane.append(&btn_box);

    let lbl_status = Label::new(Some("Ready."));
    right_pane.append(&lbl_status);
    root.append(&right_pane);

    // ================= LOGIC =================

    let current_struct = state.borrow().active_tab().structure.clone();

    let vis_state = Rc::new(RefCell::new(VisState {
        h: 1.0,
        k: 1.0,
        l: 0.0,
        thickness: 1.0,
        structure: current_struct,
    }));

    let draw_state = vis_state.clone();

    drawing_area.set_draw_func(move |_, cr, width, height| {
        // 1. White Background
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().expect("Failed to paint background");

        let w = width as f64;
        let h_dim = height as f64;
        let scale = w.min(h_dim) / 3.5;
        let cx = w / 2.0;
        let cy = h_dim / 2.0;

        let st = draw_state.borrow();
        let (nh, nk, nl) = (st.h, st.k, st.l);

        // Standard Isometric Projection
        let yaw = PI / 4.0 + 0.5;
        let pitch = PI / 6.0;
        let project = |x: f64, y: f64, z: f64| -> (f64, f64) {
            let x1 = x * yaw.cos() - z * yaw.sin();
            let z1 = x * yaw.sin() + z * yaw.cos();
            let y2 = y * pitch.cos() - z1 * pitch.sin();
            (cx + x1 * scale, cy + y2 * scale)
        };

        // --- 2. Draw Atoms (Ghosted) ---
        if let Some(structure) = &st.structure {
            let lat = structure.lattice;
            let lat_mat = Matrix3::new(
                lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0],
                lat[2][1], lat[2][2],
            );

            if let Some(inv_lat) = lat_mat.try_inverse() {
                let to_frac = inv_lat.transpose();
                for atom in &structure.atoms {
                    let cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                    let frac = to_frac * cart;

                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                let nx = frac.x + dx as f64;
                                let ny = frac.y + dy as f64;
                                let nz = frac.z + dz as f64;
                                if nx >= -0.05
                                    && nx <= 1.05
                                    && ny >= -0.05
                                    && ny <= 1.05
                                    && nz >= -0.05
                                    && nz <= 1.05
                                {
                                    let bx = nx * 2.0 - 1.0;
                                    let by = ny * 2.0 - 1.0;
                                    let bz = nz * 2.0 - 1.0;
                                    let (px, py) = project(bx, by, bz);

                                    cr.new_path();
                                    cr.arc(px, py, 6.0, 0.0, 2.0 * PI);
                                    cr.set_source_rgba(0.0, 0.5, 0.5, 0.8);
                                    cr.fill_preserve().unwrap();
                                }
                            }
                        }
                    }
                }
            }
        }

        // --- 3. Draw Unit Cell Wireframe ---
        cr.set_line_width(1.5);
        cr.set_source_rgb(0.2, 0.2, 0.2);
        let corners = [
            (-1.0, -1.0, -1.0),
            (1.0, -1.0, -1.0),
            (1.0, 1.0, -1.0),
            (-1.0, 1.0, -1.0),
            (-1.0, -1.0, 1.0),
            (1.0, -1.0, 1.0),
            (1.0, 1.0, 1.0),
            (-1.0, 1.0, 1.0),
        ];
        let edges = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        for (start, end) in edges {
            let p1 = project(corners[start].0, corners[start].1, corners[start].2);
            let p2 = project(corners[end].0, corners[end].1, corners[end].2);
            cr.move_to(p1.0, p1.1);
            cr.line_to(p2.0, p2.1);
            cr.stroke().unwrap();
        }

        // --- 4. THE DIAMOND VISUALIZATION ---
        // Clean, single-plane cut. No finite thickness artifacts.

        let math = MillerMath::new(nh as i32, nk as i32, nl as i32);
        let poly_3d = math.get_intersection_polygon();

        if !poly_3d.is_empty() {
            // Project the 3D polygon points to 2D screen coords
            let p_draw: Vec<(f64, f64)> = poly_3d
                .iter()
                .map(|p| {
                    // Map 0..1 coords to -1..1 box coords
                    let bx = p[0] * 2.0 - 1.0;
                    let by = p[1] * 2.0 - 1.0;
                    let bz = p[2] * 2.0 - 1.0;
                    project(bx, by, bz)
                })
                .collect();

            // A. Draw Semi-Transparent Red Plane
            cr.set_source_rgba(1.0, 0.0, 0.0, 0.2);

            cr.move_to(p_draw[0].0, p_draw[0].1);
            for p in p_draw.iter().skip(1) {
                cr.line_to(p.0, p.1);
            }
            cr.close_path();
            cr.fill().unwrap();

            // B. Draw Solid Red Outline (For sharpness)
            cr.set_source_rgba(0.8, 0.0, 0.0, 0.8);
            cr.set_line_width(2.0);

            cr.move_to(p_draw[0].0, p_draw[0].1);
            for p in p_draw.iter().skip(1) {
                cr.line_to(p.0, p.1);
            }
            cr.close_path();
            cr.stroke().unwrap();
        }
    });

    // --- Signals (Logic remains the same) ---
    let da_clone = drawing_area.clone();
    let state_h = vis_state.clone();
    spin_h.connect_value_changed(move |s| {
        state_h.borrow_mut().h = s.value();
        da_clone.queue_draw();
    });

    let da_clone = drawing_area.clone();
    let state_k = vis_state.clone();
    spin_k.connect_value_changed(move |s| {
        state_k.borrow_mut().k = s.value();
        da_clone.queue_draw();
    });

    let da_clone = drawing_area.clone();
    let state_l = vis_state.clone();
    spin_l.connect_value_changed(move |s| {
        state_l.borrow_mut().l = s.value();
        da_clone.queue_draw();
    });

    // let da_clone = drawing_area.clone();
    let state_thick = vis_state.clone();
    spin_thick.connect_value_changed(move |s| {
        // We still update the state for the physics generation,
        // but we don't change the visualization (it stays a single plane).
        state_thick.borrow_mut().thickness = s.value();
    });

    // Generate / Undo Logic
    let undo_store: Rc<RefCell<Option<Structure>>> = Rc::new(RefCell::new(None));
    let state_gen = state.clone();
    let undo_gen = undo_store.clone();
    let btn_undo_gen = btn_undo.clone();
    let lbl_gen = lbl_status.clone();

    btn_gen.connect_clicked(move |_| {
        let mut st = state_gen.borrow_mut();
        let tab = st.active_tab_mut();

        if let Some(structure) = &tab.structure {
            *undo_gen.borrow_mut() = Some(structure.clone());

            let h = spin_h.value() as i32;
            let k = spin_k.value() as i32;
            let l = spin_l.value() as i32;
            let thick = spin_thick.value() as u32;
            let vac = spin_vac.value();

            match slab::generate_slab(structure, h, k, l, thick, vac) {
                Ok(new_struct) => {
                    tab.structure = Some(new_struct);
                    lbl_gen.set_markup("<span color='green'>Slab generated.</span>");
                    btn_undo_gen.set_sensitive(true);
                }
                Err(e) => {
                    lbl_gen.set_markup(&format!("<span color='red'>Error: {}</span>", e));
                }
            }
        }
    });

    let state_undo = state.clone();
    let undo_store_ref = undo_store.clone();
    let btn_undo_ref = btn_undo.clone();
    let lbl_undo = lbl_status.clone();

    btn_undo.connect_clicked(move |_| {
        let mut st = state_undo.borrow_mut();
        let tab = st.active_tab_mut();
        if let Some(backup) = undo_store_ref.borrow_mut().take() {
            tab.structure = Some(backup);
            lbl_undo.set_text("Undone.");
            btn_undo_ref.set_sensitive(false);
        }
    });

    root
}
