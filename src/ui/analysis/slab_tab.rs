use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, Button, SpinButton, Align, Grid, DrawingArea};
use std::rc::Rc;
use std::cell::RefCell;
use std::f64::consts::PI;
use crate::state::AppState;
use crate::physics::slab;
use crate::model::structure::Structure;

struct VisState {
    h: f64, k: f64, l: f64,
    structure: Option<Structure>,
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    // 1. Layout: Horizontal
    let root = Box::new(Orientation::Horizontal, 20);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // Left Pane: Visualization
    let left_pane = Box::new(Orientation::Vertical, 10);
    left_pane.set_halign(Align::Start);

    // Right Pane: Controls
    let right_pane = Box::new(Orientation::Vertical, 10);
    right_pane.set_hexpand(true);

    // --- Visualization State ---
    let current_struct = state.borrow().structure.clone();
    let vis_state = Rc::new(RefCell::new(VisState {
        h: 1.0, k: 1.0, l: 0.0,
        structure: current_struct
    }));

    // --- Right Pane Header ---
    let title = Label::new(Some("Surface Slab Generator"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let info = Label::new(Some("Configure Miller indices to cut the crystal surface."));
    info.set_halign(Align::Start);
    info.set_margin_bottom(10);
    right_pane.append(&info);

    // --- Drawing Area ---
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(300);
    drawing_area.set_content_height(300);

    let draw_state = vis_state.clone();

    drawing_area.set_draw_func(move |_, cr, width, height| {
        // 1. White Background
        cr.set_source_rgb(0.98, 0.98, 0.98);
        cr.paint().expect("Failed to paint background");

        let w = width as f64;
        let h_dim = height as f64;
        let scale = w.min(h_dim) / 3.5;
        let cx = w / 2.0;
        let cy = h_dim / 2.0;

        let st = draw_state.borrow();
        let (nh, nk, nl) = (st.h, st.k, st.l);

        // 2. Projection Logic
        let yaw = PI / 4.0 + 0.5;
        let pitch = PI / 6.0;

        let project = |x: f64, y: f64, z: f64| -> (f64, f64) {
            let x1 = x * yaw.cos() - z * yaw.sin();
            let z1 = x * yaw.sin() + z * yaw.cos();
            let y2 = y * pitch.cos() - z1 * pitch.sin();
            (cx + x1 * scale, cy + y2 * scale)
        };

        // 3. Draw Atoms (With Periodic Ghosts)
        if let Some(structure) = &st.structure {
            for atom in &structure.atoms {
                // Cartesian positions are not useful here; we need Fractional.
                // Assuming atom.position is fractional if structure loaded from CIF/POSCAR
                // typically maintains internal coords.
                // BUT: crate::model::structure::Atom usually stores Cartesian.
                // We need to convert back to fractional for the Unit Box display?

                // SIMPLIFICATION:
                // Since this is just a schema, we assume the input is fractional
                // OR we map the bounding box.
                // If your app stores Cartesian, we must multiply by inverse lattice.
                // However, for visualization schema, let's assume we map the bounding box of the atoms to -1..1

                // ACTUALLY: The correct way for the schema is to assume the
                // loaded structure is the Unit Cell.
                // We need fractional coordinates.
                // Let's calculate them on the fly roughly, or assume the app stores fractional?
                // If app stores Cartesian, let's try to infer fractional by matrix inversion.

                // QUICK HACK for Schema:
                // We just iterate 3x3x3 shifts for every atom.
                // If the atom falls inside the [0, 1] box, we draw it.

                // Wait, if we don't have fractional coords, we can't easily do this.
                // Let's assume for this "Cartoon" that we are just plotting the atoms
                // relative to the lattice vectors A, B, C.

                // Let's calculate fractional coords 'f' manually here:
                // matrix M = columns(A, B, C)
                // f = M_inv * pos

                // Let's use the lattice stored in structure
                let lat = structure.lattice;
                let ax = lat[0][0]; let ay = lat[0][1]; let az = lat[0][2];
                let bx = lat[1][0]; let by = lat[1][1]; let bz = lat[1][2];
                let cx = lat[2][0]; let cy = lat[2][1]; let cz = lat[2][2];

                // Determinant & Inverse (Hardcoded 3x3 inv)
                let det = ax*(by*cz - bz*cy) - ay*(bx*cz - bz*cx) + az*(bx*cy - by*cx);
                if det.abs() > 1e-6 {
                    let inv_det = 1.0 / det;
                    // We only need this to get fractional coords of the atom
                    let x = atom.position[0]; let y = atom.position[1]; let z = atom.position[2];

                    let fx = ((by*cz - bz*cy)*x + (az*cy - ay*cz)*y + (ay*bz - az*by)*z) * inv_det;
                    let fy = ((bz*cx - bx*cz)*x + (ax*cz - az*cx)*y + (az*bx - ax*bz)*z) * inv_det;
                    let fz = ((bx*cy - by*cx)*x + (ay*cx - ax*cy)*y + (ax*by - ay*bx)*z) * inv_det;

                    // NOW: "Complete the Box" Logic
                    // We iterate shifts -1, 0, 1. If (f + shift) is in [0, 1], draw it.
                    // Actually, we want to draw atoms at boundaries [0] AND [1].
                    // So if f is 0.05, we draw at 0.05.
                    // We ALSO check f+1 (1.05 -> No), f-1 (-0.95 -> No).
                    // BUT: We want to show the corners.
                    // Rule: Draw any image of the atom that falls within [-0.01, 1.01].

                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                let nx = fx + dx as f64;
                                let ny = fy + dy as f64;
                                let nz = fz + dz as f64;

                                // Tolerance for "Visual Edge"
                                let eps = 0.05;
                                if nx >= -eps && nx <= 1.0+eps &&
                                   ny >= -eps && ny <= 1.0+eps &&
                                   nz >= -eps && nz <= 1.0+eps
                                {
                                    // Map 0..1 to -1..1 for drawing
                                    let bx = nx * 2.0 - 1.0;
                                    let by = ny * 2.0 - 1.0;
                                    let bz = nz * 2.0 - 1.0;

                                    let (px, py) = project(bx, by, bz);

                                    cr.new_path();
                                    cr.arc(px, py, 5.0, 0.0, 2.0 * PI);
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

        // 4. Draw Wireframe Cube
        cr.set_line_width(1.5);
        cr.set_source_rgb(0.2, 0.2, 0.2);

        let corners = [
            (-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, 1.0, -1.0),
            (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (1.0, 1.0, 1.0), (-1.0, 1.0, 1.0),
        ];

        let edges = [
            (0,1), (1,2), (2,3), (3,0),
            (4,5), (5,6), (6,7), (7,4),
            (0,4), (1,5), (2,6), (3,7)
        ];

        for (start, end) in edges {
            let p1 = project(corners[start].0, corners[start].1, corners[start].2);
            let p2 = project(corners[end].0, corners[end].1, corners[end].2);
            cr.move_to(p1.0, p1.1);
            cr.line_to(p2.0, p2.1);
            cr.stroke().expect("Drawing failed");
        }

        // 5. Draw Cutting Plane
        let len = (nh*nh + nk*nk + nl*nl).sqrt();
        if len > 0.001 {
            let n = (nh/len, nk/len, nl/len);
            let up = if n.1.abs() > 0.9 { (0.0, 0.0, 1.0) } else { (0.0, 1.0, 0.0) };
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

            cr.set_source_rgba(0.8, 0.2, 0.2, 0.3);
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

    left_pane.append(&drawing_area);
    root.append(&left_pane);

    // --- Right Pane: Controls ---
    let grid = Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);
    grid.set_margin_top(10);

    grid.attach(&Label::new(Some("Miller Indices:")), 0, 0, 1, 1);
    let spin_h = SpinButton::with_range(-10.0, 10.0, 1.0); spin_h.set_value(1.0);
    let spin_k = SpinButton::with_range(-10.0, 10.0, 1.0); spin_k.set_value(1.0);
    let spin_l = SpinButton::with_range(-10.0, 10.0, 1.0); spin_l.set_value(0.0);

    let da_clone = drawing_area.clone();
    let state_h = vis_state.clone();
    spin_h.connect_value_changed(move |s| { state_h.borrow_mut().h = s.value(); da_clone.queue_draw(); });

    let da_clone = drawing_area.clone();
    let state_k = vis_state.clone();
    spin_k.connect_value_changed(move |s| { state_k.borrow_mut().k = s.value(); da_clone.queue_draw(); });

    let da_clone = drawing_area.clone();
    let state_l = vis_state.clone();
    spin_l.connect_value_changed(move |s| { state_l.borrow_mut().l = s.value(); da_clone.queue_draw(); });

    let box_hkl = Box::new(Orientation::Horizontal, 5);
    box_hkl.append(&Label::new(Some("h:"))); box_hkl.append(&spin_h);
    box_hkl.append(&Label::new(Some("k:"))); box_hkl.append(&spin_k);
    box_hkl.append(&Label::new(Some("l:"))); box_hkl.append(&spin_l);
    grid.attach(&box_hkl, 1, 0, 3, 1);

    grid.attach(&Label::new(Some("Thickness (layers):")), 0, 1, 1, 1);
    let spin_thick = SpinButton::with_range(1.0, 50.0, 1.0);
    spin_thick.set_value(1.0);
    grid.attach(&spin_thick, 1, 1, 1, 1);

    grid.attach(&Label::new(Some("Vacuum (Å):")), 0, 2, 1, 1);
    let spin_vac = SpinButton::with_range(0.0, 100.0, 1.0);
    spin_vac.set_value(10.0);
    grid.attach(&spin_vac, 1, 2, 1, 1);

    right_pane.append(&grid);

    // Buttons
    let btn_box = Box::new(Orientation::Vertical, 5);
    btn_box.set_margin_top(20);
    btn_box.set_halign(Align::Fill);

    let btn_gen = Button::with_label("Generate Slab");
    btn_gen.add_css_class("suggested-action");

    let btn_undo = Button::with_label("Undo Last Cut");
    btn_undo.set_sensitive(false);

    btn_box.append(&btn_gen);
    btn_box.append(&btn_undo);
    right_pane.append(&btn_box);

    let lbl_status = Label::new(Some(""));
    lbl_status.set_margin_top(10);
    lbl_status.set_wrap(true);
    right_pane.append(&lbl_status);

    root.append(&right_pane);

    // Logic
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
                    lbl_gen.set_markup("<span color='green'>Success.</span>");
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
