use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, Button, SpinButton, Align, Grid, DrawingArea, Frame};
use std::rc::Rc;
use std::cell::RefCell;
use std::f64::consts::PI;
use crate::state::AppState;
use crate::physics::operations::slab;
use crate::model::structure::Structure;

// Shared state for the visualization
struct VisState {
    h: f64, k: f64, l: f64,
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
    left_pane.set_hexpand(true); // Take available width

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
    right_pane.set_width_request(250); // Fixed sidebar width

    // Header
    let title = Label::new(Some("Slab Generator"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let info = Label::new(Some("Define the Miller indices (h k l) to cut the surface."));
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

    let spin_h = SpinButton::with_range(-10.0, 10.0, 1.0); spin_h.set_value(1.0);
    let spin_k = SpinButton::with_range(-10.0, 10.0, 1.0); spin_k.set_value(1.0);
    let spin_l = SpinButton::with_range(-10.0, 10.0, 1.0); spin_l.set_value(0.0);

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
        h: 1.0, k: 1.0, l: 0.0,
        structure: current_struct
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
            (cx + x1 * scale, cy + y2 * scale)
        };

        // 2. Draw Atoms (With Periodic Ghosts)
        if let Some(structure) = &st.structure {
            // Need lattice for Cartesian -> Fractional conversion
            let lat = structure.lattice;
            let ax = lat[0][0]; let ay = lat[0][1]; let az = lat[0][2];
            let bx = lat[1][0]; let by = lat[1][1]; let bz = lat[1][2];
            let cx = lat[2][0]; let cy = lat[2][1]; let cz = lat[2][2];

            // Determinant & Inverse (Hardcoded 3x3 inv)
            let det = ax*(by*cz - bz*cy) - ay*(bx*cz - bz*cx) + az*(bx*cy - by*cx);

            if det.abs() > 1e-6 {
                let inv_det = 1.0 / det;

                for atom in &structure.atoms {
                    let x = atom.position[0];
                    let y = atom.position[1];
                    let z = atom.position[2];

                    // Convert Cartesian to Fractional
                    let fx = ((by*cz - bz*cy)*x + (az*cy - ay*cz)*y + (ay*bz - az*by)*z) * inv_det;
                    let fy = ((bz*cx - bx*cz)*x + (ax*cz - az*cx)*y + (az*bx - ax*bz)*z) * inv_det;
                    let fz = ((bx*cy - by*cx)*x + (ay*cx - ax*cy)*y + (ax*by - ay*bx)*z) * inv_det;

                    // "Complete the Box" Logic: Iterate shifts to fill visual unit cell
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                let nx = fx + dx as f64;
                                let ny = fy + dy as f64;
                                let nz = fz + dz as f64;

                                // Tolerance for "Visual Edge" (atoms exactly on boundary appear on both sides)
                                let eps = 0.05;
                                if nx >= -eps && nx <= 1.0+eps &&
                                   ny >= -eps && ny <= 1.0+eps &&
                                   nz >= -eps && nz <= 1.0+eps
                                {
                                    // Map fractional 0..1 to drawing box coordinates -1..1
                                    let bx = nx * 2.0 - 1.0;
                                    let by = ny * 2.0 - 1.0;
                                    let bz = nz * 2.0 - 1.0;

                                    let (px, py) = project(bx, by, bz);

                                    // Draw Atom Sphere
                                    cr.new_path();
                                    cr.arc(px, py, 6.0, 0.0, 2.0 * PI);

                                    // Teal color
                                    cr.set_source_rgba(0.0, 0.5, 0.5, 0.8);
                                    cr.fill_preserve().unwrap();

                                    // Outline
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
            (-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, 1.0, -1.0),
            (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (1.0, 1.0, 1.0), (-1.0, 1.0, 1.0),
        ];

        let edges = [
            (0,1), (1,2), (2,3), (3,0), // Back face
            (4,5), (5,6), (6,7), (7,4), // Front face
            (0,4), (1,5), (2,6), (3,7)  // Connecting edges
        ];

        for (start, end) in edges {
            let p1 = project(corners[start].0, corners[start].1, corners[start].2);
            let p2 = project(corners[end].0, corners[end].1, corners[end].2);
            cr.move_to(p1.0, p1.1);
            cr.line_to(p2.0, p2.1);
            cr.stroke().expect("Drawing failed");
        }

        // 4. Draw Cutting Plane (Red Poly)
        let len = (nh*nh + nk*nk + nl*nl).sqrt();
        if len > 0.001 {
            let n = (nh/len, nk/len, nl/len);
            let up = if n.1.abs() > 0.9 { (0.0, 0.0, 1.0) } else { (0.0, 1.0, 0.0) };

            // Cross product helpers
            let cross = |a: (f64,f64,f64), b: (f64,f64,f64)| (a.1*b.2 - a.2*b.1, a.2*b.0 - a.0*b.2, a.0*b.1 - a.1*b.0);
            let normalize = |v: (f64,f64,f64)| { let l = (v.0*v.0+v.1*v.1+v.2*v.2).sqrt(); if l==0.0{(0.,0.,0.)}else{(v.0/l,v.1/l,v.2/l)} };

            let u = normalize(cross(n, up));
            let v = normalize(cross(n, u));
            let sz = 1.4;

            let pts = [
                (-u.0*sz - v.0*sz, -u.1*sz - v.1*sz, -u.2*sz - v.2*sz),
                ( u.0*sz - v.0*sz,  u.1*sz - v.1*sz,  u.2*sz - v.2*sz),
                ( u.0*sz + v.0*sz,  u.1*sz + v.1*sz,  u.2*sz + v.2*sz),
                (-u.0*sz + v.0*sz, -u.1*sz + v.1*sz, -u.2*sz + v.2*sz),
            ];

            cr.set_source_rgba(0.8, 0.2, 0.2, 0.3); // Red transparent
            let p0 = project(pts[0].0, pts[0].1, pts[0].2);
            cr.move_to(p0.0, p0.1);
            for i in 1..4 {
                let p = project(pts[i].0, pts[i].1, pts[i].2);
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
    spin_h.connect_value_changed(move |s| { state_h.borrow_mut().h = s.value(); da_clone.queue_draw(); });

    let da_clone = drawing_area.clone();
    let state_k = vis_state.clone();
    spin_k.connect_value_changed(move |s| { state_k.borrow_mut().k = s.value(); da_clone.queue_draw(); });

    let da_clone = drawing_area.clone();
    let state_l = vis_state.clone();
    spin_l.connect_value_changed(move |s| { state_l.borrow_mut().l = s.value(); da_clone.queue_draw(); });

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

            match slab::generate_slab(structure, h, k, l, thick, vac) {
                Ok(new_struct) => {
                    st.structure = Some(new_struct);
                    lbl_gen.set_markup("<span color='green'>Slab generated.</span>");
                    btn_undo_gen.set_sensitive(true);
                },
                Err(e) => {
                    lbl_gen.set_markup(&format!("<span color='red'>Error: {}</span>", e));
                }
            }
        } else {
            lbl_gen.set_text("No structure loaded.");
        }
    });

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
