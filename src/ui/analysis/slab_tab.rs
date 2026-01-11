// src/ui/analysis/slab_tab.rs
use crate::model::structure::Structure;
use crate::physics::operations::slab;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Align, Box, Button, DrawingArea, Frame, Grid, Label, Orientation, SpinButton};
use nalgebra::{Matrix3, Vector3}; // Use nalgebra for cleaner UI math
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

// Shared state for the visualization
struct VisState {
    h: f64,
    k: f64,
    l: f64,
    structure: Option<Structure>,
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    // Root Layout (Horizontal)
    let root = Box::new(Orientation::Horizontal, 15);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // ================= LEFT PANE: Visualization =================
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

    // ================= RIGHT PANE: Controls =================
    let right_pane = Box::new(Orientation::Vertical, 10);
    right_pane.set_width_request(250);

    // Header
    let title = Label::new(Some("Slab Generator"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let info = Label::new(Some(
        "Define the Miller indices (h k l) to cut the surface.",
    ));
    info.set_wrap(true);
    info.set_halign(Align::Start);
    info.set_margin_bottom(10);
    right_pane.append(&info);

    // Grid for Inputs
    let grid = Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);

    // 1. Miller Indices
    grid.attach(&Label::new(Some("Miller Indices:")), 0, 0, 3, 1);

    let spin_h = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_h.set_value(1.0);
    let spin_k = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_k.set_value(1.0);
    let spin_l = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_l.set_value(0.0);

    // Row 1: Labels h, k, l
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

    // 2. Thickness
    grid.attach(&Label::new(Some("Thickness (layers):")), 0, 2, 2, 1);
    let spin_thick = SpinButton::with_range(1.0, 50.0, 1.0);
    spin_thick.set_value(1.0);
    grid.attach(&spin_thick, 2, 2, 1, 1);

    // 3. Vacuum
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

    // Status
    let lbl_status = Label::new(Some("Ready."));
    lbl_status.set_margin_top(10);
    lbl_status.set_wrap(true);
    right_pane.append(&lbl_status);

    root.append(&right_pane);

    // ================= LOGIC =================

    // --- Visualization State ---
    let current_struct = state.borrow().structure.clone();
    let vis_state = Rc::new(RefCell::new(VisState {
        h: 1.0,
        k: 1.0,
        l: 0.0,
        structure: current_struct,
    }));

    // --- Drawing Function (The 3D Cube + Atoms) ---
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

        // Projection Logic
        let yaw = PI / 4.0 + 0.5;
        let pitch = PI / 6.0;

        let project = |x: f64, y: f64, z: f64| -> (f64, f64) {
            let x1 = x * yaw.cos() - z * yaw.sin();
            let z1 = x * yaw.sin() + z * yaw.cos();
            let y2 = y * pitch.cos() - z1 * pitch.sin();
            // Invert Y for screen coordinates
            (cx + x1 * scale, cy + y2 * scale)
        };

        // 2. Draw Atoms (With Periodic Ghosts)
        if let Some(structure) = &st.structure {
            // Replaced manual math with nalgebra for safety
            let lat = structure.lattice;
            let lat_mat = Matrix3::new(
                lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0],
                lat[2][1], lat[2][2],
            );

            // Need Inverse to get Fractional Coords
            // Be careful: if your lattice is degenerate, this returns None
            if let Some(inv_lat) = lat_mat.try_inverse() {
                // Since our lattice uses Row vectors, Fractional = Inv^T * Cart
                let to_frac = inv_lat.transpose();

                for atom in &structure.atoms {
                    let cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                    let frac = to_frac * cart;

                    // "Complete the Box" Logic
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                let nx = frac.x + dx as f64;
                                let ny = frac.y + dy as f64;
                                let nz = frac.z + dz as f64;

                                let eps = 0.05;
                                if nx >= -eps
                                    && nx <= 1.0 + eps
                                    && ny >= -eps
                                    && ny <= 1.0 + eps
                                    && nz >= -eps
                                    && nz <= 1.0 + eps
                                {
                                    // Map fractional 0..1 to drawing box -1..1
                                    let bx = nx * 2.0 - 1.0;
                                    let by = ny * 2.0 - 1.0;
                                    let bz = nz * 2.0 - 1.0;

                                    let (px, py) = project(bx, by, bz);

                                    // Draw Atom
                                    cr.new_path();
                                    cr.arc(px, py, 6.0, 0.0, 2.0 * PI);
                                    cr.set_source_rgba(0.0, 0.5, 0.5, 0.8);
                                    cr.fill_preserve().unwrap();
                                    cr.set_source_rgb(0.0, 0.3, 0.3);
                                    cr.set_line_width(1.0);
                                    cr.stroke().unwrap();
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Draw Wireframe Cube (Unit Cell)
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
            (3, 0), // Back face
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4), // Front face
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7), // Connectors
        ];

        for (start, end) in edges {
            let p1 = project(corners[start].0, corners[start].1, corners[start].2);
            let p2 = project(corners[end].0, corners[end].1, corners[end].2);
            cr.move_to(p1.0, p1.1);
            cr.line_to(p2.0, p2.1);
            cr.stroke().expect("Drawing failed");
        }

        // 4. Draw Cutting Plane (Red Poly)
        let len = (nh * nh + nk * nk + nl * nl).sqrt();
        if len > 0.001 {
            let n = Vector3::new(nh / len, nk / len, nl / len);

            // Choose an arbitrary "up" vector not parallel to n
            let up = if n.y.abs() > 0.9 {
                Vector3::new(0.0, 0.0, 1.0)
            } else {
                Vector3::new(0.0, 1.0, 0.0)
            };

            let u = n.cross(&up).normalize();
            let v = n.cross(&u).normalize();
            let sz = 1.4;

            let pts = [
                -u * sz - v * sz,
                u * sz - v * sz,
                u * sz + v * sz,
                -u * sz + v * sz,
            ];

            cr.set_source_rgba(0.8, 0.2, 0.2, 0.3); // Red transparent

            let p0 = project(pts[0].x, pts[0].y, pts[0].z);
            cr.move_to(p0.0, p0.1);
            for i in 1..4 {
                let p = project(pts[i].x, pts[i].y, pts[i].z);
                cr.line_to(p.0, p.1);
            }
            cr.close_path();
            cr.fill_preserve().expect("Fill failed");
            cr.set_source_rgb(0.8, 0.2, 0.2);
            cr.stroke().expect("Stroke failed");
        }
    });

    // --- Update Signals ---
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

    // --- Generate / Undo Logic ---
    let undo_store: Rc<RefCell<Option<Structure>>> = Rc::new(RefCell::new(None));

    let state_gen = state.clone();
    let undo_gen = undo_store.clone();
    let btn_undo_gen = btn_undo.clone();
    let lbl_gen = lbl_status.clone();

    btn_gen.connect_clicked(move |_| {
        let mut st = state_gen.borrow_mut();
        if let Some(structure) = &st.structure {
            *undo_gen.borrow_mut() = Some(structure.clone());

            let h = spin_h.value() as i32;
            let k = spin_k.value() as i32;
            let l = spin_l.value() as i32;
            let thick = spin_thick.value() as u32;
            let vac = spin_vac.value();

            // Calling the updated physics engine
            match slab::generate_slab(structure, h, k, l, thick, vac) {
                Ok(new_struct) => {
                    st.structure = Some(new_struct);
                    lbl_gen.set_markup("<span color='green'>Slab generated.</span>");
                    btn_undo_gen.set_sensitive(true);
                }
                Err(e) => {
                    lbl_gen.set_markup(&format!("<span color='red'>Error: {}</span>", e));
                }
            }
        } else {
            lbl_gen.set_text("No structure loaded.");
        }
    });

    // Undo Logic (Unchanged)
    let state_undo = state.clone();
    let undo_store_ref = undo_store.clone();
    let btn_undo_ref = btn_undo.clone();
    let lbl_undo = lbl_status.clone();

    btn_undo.connect_clicked(move |_| {
        let mut st = state_undo.borrow_mut();
        if let Some(backup) = undo_store_ref.borrow_mut().take() {
            st.structure = Some(backup);
            lbl_undo.set_text("Undone.");
            btn_undo_ref.set_sensitive(false);
        }
    });

    root
}
