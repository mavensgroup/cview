use crate::model::structure::Structure;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{
    Align, Box, Button, DrawingArea, DropDown, Frame, Grid, Label, Orientation, Separator,
    SpinButton, StringList,
};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

// Import constants and types from Physics
use crate::physics::analysis::voids::{self, RadiusType, VoidConfig, VoidResult};

struct VoidsVisState {
    structure: Option<Structure>,
    result: Option<VoidResult>,
}

struct DrawableAtom {
    x: f64,
    y: f64,
    z: f64,
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

    // --- LEFT PANE (Visualization) ---
    let left_pane = Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);
    let frame = Frame::new(Some("Structure & Void"));
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(400);
    drawing_area.set_content_height(400);
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);
    frame.set_child(Some(&drawing_area));
    left_pane.append(&frame);
    root.append(&left_pane);

    // --- RIGHT PANE (Controls) ---
    let right_pane = Box::new(Orientation::Vertical, 10);
    right_pane.set_width_request(300);

    let title = Label::new(Some("Void Analysis"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    right_pane.append(&title);

    let ctrl_box = Box::new(Orientation::Vertical, 8);

    // 1. Grid Resolution
    let row_res = Box::new(Orientation::Horizontal, 10);
    row_res.append(&Label::new(Some("Grid Res (pts/Å):")));
    let spin_res = SpinButton::with_range(0.1, 2.0, 0.1);
    spin_res.set_value(0.3);
    spin_res.set_hexpand(true);
    row_res.append(&spin_res);
    ctrl_box.append(&row_res);

    // 2. Radius Type
    let row_type = Box::new(Orientation::Horizontal, 10);
    row_type.append(&Label::new(Some("Radius Type:")));
    // Added "Ionic" as the first option
    let type_model = StringList::new(&["Ionic", "Van der Waals", "Covalent"]);
    let drop_type = DropDown::new(Some(type_model), None::<&gtk4::Expression>);
    drop_type.set_selected(0); // Default to Ionic
    drop_type.set_hexpand(true);
    row_type.append(&drop_type);
    ctrl_box.append(&row_type);

    // 3. Atom Scale
    let row_scale = Box::new(Orientation::Horizontal, 10);
    row_scale.append(&Label::new(Some("Radius Scale:")));
    let spin_scale = SpinButton::with_range(0.1, 1.5, 0.05);
    spin_scale.set_value(1.0);
    spin_scale.set_hexpand(true);
    row_scale.append(&spin_scale);
    ctrl_box.append(&row_scale);

    // 4. Probe Radius
    let row_probe = Box::new(Orientation::Horizontal, 10);
    row_probe.append(&Label::new(Some("Probe Radius (Å):")));
    let spin_probe = SpinButton::with_range(0.0, 5.0, 0.05);
    spin_probe.set_value(1.20);
    spin_probe.set_hexpand(true);
    row_probe.append(&spin_probe);
    ctrl_box.append(&row_probe);

    // 5. Dynamic Probe Buttons (Source: Physics)
    let grid_probes = Grid::builder().row_spacing(5).column_spacing(5).build();
    for (i, (name, rad)) in voids::PRESET_PROBES.iter().enumerate() {
        let btn = Button::with_label(name);
        let sp = spin_probe.clone();
        let r_val = *rad;
        btn.connect_clicked(move |_| sp.set_value(r_val));
        grid_probes.attach(&btn, (i % 4) as i32, (i / 4) as i32, 1, 1);
    }
    ctrl_box.append(&grid_probes);

    let btn_calc = Button::with_label("Calculate");
    btn_calc.add_css_class("suggested-action");
    ctrl_box.append(&btn_calc);

    right_pane.append(&ctrl_box);
    right_pane.append(&Separator::new(Orientation::Horizontal));

    // Results Display
    let res_grid = Grid::new();
    res_grid.set_column_spacing(10);
    res_grid.set_row_spacing(5);
    let add_res = |r, t, l: &Label| {
        res_grid.attach(
            &Label::builder().label(t).halign(Align::Start).build(),
            0,
            r,
            1,
            1,
        );
        res_grid.attach(l, 1, r, 1, 1);
    };

    let val_r = Label::new(Some("-"));
    val_r.add_css_class("title-3");
    let val_d = Label::new(Some("-"));
    let val_vol = Label::new(Some("-"));
    add_res(0, "Max Radius:", &val_r);
    add_res(1, "Diameter:", &val_d);
    add_res(2, "Void Vol %:", &val_vol);

    right_pane.append(&res_grid);
    right_pane.append(
        &Label::builder()
            .label("Candidates:")
            .halign(Align::Start)
            .margin_top(8)
            .build(),
    );
    let val_cand = Label::builder()
        .label("-")
        .halign(Align::Start)
        .wrap(true)
        .build();
    right_pane.append(&val_cand);

    root.append(&right_pane);

    // --- INTERACTION LOGIC ---
    let vis_state = Rc::new(RefCell::new(VoidsVisState {
        structure: state.borrow().structure.clone(),
        result: None,
    }));
    let state_c = state.clone();
    let vis_c = vis_state.clone();
    let da_c = drawing_area.clone();

    btn_calc.connect_clicked(move |_| {
        let st = state_c.borrow();
        if let Some(structure) = &st.structure {
            // Map Index -> Enum
            let idx = drop_type.selected();
            let r_type = match idx {
                0 => RadiusType::Ionic,
                1 => RadiusType::VanDerWaals,
                _ => RadiusType::Covalent,
            };

            // Create Config
            let config = VoidConfig {
                grid_resolution: spin_res.value(),
                probe_radius: spin_probe.value(),
                radii_scale: spin_scale.value(),
                radius_type: r_type,
                max_grid_points: 10_000_000,
            };

            // Calculate & Handle Result
            match voids::calculate_voids(structure, config) {
                Ok(result) => {
                    let r_max = result.max_sphere_radius;

                    val_r.set_text(&format!("{:.3} Å", r_max));
                    val_d.set_text(&format!("{:.3} Å", r_max * 2.0));
                    val_vol.set_text(&format!("{:.2} %", result.void_fraction));

                    if r_max > 0.0 {
                        let mut fits = Vec::new();
                        for (ion, rad) in voids::CANDIDATE_IONS {
                            if *rad <= r_max {
                                fits.push(*ion);
                            }
                        }
                        if fits.is_empty() {
                            val_cand.set_markup("<span color='orange'>None (Too Small)</span>");
                        } else {
                            val_cand.set_markup(&format!("<b>{}</b>", fits.join(", ")));
                        }
                    } else {
                        val_cand.set_markup("<span color='red'>Overlap Detected</span>");
                    }

                    let mut vs = vis_c.borrow_mut();
                    vs.structure = Some(structure.clone());
                    vs.result = Some(result);
                    da_c.queue_draw();
                }
                Err(e) => {
                    val_cand.set_markup(&format!("<span color='red'>Error: {}</span>", e));
                    val_r.set_text("-");
                }
            }
        }
    });

    // --- DRAWING LOGIC (Cartesian + Fixed Sorting) ---
    let vis_draw = vis_state.clone();
    drawing_area.set_draw_func(move |_, cr, width, height| {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().unwrap();
        let w = width as f64;
        let h = height as f64;
        let cx = w / 2.0;
        let cy = h / 2.0;
        let yaw = PI / 4.0 + 0.3;
        let pitch = PI / 6.0;

        let vs = vis_draw.borrow();
        if let Some(structure) = &vs.structure {
            let lat = structure.lattice;
            let ax = lat[0][0];
            let ay = lat[0][1];
            let az = lat[0][2];
            let bx = lat[1][0];
            let by = lat[1][1];
            let bz = lat[1][2];
            let cx_vec = lat[2][0];
            let cy_vec = lat[2][1];
            let cz_vec = lat[2][2];

            let len_a = (ax * ax + ay * ay + az * az).sqrt();
            let len_b = (bx * bx + by * by + bz * bz).sqrt();
            let len_c = (cx_vec * cx_vec + cy_vec * cy_vec + cz_vec * cz_vec).sqrt();
            let max_dim = len_a.max(len_b).max(len_c);
            let view_scale = (w.min(h) * 0.5) / max_dim;

            let center_x = (ax + bx + cx_vec) * 0.5;
            let center_y = (ay + by + cy_vec) * 0.5;
            let center_z = (az + bz + cz_vec) * 0.5;

            let project = |x: f64, y: f64, z: f64| {
                let x1 = x * yaw.cos() - z * yaw.sin();
                let z1 = x * yaw.sin() + z * yaw.cos();
                let y2 = y * pitch.cos() - z1 * pitch.sin();
                let z2 = y * pitch.sin() + z1 * pitch.cos();
                (cx + x1 * view_scale, cy + y2 * view_scale, z2)
            };

            let det = ax * (by * cz_vec - bz * cy_vec) - ay * (bx * cz_vec - bz * cx_vec)
                + az * (bx * cy_vec - by * cx_vec);
            let inv_det = if det.abs() > 1e-6 { 1.0 / det } else { 0.0 };

            let mut list = Vec::new();

            for atom in &structure.atoms {
                let x = atom.position[0];
                let y = atom.position[1];
                let z = atom.position[2];
                let mut fx = ((by * cz_vec - bz * cy_vec) * x
                    + (az * cy_vec - ay * cz_vec) * y
                    + (ay * bz - az * by) * z)
                    * inv_det;
                let mut fy = ((bz * cx_vec - bx * cz_vec) * x
                    + (ax * cz_vec - az * cx_vec) * y
                    + (az * bx - ax * bz) * z)
                    * inv_det;
                let mut fz = ((bx * cy_vec - by * cx_vec) * x
                    + (ay * cx_vec - ax * cy_vec) * y
                    + (ax * by - ay * bx) * z)
                    * inv_det;

                fx = fx.rem_euclid(1.0);
                fy = fy.rem_euclid(1.0);
                fz = fz.rem_euclid(1.0);

                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let nx = fx + dx as f64;
                            let ny = fy + dy as f64;
                            let nz = fz + dz as f64;
                            if nx > -0.05
                                && nx < 1.05
                                && ny > -0.05
                                && ny < 1.05
                                && nz > -0.05
                                && nz < 1.05
                            {
                                let rx = nx * ax + ny * bx + nz * cx_vec;
                                let ry = nx * ay + ny * by + nz * cy_vec;
                                let rz = nx * az + ny * bz + nz * cz_vec;
                                let (px, py, pz) =
                                    project(rx - center_x, ry - center_y, rz - center_z);
                                list.push(DrawableAtom {
                                    x: px,
                                    y: py,
                                    z: pz,
                                    r: 5.0,
                                    color: (0.2, 0.2, 0.2, 0.5),
                                    is_void: false,
                                });
                            }
                        }
                    }
                }
            }

            if let Some(res) = &vs.result {
                if res.max_sphere_radius > 0.0 {
                    let vx = res.max_sphere_center[0];
                    let vy = res.max_sphere_center[1];
                    let vz = res.max_sphere_center[2];
                    let (px, py, pz) = project(vx - center_x, vy - center_y, vz - center_z);
                    list.push(DrawableAtom {
                        x: px,
                        y: py,
                        z: pz,
                        r: res.max_sphere_radius * view_scale,
                        color: (0.9, 0.1, 0.1, 0.6),
                        is_void: true,
                    });
                }
            }

            // CRITICAL FIX: Sort DESCENDING (Far to Near) so atoms close to camera overlap atoms behind.
            list.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));

            // Draw Box (Wireframe) - drawn first so it's behind
            cr.set_source_rgb(0.5, 0.5, 0.5);
            cr.set_line_width(1.0);
            let corners = [
                (0., 0., 0.),
                (1., 0., 0.),
                (1., 1., 0.),
                (0., 1., 0.),
                (0., 0., 1.),
                (1., 0., 1.),
                (1., 1., 1.),
                (0., 1., 1.),
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
            for (s, e) in edges {
                let c1 = corners[s];
                let c2 = corners[e];
                let r1x = c1.0 * ax + c1.1 * bx + c1.2 * cx_vec;
                let r1y = c1.0 * ay + c1.1 * by + c1.2 * cy_vec;
                let r1z = c1.0 * az + c1.1 * bz + c1.2 * cz_vec;
                let r2x = c2.0 * ax + c2.1 * bx + c2.2 * cx_vec;
                let r2y = c2.0 * ay + c2.1 * by + c2.2 * cy_vec;
                let r2z = c2.0 * az + c2.1 * bz + c2.2 * cz_vec;
                let (p1x, p1y, _) = project(r1x - center_x, r1y - center_y, r1z - center_z);
                let (p2x, p2y, _) = project(r2x - center_x, r2y - center_y, r2z - center_z);
                cr.move_to(p1x, p1y);
                cr.line_to(p2x, p2y);
                cr.stroke().unwrap();
            }

            for d in list {
                cr.new_path();
                cr.arc(d.x, d.y, d.r, 0.0, 2.0 * PI);
                if d.is_void {
                    cr.set_source_rgba(d.color.0, d.color.1, d.color.2, d.color.3);
                    cr.fill_preserve().unwrap();
                    cr.set_source_rgb(1.0, 0.0, 0.0);
                    cr.stroke().unwrap();
                } else {
                    cr.set_source_rgba(d.color.0, d.color.1, d.color.2, d.color.3);
                    cr.fill().unwrap();
                }
            }
        }
    });

    root
}
