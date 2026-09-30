use crate::model::structure::Structure;
use crate::state::AppState;
use crate::ui::style::{captioned, card_body, group};
use gtk4::prelude::*;
use gtk4::{
    Align, Box, Button, DrawingArea, DropDown, Frame, Grid, Label, Notebook, Orientation,
    PolicyType, ScrolledWindow, Separator, SpinButton, StringList,
};
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

// Import constants and types from Physics
use crate::physics::analysis::interstitial::{self, InterstitialSite};
use crate::physics::analysis::voids::{self, RadiusType, VoidConfig, VoidResult};
use crate::state::InterstitialOverlay;
use crate::utils::console;
use crate::utils::task::{self, JobHandle};
use std::time::Instant;

struct VoidsVisState {
    structure: Option<Structure>,
    result: Option<VoidResult>,
    sites: Vec<InterstitialSite>,
}

struct DrawableAtom {
    x: f64,
    y: f64,
    z: f64,
    r: f64,
    color: (f64, f64, f64, f64),
    is_void: bool,
}

pub fn build(state: Rc<RefCell<AppState>>, main_notebook: &Notebook) -> Box {
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

    let ctrl_box = Box::new(Orientation::Vertical, 8);

    // --- Void fraction card: everything that defines "open space" ---
    let void_body = card_body(8);

    // Row 1: Grid resolution | Radius scale
    let spin_res = SpinButton::with_range(0.1, 2.0, 0.1);
    spin_res.set_value(0.3);
    spin_res.set_width_chars(3);
    let spin_scale = SpinButton::with_range(0.1, 1.5, 0.05);
    spin_scale.set_value(1.0);
    spin_scale.set_width_chars(3);
    let row_a = Box::new(Orientation::Horizontal, 8);
    row_a.set_homogeneous(true);
    row_a.append(&captioned("Grid (pts/Å)", &spin_res));
    row_a.append(&captioned("Radius scale", &spin_scale));
    void_body.append(&row_a);

    // Row 2: Radius type | Probe radius
    // Added "Ionic" as the first option
    let type_model = StringList::new(&["Ionic", "Van der Waals", "Covalent"]);
    let drop_type = DropDown::new(Some(type_model), None::<&gtk4::Expression>);
    drop_type.set_selected(0); // Default to Ionic
    let spin_probe = SpinButton::with_range(0.0, 5.0, 0.05);
    spin_probe.set_value(1.20);
    spin_probe.set_width_chars(3);
    let row_b = Box::new(Orientation::Horizontal, 8);
    row_b.set_homogeneous(true);
    row_b.append(&captioned("Radius type", &drop_type));
    row_b.append(&captioned("Probe radius (Å)", &spin_probe));
    void_body.append(&row_b);

    // Dynamic Probe Buttons (Source: Physics)
    // Three columns, not four: at four the "Geometric" button lands in a
    // column of its own and drags the whole control panel out to 316 px,
    // wider than the other analysis tabs.
    const PROBE_COLS: usize = 3;
    let grid_probes = Grid::builder().row_spacing(5).column_spacing(5).build();
    for (i, (name, rad)) in voids::PRESET_PROBES.iter().enumerate() {
        let btn = Button::with_label(name);
        let sp = spin_probe.clone();
        let r_val = *rad;
        btn.connect_clicked(move |_| sp.set_value(r_val));
        grid_probes.attach(&btn, (i % PROBE_COLS) as i32, (i / PROBE_COLS) as i32, 1, 1);
    }
    void_body.append(&grid_probes);
    ctrl_box.append(&group("Void fraction", &void_body));

    // --- Interstitial search card ---
    //
    // Separate from the probe radius above: the probe decides what counts as
    // open space for the void fraction, while this decides which of the sites
    // found are big enough to report. Keeping them apart means changing the
    // ion does not silently redefine the porosity number next to it.
    let ion_names: Vec<&str> = voids::CANDIDATE_IONS.iter().map(|(n, _)| *n).collect();
    let drop_ion = DropDown::new(
        Some(StringList::new(&ion_names)),
        None::<&gtk4::Expression>,
    );
    drop_ion.set_selected(0); // Li+
    let ion_body = card_body(8);
    ion_body.append(&captioned("Insert ion", &drop_ion));
    ctrl_box.append(&group("Interstitial search", &ion_body));

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
    // max_width_chars matters as much as wrap here: a wrapping Label still
    // reports its *unwrapped* width as natural, so a long candidate list
    // would stretch the control column.
    let val_cand = Label::builder()
        .label("-")
        .halign(Align::Start)
        .wrap(true)
        .max_width_chars(28)
        .build();
    right_pane.append(&val_cand);

    let val_sites = Label::builder()
        .label("Interstitial sites: -")
        .halign(Align::Start)
        .margin_top(10)
        .build();
    val_sites.add_css_class("heading");
    right_pane.append(&val_sites);

    // The list scrolls: on a porous framework it can run to hundreds of rows,
    // which must not push the controls off the panel.
    let site_list = Label::builder()
        .label("")
        .halign(Align::Start)
        .valign(Align::Start)
        .wrap(false)
        .build();
    site_list.add_css_class("monospace");

    // The screen is geometry only, and the caveat goes directly under the
    // heading rather than after the list: a list hundreds of rows long would
    // scroll it out of sight, and this is the line that stops the numbers
    // being read as energetics. Full wording in the tooltip.
    let caveat = Label::builder()
        .label("Geometry only — no electrostatics, no barriers.")
        .tooltip_text(
            "Geometric screen: rigid framework, hard spheres. \
             No electrostatics and no migration barriers.",
        )
        .halign(Align::Start)
        .wrap(true)
        .max_width_chars(28)
        .build();
    caveat.add_css_class("dim-label");
    right_pane.append(&caveat);

    // Horizontal scrolling only, growing to whatever height the list needs:
    // the vertical scrolling is the column's job (below), and two nested
    // vertical scrollers would fight over the wheel.
    let site_scroll = ScrolledWindow::builder()
        .child(&site_list)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .propagate_natural_height(true)
        .build();
    right_pane.append(&site_scroll);

    // The controls scroll as a unit. Without this the column's natural
    // height -- which the site list can push to any value -- becomes the
    // Analysis window's minimum height, and the window grows to fit it.
    let right_scroll = ScrolledWindow::builder()
        .child(&right_pane)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .build();
    right_scroll.set_width_request(super::CONTROL_PANE_WIDTH);
    root.append(&right_scroll);

    // --- INTERACTION LOGIC ---
    let vis_state = Rc::new(RefCell::new(VoidsVisState {
        structure: state.borrow().active_tab().structure.clone(),
        result: None,
        sites: Vec::new(),
    }));
    let state_c = state.clone();
    let vis_c = vis_state.clone();
    let da_c = drawing_area.clone();
    let nb_weak = main_notebook.downgrade();

    // One slot for the in-flight job. Dropping a JobHandle cancels its
    // worker, so assigning here is what stops a superseded run: the user can
    // hammer Calculate and only the newest sweep survives to touch the UI.
    let job: Rc<RefCell<Option<JobHandle>>> = Rc::new(RefCell::new(None));
    let job_c = job.clone();

    btn_calc.connect_clicked(move |btn| {
        // Pin the tab this run belongs to. A sweep takes long enough that the
        // user can switch tabs before it lands, and the overlay has to follow
        // the structure it was computed from, not whatever is on screen when
        // the worker happens to finish.
        let (structure, tab_index) = {
            let st = state_c.borrow();
            match st.active_tab().structure.clone() {
                Some(s) => (s, st.active_tab_index),
                None => return,
            }
        };

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

        // Cancel whatever was running before starting the replacement.
        *job_c.borrow_mut() = None;

        btn.set_sensitive(false);
        btn.set_label("Calculating…");
        val_cand.set_markup("<i>Working…</i>");

        let ion_index = drop_ion.selected() as usize;
        let (ion_name, ion_radius) = voids::CANDIDATE_IONS
            .get(ion_index)
            .copied()
            .unwrap_or(("Li\u{207a}", 0.76));

        // The worker gets its own copy; the UI keeps one to draw against so
        // neither thread has to reach for the other's state.
        let work_structure = structure.clone();

        let btn_done = btn.clone();
        let val_r = val_r.clone();
        let val_d = val_d.clone();
        let val_vol = val_vol.clone();
        let val_cand = val_cand.clone();
        let val_sites = val_sites.clone();
        let site_list = site_list.clone();
        let vis_done = vis_c.clone();
        let da_done = da_c.clone();
        let state_done = Rc::downgrade(&state_c);
        let nb_done = nb_weak.clone();

        let handle = task::spawn(
            move |cancel| {
                let started = Instant::now();
                // One field, both answers — sampling the cell is the whole
                // cost, and running it twice to get the void numbers and the
                // sites separately would double it for nothing.
                match interstitial::screen_structure(
                    &work_structure,
                    config,
                    ion_radius,
                    &cancel,
                ) {
                    Ok(Some((result, sites))) => Some(Ok((result, sites, started.elapsed()))),
                    // Superseded — the UI has already moved on, say nothing.
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                }
            },
            move |outcome| {
                btn_done.set_sensitive(true);
                btn_done.set_label("Calculate");

                match outcome {
                    Ok((result, sites, elapsed)) => {
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
                                val_cand
                                    .set_markup("<span color='orange'>None (Too Small)</span>");
                            } else {
                                val_cand.set_markup(&format!("<b>{}</b>", fits.join(", ")));
                            }
                        } else {
                            val_cand.set_markup("<span color='red'>Overlap Detected</span>");
                        }

                        // --- interstitial sites ---
                        if sites.is_empty() {
                            val_sites.set_markup(&format!(
                                "Interstitial sites: <b>none</b> fit {} ({:.2} Å)",
                                ion_name, ion_radius
                            ));
                            site_list.set_text("");
                        } else {
                            val_sites.set_markup(&format!(
                                "Interstitial sites: <b>{}</b> fit {} ({:.2} Å)",
                                sites.len(),
                                ion_name,
                                ion_radius
                            ));

                            // Sites are sorted largest-first; a porous cell can
                            // yield hundreds, and the tail is all but identical.
                            const SHOWN: usize = 40;
                            let mut text = String::from("   #   radius      a      b      c\n");
                            for (i, s) in sites.iter().take(SHOWN).enumerate() {
                                text.push_str(&format!(
                                    "{:>4}  {:>6.3}  {:>5.3}  {:>5.3}  {:>5.3}\n",
                                    i + 1,
                                    s.radius,
                                    s.frac[0],
                                    s.frac[1],
                                    s.frac[2]
                                ));
                            }
                            if sites.len() > SHOWN {
                                text.push_str(&format!(
                                    "… and {} more\n",
                                    sites.len() - SHOWN
                                ));
                            }
                            site_list.set_text(&text);
                        }

                        console::log_info(&format!(
                            "Void analysis: {}×{}×{} grid ({} points), {} site(s) for {} in {:.0} ms",
                            result.grid_info.nx,
                            result.grid_info.ny,
                            result.grid_info.nz,
                            result.grid_info.total_points,
                            sites.len(),
                            ion_name,
                            elapsed.as_secs_f64() * 1000.0
                        ));

                        // Hand the overlay to the tab so the main 3D view can
                        // draw it, then repaint both views.
                        if let Some(st) = state_done.upgrade() {
                            let mut st = st.borrow_mut();
                            // Bounds-checked: the tab may have been closed
                            // while the sweep was running.
                            if let Some(tab) = st.tabs.get_mut(tab_index) {
                                tab.interstitial = Some(InterstitialOverlay {
                                    ion: ion_name.to_string(),
                                    ion_radius,
                                    sites: sites.clone(),
                                });
                            }
                        }
                        if let Some(nb) = nb_done.upgrade() {
                            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                                da.queue_draw();
                            }
                        }

                        let mut vs = vis_done.borrow_mut();
                        vs.structure = Some(structure);
                        vs.result = Some(result);
                        vs.sites = sites;
                        drop(vs);
                        da_done.queue_draw();
                    }
                    Err(e) => {
                        val_cand.set_markup(&format!("<span color='red'>Error: {}</span>", e));
                        val_r.set_text("-");
                        val_sites.set_text("Interstitial sites: -");
                        site_list.set_text("");
                    }
                }
            },
        );

        *job_c.borrow_mut() = Some(handle);
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

            // Interstitial sites, at their fitted radius so a tight site reads
            // as tight. Teal, to separate them from the red largest-sphere
            // marker above and the grey framework atoms.
            for site in &vs.sites {
                let sx = site.frac[0] * ax + site.frac[1] * bx + site.frac[2] * cx_vec;
                let sy = site.frac[0] * ay + site.frac[1] * by + site.frac[2] * cy_vec;
                let sz = site.frac[0] * az + site.frac[1] * bz + site.frac[2] * cz_vec;
                let (px, py, pz) = project(sx - center_x, sy - center_y, sz - center_z);
                list.push(DrawableAtom {
                    x: px,
                    y: py,
                    z: pz,
                    r: site.radius * view_scale,
                    color: (0.0, 0.72, 0.66, 0.55),
                    is_void: true,
                });
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
