use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, Button, SpinButton, Align, Grid, DrawingArea, Frame, Separator};
use std::rc::Rc;
use std::cell::RefCell;
use std::f64::consts::PI;
use crate::state::AppState;
use crate::physics::voids::{self, VoidResult};
use crate::model::structure::Structure;

// Common ions for intercalation (Symbol, Ionic Radius in Angstroms)
const IONS: [(&str, f64); 8] = [
    ("Li⁺", 0.76), ("Mg²⁺", 0.72), ("Zn²⁺", 0.74), ("Na⁺", 1.02),
    ("Ca²⁺", 1.00), ("K⁺", 1.38), ("O²⁻", 1.40), ("Al³⁺", 0.54),
];

struct VoidsVisState {
    structure: Option<Structure>,
    result: Option<VoidResult>,
}

// Helper struct for Depth Sorting (Painter's Algorithm)
struct DrawableAtom {
    x: f64,
    y: f64,
    z_depth: f64,
    r: f64,
    color: (f64, f64, f64, f64),
    is_void: bool,
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    let root = Box::new(Orientation::Horizontal, 15);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // ================= LEFT PANE: Visualization =================
    let left_pane = Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);

    let frame = Frame::new(Some("Structure & Max Void"));
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
    right_pane.set_width_request(300);

    let title = Label::new(Some("Void Analysis"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let desc = Label::new(Some("Identify the largest spherical pore and intercalation candidates."));
    desc.set_wrap(true);
    desc.set_halign(Align::Start);
    right_pane.append(&desc);

    // Controls
    let ctrl_box = Box::new(Orientation::Vertical, 5);
    let row_res = Box::new(Orientation::Horizontal, 10);
    row_res.append(&Label::new(Some("Grid Resolution (Å):")));

    let spin_res = SpinButton::with_range(0.1, 1.0, 0.1);
    spin_res.set_value(0.25);
    spin_res.set_hexpand(true);
    row_res.append(&spin_res);
    ctrl_box.append(&row_res);

    let btn_calc = Button::with_label("Calculate Void");
    btn_calc.add_css_class("suggested-action");
    btn_calc.set_margin_top(5);
    ctrl_box.append(&btn_calc);

    right_pane.append(&ctrl_box);
    right_pane.append(&Separator::new(Orientation::Horizontal));

    // Results
    let res_grid = Grid::new();
    res_grid.set_column_spacing(10);
    res_grid.set_row_spacing(8);

    let add_row = |grid: &Grid, row: i32, title: &str, val_lbl: &Label| {
        let l = Label::builder().label(title).halign(Align::Start).build();
        grid.attach(&l, 0, row, 1, 1);
        grid.attach(val_lbl, 1, row, 1, 1);
    };

    let val_r = Label::builder().label("-").halign(Align::Start).build();
    val_r.add_css_class("title-3");
    add_row(&res_grid, 0, "Max Radius:", &val_r);
    let val_d = Label::builder().label("-").halign(Align::Start).build();
    add_row(&res_grid, 1, "Diameter:", &val_d);
    let val_pos = Label::builder().label("-").halign(Align::Start).build();
    add_row(&res_grid, 2, "Center (x,y,z):", &val_pos);
    let val_vol = Label::builder().label("-").halign(Align::Start).build();
    add_row(&res_grid, 3, "Void Volume %:", &val_vol);

    right_pane.append(&res_grid);

    let lbl_cand_title = Label::builder().label("Intercalation Candidates:").halign(Align::Start).margin_top(10).build();
    right_pane.append(&lbl_cand_title);
    let val_cand = Label::builder().label("-").halign(Align::Start).wrap(true).build();
    right_pane.append(&val_cand);

    root.append(&right_pane);

    // ================= LOGIC =================
    let vis_state = Rc::new(RefCell::new(VoidsVisState {
        structure: state.borrow().structure.clone(),
        result: None,
    }));

    let state_calc = state.clone();
    let vis_state_calc = vis_state.clone();
    let da_calc = drawing_area.clone();

    btn_calc.connect_clicked(move |_| {
        let st = state_calc.borrow();
        if let Some(structure) = &st.structure {
            let res = spin_res.value();
            let result = voids::calculate_voids(structure, res);
            let r_max = result.max_sphere_radius;

            if r_max > 0.0 {
                val_r.set_text(&format!("{:.3} Å", r_max));
                val_d.set_text(&format!("{:.3} Å", r_max * 2.0));
                val_pos.set_text(&format!("{:.2}, {:.2}, {:.2}",
                    result.max_sphere_center[0], result.max_sphere_center[1], result.max_sphere_center[2]));
                val_vol.set_text(&format!("{:.2} %", result.void_fraction));

                let mut fits = Vec::new();
                for (symbol, r_ion) in IONS.iter() {
                    if *r_ion <= r_max { fits.push(*symbol); }
                }
                if fits.is_empty() {
                    val_cand.set_markup("<span color='orange'>None (Too Dense)</span>");
                } else {
                    val_cand.set_markup(&format!("<b>{}</b>", fits.join(", ")));
                }
            } else {
                val_r.set_text("0.00 Å");
                val_cand.set_text("Structure is dense.");
            }

            let mut vs = vis_state_calc.borrow_mut();
            vs.structure = Some(structure.clone());
            vs.result = Some(result);
            da_calc.queue_draw();
        } else {
            val_r.set_text("No Structure");
        }
    });

    // ================= DRAWING (With Depth Sort) =================
    let vis_state_draw = vis_state.clone();

    drawing_area.set_draw_func(move |_, cr, width, height| {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().unwrap();

        let w = width as f64;
        let h_dim = height as f64;
        let scale = w.min(h_dim) / 4.0;
        let cx = w / 2.0;
        let cy = h_dim / 2.0;

        let yaw = PI / 4.0 + 0.4;
        let pitch = PI / 6.0;

        // Projection function returning (screen_x, screen_y, z_depth)
        let project_3d = |x: f64, y: f64, z: f64| -> (f64, f64, f64) {
            let x1 = x * yaw.cos() - z * yaw.sin();
            let z1 = x * yaw.sin() + z * yaw.cos();
            let y2 = y * pitch.cos() - z1 * pitch.sin();
            let z2 = y * pitch.sin() + z1 * pitch.cos(); // Depth
            (cx + x1 * scale, cy + y2 * scale, z2)
        };

        let vs = vis_state_draw.borrow();
        if let Some(structure) = &vs.structure {
            // Lattice Matrix
            let lat = structure.lattice;
            let ax = lat[0][0]; let ay = lat[0][1]; let az = lat[0][2];
            let bx = lat[1][0]; let by = lat[1][1]; let bz = lat[1][2];
            let cx_v = lat[2][0]; let cy_v = lat[2][1]; let cz_v = lat[2][2];

            // Determinant & Inverse
            let det = ax*(by*cz_v - bz*cy_v) - ay*(bx*cz_v - bz*cx_v) + az*(bx*cy_v - by*cx_v);
            let inv_det = if det.abs() > 1e-6 { 1.0/det } else { 0.0 };

            let mut draw_list: Vec<DrawableAtom> = Vec::new();

            // 1. Process Atoms
            for atom in &structure.atoms {
                // Cartesian to Fractional
                let x = atom.position[0]; let y = atom.position[1]; let z = atom.position[2];
                let mut fx = ((by*cz_v - bz*cy_v)*x + (az*cy_v - ay*cz_v)*y + (ay*bz - az*by)*z) * inv_det;
                let mut fy = ((bz*cx_v - bx*cz_v)*x + (ax*cz_v - az*cx_v)*y + (az*bx - ax*bz)*z) * inv_det;
                let mut fz = ((bx*cy_v - by*cx_v)*x + (ay*cx_v - ax*cy_v)*y + (ax*by - ay*bx)*z) * inv_det;

                // WRAP to [0, 1) first to standardize input
                fx = fx.rem_euclid(1.0);
                fy = fy.rem_euclid(1.0);
                fz = fz.rem_euclid(1.0);

                // Generate Ghosts: Check -1, 0, 1 for boundary overlap
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let nx = fx + dx as f64;
                            let ny = fy + dy as f64;
                            let nz = fz + dz as f64;

                            // Visual Tolerance: Draw if within [-0.05, 1.05]
                            // This ensures an atom at 0.02 is drawn at 0.02 AND 1.02
                            let eps = 0.05;
                            if nx >= -eps && nx <= 1.0+eps &&
                               ny >= -eps && ny <= 1.0+eps &&
                               nz >= -eps && nz <= 1.0+eps
                            {
                                // Map 0..1 to -1..1 for drawing centered
                                let bx_d = nx * 2.0 - 1.0;
                                let by_d = ny * 2.0 - 1.0;
                                let bz_d = nz * 2.0 - 1.0;

                                let (px, py, pz) = project_3d(bx_d, by_d, bz_d);

                                draw_list.push(DrawableAtom {
                                    x: px, y: py, z_depth: pz,
                                    r: 6.0,
                                    color: (0.0, 0.5, 0.5, 0.9),
                                    is_void: false
                                });
                            }
                        }
                    }
                }
            }

            // 2. Process Void Sphere
            if let Some(res) = &vs.result {
                if res.max_sphere_radius > 0.1 {
                    let vx = res.max_sphere_center[0];
                    let vy = res.max_sphere_center[1];
                    let vz = res.max_sphere_center[2];

                    // Convert Void Center Cartesian -> Fractional
                    let mut vfx = ((by*cz_v - bz*cy_v)*vx + (az*cy_v - ay*cz_v)*vy + (ay*bz - az*by)*vz) * inv_det;
                    let mut vfy = ((bz*cx_v - bx*cz_v)*vx + (ax*cz_v - az*cx_v)*vy + (az*bx - ax*bz)*vz) * inv_det;
                    let mut vfz = ((bx*cy_v - by*cx_v)*vx + (ay*cx_v - ax*cy_v)*vy + (ax*by - ay*bx)*vz) * inv_det;

                    // Wrap Void to unit cell for display
                    vfx = vfx.rem_euclid(1.0);
                    vfy = vfy.rem_euclid(1.0);
                    vfz = vfz.rem_euclid(1.0);

                    let bx_d = vfx * 2.0 - 1.0;
                    let by_d = vfy * 2.0 - 1.0;
                    let bz_d = vfz * 2.0 - 1.0;

                    let (px, py, pz) = project_3d(bx_d, by_d, bz_d);
                    let avg_lat = (ax*ax+ay*ay+az*az).sqrt().max(5.0);
                    let draw_r = (res.max_sphere_radius / avg_lat) * scale * 2.5;

                    draw_list.push(DrawableAtom {
                        x: px, y: py, z_depth: pz,
                        r: draw_r,
                        color: (0.9, 0.7, 0.0, 0.6), // Gold
                        is_void: true
                    });
                }
            }

            // 3. SORT by Depth (Z) - Back to Front
            // Higher Z is "deeper" into the screen (depending on coord system),
            // usually in this projection Z+ is towards viewer?
            // Let's check: x1*sin + z*cos. If we rotate Y, z moves.
            // Standard: Sort ascending (draw small Z first) or descending?
            // If z2 increases away from camera, draw large Z first.
            // Let's try sorting Ascending (Small Z = Background).
            draw_list.sort_by(|a, b| a.z_depth.partial_cmp(&b.z_depth).unwrap_or(std::cmp::Ordering::Equal));

            // 4. Draw Unit Cell Box (Wireframe - Always behind or handle separately?
            // Box lines don't occlude, so just draw them first.)
            cr.set_line_width(1.0);
            cr.set_source_rgb(0.7, 0.7, 0.7);
            let corners = [
                (-1., -1., -1.), (1., -1., -1.), (1., 1., -1.), (-1., 1., -1.),
                (-1., -1., 1.), (1., -1., 1.), (1., 1., 1.), (-1., 1., 1.)
            ];
            let edges = [(0,1), (1,2), (2,3), (3,0), (4,5), (5,6), (6,7), (7,4), (0,4), (1,5), (2,6), (3,7)];
            for (s, e) in edges {
                let p1 = project_3d(corners[s].0, corners[s].1, corners[s].2);
                let p2 = project_3d(corners[e].0, corners[e].1, corners[e].2);
                cr.move_to(p1.0, p1.1);
                cr.line_to(p2.0, p2.1);
                cr.stroke().unwrap();
            }

            // 5. Draw Atoms/Void
            for item in draw_list {
                cr.new_path();
                cr.arc(item.x, item.y, item.r, 0.0, 2.0 * PI);

                if item.is_void {
                    // Void Style
                    cr.set_source_rgba(item.color.0, item.color.1, item.color.2, item.color.3);
                    cr.fill_preserve().unwrap();
                    cr.set_source_rgba(item.color.0, item.color.1, item.color.2, 1.0);
                    cr.set_line_width(2.0);
                    cr.set_dash(&[4.0, 2.0], 0.0);
                    cr.stroke().unwrap();
                    cr.set_dash(&[], 0.0);
                } else {
                    // Atom Style
                    cr.set_source_rgba(item.color.0, item.color.1, item.color.2, item.color.3);
                    cr.fill_preserve().unwrap();
                    cr.set_source_rgb(0.0, 0.3, 0.3);
                    cr.set_line_width(1.0);
                    cr.stroke().unwrap();
                }
            }
        }
    });

    root
}
