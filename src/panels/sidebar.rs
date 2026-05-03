// src/panels/sidebar.rs
// All features preserved — logging via centralized utils::console

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Align, Box as GtkBox, Button, CheckButton, ColorButton, CssProvider, DropDown,
    Expander, Frame, Label, Notebook, Orientation, PolicyType, Scale, ScrolledWindow, Separator,
    STYLE_PROVIDER_PRIORITY_APPLICATION,
};

use crate::config::ColorMode;
use crate::model::elements::get_element_color;
use crate::state::AppState;
use crate::utils::console;
use std::cell::RefCell;
use std::rc::Rc;

/// Builds the sidebar and returns (The ScrolledWindow, The Atom List Container Box)
pub fn build(state: Rc<RefCell<AppState>>, notebook: &Notebook) -> (ScrolledWindow, GtkBox) {
    // --- 0. INJECT CUSTOM CSS FOR "BOLD LINE" SLIDERS ---
    let provider = CssProvider::new();
    provider.load_from_data(
        "
        scale.thin-slider slider {
            min-width: 6px;
            min-height: 18px;
            margin-top: -7px;
            margin-bottom: -7px;
            border-radius: 2px;
            background-color: #555555;
            box-shadow: none;
            outline: none;
        }
        scale.thin-slider slider:hover {
            background-color: #3584e4;
        }
    ",
    );

    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // 1. Root Container (Scrollable) - FIXED WIDTH
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_width(200)
        .max_content_width(400)
        .build();

    let root_vbox = GtkBox::new(Orientation::Vertical, 10);
    root_vbox.set_margin_start(10);
    root_vbox.set_margin_end(10);
    root_vbox.set_margin_top(10);
    root_vbox.set_margin_bottom(10);
    root_vbox.set_width_request(300);
    root_vbox.set_hexpand(false);
    scroll.set_child(Some(&root_vbox));

    // --- Helper for Sliders (Fixed Snapping & Styling) ---
    let create_slider =
        |label: &str, min: f64, max: f64, step: f64, val: f64, callback: Box<dyn Fn(f64)>| {
            let b = GtkBox::new(Orientation::Vertical, 2);
            b.append(&Label::builder().label(label).halign(Align::Start).build());

            let adj = Adjustment::new(val, min, max, step, step, 0.0);
            let scale = Scale::new(Orientation::Horizontal, Some(&adj));

            scale.add_css_class("thin-slider");
            scale.set_digits(2);
            scale.set_draw_value(true);
            scale.set_value_pos(gtk4::PositionType::Right);

            scale.connect_value_changed(move |sc| {
                let raw = sc.value();
                let snapped = (raw / step).round() * step;

                if (raw - snapped).abs() > 0.0001 {
                    sc.set_value(snapped);
                    return;
                }
                callback(snapped);
            });
            b.append(&scale);
            b
        };

    // ============================================================
    // Create atom list container EARLY
    // ============================================================
    let atoms_list_container = GtkBox::new(Orientation::Vertical, 5);
    atoms_list_container.set_margin_start(5);
    atoms_list_container.set_margin_end(5);
    atoms_list_container.set_margin_top(5);
    atoms_list_container.set_margin_bottom(5);

    // ============================================================
    // SECTION 1: VIEW CONTROLS
    // ============================================================
    let controls_expander = Expander::new(Some("View Controls"));
    controls_expander.set_expanded(false);

    let controls_box = GtkBox::new(Orientation::Vertical, 15);
    controls_box.set_margin_top(10);
    controls_box.set_margin_bottom(10);
    controls_box.set_margin_start(5);

    let nb_weak = notebook.downgrade();

    let queue_active_draw = move |nb_weak: &gtk4::glib::WeakRef<Notebook>| {
        if let Some(nb) = nb_weak.upgrade() {
            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                da.queue_draw();
            }
        }
    };

    // Zoom
    let s_z = state.clone();
    let nb_z = nb_weak.clone();
    let cb_z = queue_active_draw;
    controls_box.append(&create_slider(
        "Zoom",
        0.1,
        5.0,
        0.1,
        state.borrow().active_tab().view.zoom,
        Box::new(move |v| {
            s_z.borrow_mut().active_tab_mut().view.zoom = v;
            cb_z(&nb_z);
        }),
    ));

    // Rotation sliders set absolute Euler angles (XYZ-intrinsic). Each callback
    // decomposes the current quaternion, replaces one component, and recomposes.
    // Sliders don't auto-track mouse-drag changes (slider widgets aren't reactive
    // to state changes); that mirrors prior behavior.
    let (init_rx, init_ry, init_rz) = state.borrow().active_tab().view.euler_xyz_deg();

    let s_rx = state.clone();
    let nb_rx = nb_weak.clone();
    let cb_rx = queue_active_draw;
    controls_box.append(&create_slider(
        "Rotation X",
        -180.0,
        180.0,
        1.0,
        init_rx,
        Box::new(move |v| {
            let mut st = s_rx.borrow_mut();
            let view = &mut st.active_tab_mut().view;
            let (_, ry, rz) = view.euler_xyz_deg();
            view.set_euler_xyz_deg(v, ry, rz);
            drop(st);
            cb_rx(&nb_rx);
        }),
    ));

    let s_ry = state.clone();
    let nb_ry = nb_weak.clone();
    let cb_ry = queue_active_draw;
    controls_box.append(&create_slider(
        "Rotation Y",
        -180.0,
        180.0,
        1.0,
        init_ry,
        Box::new(move |v| {
            let mut st = s_ry.borrow_mut();
            let view = &mut st.active_tab_mut().view;
            let (rx, _, rz) = view.euler_xyz_deg();
            view.set_euler_xyz_deg(rx, v, rz);
            drop(st);
            cb_ry(&nb_ry);
        }),
    ));

    let s_rz = state.clone();
    let nb_rz = nb_weak.clone();
    let cb_rz = queue_active_draw;
    controls_box.append(&create_slider(
        "Rotation Z",
        -180.0,
        180.0,
        1.0,
        init_rz,
        Box::new(move |v| {
            let mut st = s_rz.borrow_mut();
            let view = &mut st.active_tab_mut().view;
            let (rx, ry, _) = view.euler_xyz_deg();
            view.set_euler_xyz_deg(rx, ry, v);
            drop(st);
            cb_rz(&nb_rz);
        }),
    ));

    controls_expander.set_child(Some(&controls_box));
    // The View Controls panel is appended below (after Appearance) so the
    // sidebar reads Appearance → View Controls → Bond Valence top-down.

    // ============================================================
    // SECTION 2: APPEARANCE
    // ============================================================
    let style_expander = Expander::new(Some("Appearance"));
    style_expander.set_expanded(true);

    let style_box = GtkBox::new(Orientation::Vertical, 15);
    style_box.set_margin_top(10);
    style_box.set_margin_bottom(10);
    style_box.set_margin_start(5);

    // --- MATERIAL ---
    let frame_mat = Frame::new(Some("Material"));
    let vbox_mat = GtkBox::new(Orientation::Vertical, 10);
    vbox_mat.set_margin_top(10);
    vbox_mat.set_margin_bottom(10);
    vbox_mat.set_margin_start(10);
    vbox_mat.set_margin_end(10);

    // Metallic
    let s_met = state.clone();
    let nb_met = nb_weak.clone();
    let cb_met = queue_active_draw;
    vbox_mat.append(&create_slider(
        "Metallic",
        0.0,
        1.0,
        0.05,
        state.borrow().active_tab().style.metallic,
        Box::new(move |v| {
            let mut st = s_met.borrow_mut();
            let tab = st.active_tab_mut();
            tab.style.metallic = v;
            tab.style.atom_cache.borrow_mut().clear();
            cb_met(&nb_met);
        }),
    ));

    // Roughness
    let s_rgh = state.clone();
    let nb_rgh = nb_weak.clone();
    let cb_rgh = queue_active_draw;
    vbox_mat.append(&create_slider(
        "Roughness",
        0.0,
        1.0,
        0.05,
        state.borrow().active_tab().style.roughness,
        Box::new(move |v| {
            let mut st = s_rgh.borrow_mut();
            let tab = st.active_tab_mut();
            tab.style.roughness = v;
            tab.style.atom_cache.borrow_mut().clear();
            cb_rgh(&nb_rgh);
        }),
    ));

    // Transmission
    let s_tr = state.clone();
    let nb_tr = nb_weak.clone();
    let cb_tr = queue_active_draw;
    vbox_mat.append(&create_slider(
        "Transmission",
        0.0,
        1.0,
        0.05,
        state.borrow().active_tab().style.transmission,
        Box::new(move |v| {
            let mut st = s_tr.borrow_mut();
            let tab = st.active_tab_mut();
            tab.style.transmission = v;
            tab.style.atom_cache.borrow_mut().clear();
            cb_tr(&nb_tr);
        }),
    ));

    frame_mat.set_child(Some(&vbox_mat));
    style_box.append(&frame_mat);

    // --- ATOM SIZE ---
    let frame_atom = Frame::new(Some("Atom Size"));
    let vbox_atom = GtkBox::new(Orientation::Vertical, 10);
    vbox_atom.set_margin_top(10);
    vbox_atom.set_margin_bottom(10);
    vbox_atom.set_margin_start(10);
    vbox_atom.set_margin_end(10);

    let s_as = state.clone();
    let nb_as = nb_weak.clone();
    let cb_as = queue_active_draw;
    vbox_atom.append(&create_slider(
        "Scale",
        0.1,
        1.5,
        0.05,
        state.borrow().active_tab().style.atom_scale,
        Box::new(move |v| {
            s_as.borrow_mut().active_tab_mut().style.atom_scale = v;
            cb_as(&nb_as);
        }),
    ));

    // --- Show Labels Toggle ---
    let check_labels = CheckButton::with_label("Show Atomic Symbols");
    // Set initial state
    check_labels.set_active(state.borrow().active_tab().style.show_labels);

    let s_lbl = state.clone();
    let nb_lbl = nb_weak.clone();
    let cb_lbl = queue_active_draw;

    check_labels.connect_toggled(move |btn| {
        let mut st = s_lbl.borrow_mut();
        st.active_tab_mut().style.show_labels = btn.is_active();
        drop(st);
        cb_lbl(&nb_lbl);
    });
    vbox_atom.append(&check_labels);
    frame_atom.set_child(Some(&vbox_atom));
    style_box.append(&frame_atom);

    // --- ELEMENT COLORS ---
    let frame_elem = Frame::new(Some("Element Colors"));
    frame_elem.set_child(Some(&atoms_list_container));
    style_box.append(&frame_elem);

    // --- BONDS ---
    let frame_bonds = Frame::new(Some("Bonds"));
    let vbox_bonds = GtkBox::new(Orientation::Vertical, 10);
    vbox_bonds.set_margin_top(10);
    vbox_bonds.set_margin_bottom(10);
    vbox_bonds.set_margin_start(10);
    vbox_bonds.set_margin_end(10);

    // Bond Radius
    let s_br = state.clone();
    let nb_br = nb_weak.clone();
    let cb_br = queue_active_draw;
    vbox_bonds.append(&create_slider(
        "Radius",
        0.01,
        0.5,
        0.01,
        state.borrow().active_tab().style.bond_radius,
        Box::new(move |v| {
            s_br.borrow_mut().active_tab_mut().style.bond_radius = v;
            cb_br(&nb_br);
        }),
    ));

    // Bond Tolerance ← RESTORED
    let s_tol = state.clone();
    let nb_tol = nb_weak.clone();
    let cb_tol = queue_active_draw;
    vbox_bonds.append(&create_slider(
        "Tolerance",
        0.6,
        1.6,
        0.05,
        state.borrow().active_tab().view.bond_cutoff,
        Box::new(move |v| {
            s_tol.borrow_mut().active_tab_mut().view.bond_cutoff = v;
            cb_tol(&nb_tol);
        }),
    ));

    // Bond Color
    let box_bcol = GtkBox::new(Orientation::Horizontal, 10);
    box_bcol.append(&Label::new(Some("Color:")));

    let current_bc = {
        let st = state.borrow();
        let c = st.active_tab().style.bond_color;
        gdk::RGBA::new(c.0 as f32, c.1 as f32, c.2 as f32, 1.0)
    };
    let btn_bcol = ColorButton::new();
    btn_bcol.set_rgba(&current_bc);

    let s_bc = state.clone();
    let nb_bc = nb_weak.clone();
    let cb_bc = queue_active_draw;
    btn_bcol.connect_color_set(move |b| {
        let c = b.rgba();
        s_bc.borrow_mut().active_tab_mut().style.bond_color =
            (c.red() as f64, c.green() as f64, c.blue() as f64);
        cb_bc(&nb_bc);
    });
    box_bcol.append(&btn_bcol);
    vbox_bonds.append(&box_bcol);

    frame_bonds.set_child(Some(&vbox_bonds));
    style_box.append(&frame_bonds);

    style_expander.set_child(Some(&style_box));
    root_vbox.append(&style_expander);
    root_vbox.append(&controls_expander);

    // ============================================================
    // SECTION 3: BOND VALENCE
    // ============================================================
    let bvs_expander = Expander::new(Some("Bond Valence"));
    bvs_expander.set_expanded(false);

    let bvs_box = GtkBox::new(Orientation::Vertical, 10);
    bvs_box.set_margin_top(10);
    bvs_box.set_margin_bottom(10);
    bvs_box.set_margin_start(10);
    bvs_box.set_margin_end(10);

    // Color Mode Selector
    let mode_row = GtkBox::new(Orientation::Horizontal, 10);
    mode_row.append(&Label::new(Some("Color Mode:")));

    let mode_dropdown = DropDown::from_strings(&["Element Colors", "Bond Valence"]);

    // Set initial selection based on current mode
    mode_dropdown.set_selected(match state.borrow().active_tab().style.color_mode {
        ColorMode::Element => 0,
        ColorMode::BondValence => 1,
        _ => 0,
    });

    mode_dropdown.set_hexpand(true);

    let state_mode = state.clone();
    let nb_mode = nb_weak.clone();

    mode_dropdown.connect_selected_notify(move |dd| {
        let mode = match dd.selected() {
            0 => ColorMode::Element,
            1 => ColorMode::BondValence,
            _ => ColorMode::Element,
        };

        let mut st = state_mode.borrow_mut();
        let tab = st.active_tab_mut();
        tab.style.color_mode = mode;

        // Recalculate BVS if switching to BVS mode
        if matches!(mode, ColorMode::BondValence) {
            tab.invalidate_bvs_cache();
            let _ = tab.get_bvs_values();

            // Show BVS report in Structure Info tab
            if let Some(ref structure) = tab.structure {
                use crate::utils::report;
                let report_text = report::bvs_analysis(structure);
                console::info_report(&report_text);
            }
        }

        // Redraw
        if let Some(nb) = nb_mode.upgrade() {
            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                da.queue_draw();
            }
        }
    });

    mode_row.append(&mode_dropdown);
    bvs_box.append(&mode_row);

    bvs_box.append(&Separator::new(Orientation::Horizontal));

    // The two BVS-coloring thresholds must satisfy `warn ≥ good + MIN_GAP` —
    // otherwise the gradient zone (good→warn) inverts and the painter shows
    // contradictory colors. We enforce this by linking the two scales: each
    // callback nudges the other when the user crosses the boundary.
    const STEP: f64 = 0.01;
    const MIN_GAP: f64 = 0.05;

    let mk_threshold_row =
        |label: &str, min: f64, max: f64, val: f64| -> (GtkBox, Scale) {
            let b = GtkBox::new(Orientation::Vertical, 2);
            b.append(&Label::builder().label(label).halign(Align::Start).build());
            let adj = Adjustment::new(val, min, max, STEP, STEP, 0.0);
            let scale = Scale::new(Orientation::Horizontal, Some(&adj));
            scale.add_css_class("thin-slider");
            scale.set_digits(2);
            scale.set_draw_value(true);
            scale.set_value_pos(gtk4::PositionType::Right);
            b.append(&scale);
            (b, scale)
        };

    let (good_row, good_scale) = mk_threshold_row(
        "Good Threshold",
        0.05,
        0.30,
        state.borrow().active_tab().style.bvs_threshold_good,
    );
    let (warn_row, warn_scale) = mk_threshold_row(
        "Warning Threshold",
        0.20,
        0.60,
        state.borrow().active_tab().style.bvs_threshold_warn,
    );

    let s_good = state.clone();
    let nb_good = nb_weak.clone();
    let warn_scale_w = warn_scale.downgrade();
    good_scale.connect_value_changed(move |sc| {
        let raw = sc.value();
        let snapped = (raw / STEP).round() * STEP;
        if (raw - snapped).abs() > 0.0001 {
            sc.set_value(snapped);
            return;
        }
        // Enforce warn ≥ good + MIN_GAP by lifting warn if needed.
        if let Some(warn_sc) = warn_scale_w.upgrade() {
            if warn_sc.value() < snapped + MIN_GAP {
                warn_sc.set_value(snapped + MIN_GAP);
                // The warn callback will fire and persist its own state.
            }
        }
        let mut st = s_good.borrow_mut();
        st.active_tab_mut().style.bvs_threshold_good = snapped;
        drop(st);
        queue_active_draw(&nb_good);
    });

    let s_warn = state.clone();
    let nb_warn = nb_weak.clone();
    let good_scale_w = good_scale.downgrade();
    warn_scale.connect_value_changed(move |sc| {
        let raw = sc.value();
        let snapped = (raw / STEP).round() * STEP;
        if (raw - snapped).abs() > 0.0001 {
            sc.set_value(snapped);
            return;
        }
        // Enforce warn ≥ good + MIN_GAP by lowering good if needed.
        if let Some(good_sc) = good_scale_w.upgrade() {
            if good_sc.value() > snapped - MIN_GAP {
                good_sc.set_value(snapped - MIN_GAP);
            }
        }
        let mut st = s_warn.borrow_mut();
        st.active_tab_mut().style.bvs_threshold_warn = snapped;
        drop(st);
        queue_active_draw(&nb_warn);
    });

    bvs_box.append(&good_row);
    bvs_box.append(&warn_row);

    // Help Text
    // let help_text = Label::new(Some(
    // "💡 Load a structure (e.g., Li₂O, NaCl)\n\n\
    // 🟢 Green = Good match\n\
    // 🟡 Yellow = Warning\n\
    // 🔴 Red = Poor match\n\n\
    // Tip: Adjust thresholds to change\n\
    // color sensitivity.",
    // ));
    // help_text.set_wrap(true);
    // help_text.set_justify(gtk4::Justification::Left);
    // help_text.set_opacity(0.7);
    // help_text.set_margin_top(10);
    // bvs_box.append(&help_text);

    // Show BVS Report Button
    // bvs_box.append(&Separator::new(Orientation::Horizontal));

    // let btn_bvs_report = Button::with_label("📊 Show BVS Report");
    // btn_bvs_report.set_margin_top(10);
    // btn_bvs_report.set_tooltip_text(Some("Show detailed BVS analysis in Interactions tab"));

    // let state_btn = state.clone();
    // BVS report button (disabled; reports are shown automatically on mode switch)
    // btn_bvs_report.connect_clicked(move |_| {
    //     use crate::utils::report;
    //     let st = state_btn.borrow();
    //     let tab = st.active_tab();
    //     if let Some(ref structure) = tab.structure {
    //         let report_text = report::bvs_analysis(structure);
    //         console::info_report(&report_text);
    //     }
    // });

    // bvs_box.append(&btn_bvs_report);

    bvs_expander.set_child(Some(&bvs_box));
    root_vbox.append(&bvs_expander);

    (scroll, atoms_list_container)
}

/// Public helper to rebuild the list of atom colors dynamically.
/// SOTA: adds CN label on poly checkbox, transparency slider per element,
/// and an "Auto-detect Polyhedra" button at the top.
pub fn refresh_atom_list(container: &GtkBox, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    // Gather scene atoms for CN computation (need rendered atoms, not just structure)
    // We use a dummy scene call here — same pattern as the old basis_dlg before our fix,
    // but here it's fine because it's called once on refresh, not on every click.
    let (scene_atoms, bond_cutoff, poly_max_bond_dist) = {
        let st = state.borrow();
        let tab = st.active_tab();
        let cutoff = tab.view.bond_cutoff;
        let max_dist = tab
            .style
            .polyhedra_settings
            .as_ref()
            .map(|ps| ps.max_bond_dist)
            .unwrap_or(3.5);
        let (atoms, _, _) = crate::rendering::scene::calculate_scene(
            tab, &st.config, 800.0, 600.0, false, None, None,
        );
        (atoms, cutoff, max_dist)
    };

    let elements = if let Some(structure) = &state.borrow().active_tab().structure {
        let mut unique: Vec<String> = structure.atoms.iter().map(|a| a.element.clone()).collect();
        unique.sort();
        unique.dedup();
        unique
    } else {
        vec![]
    };

    if elements.is_empty() {
        let lbl = Label::new(Some("(Load file to see elements)"));
        lbl.set_opacity(0.6);
        container.append(&lbl);
        return;
    }

    let nb_weak = notebook.downgrade();

    // ── Auto-detect button ───────────────────────────────────────────────────
    let btn_auto = Button::with_label("Auto-detect Polyhedra");
    btn_auto.set_tooltip_text(Some(
        "Automatically enable polyhedra for elements with average CN 4–8",
    ));
    btn_auto.set_margin_bottom(6);

    let s_auto = state.clone();
    let nb_auto = nb_weak.clone();
    let scene_atoms_auto = scene_atoms.clone();
    let cutoff_auto = bond_cutoff;
    let max_dist_auto = poly_max_bond_dist;

    btn_auto.connect_clicked(move |_| {
        use crate::rendering::polyhedra::auto_detect_polyhedra_elements;
        let detected =
            auto_detect_polyhedra_elements(&scene_atoms_auto, cutoff_auto, max_dist_auto);

        let mut st = s_auto.borrow_mut();
        let tab = st.active_tab_mut();

        if tab.style.polyhedra_settings.is_none() {
            tab.style.polyhedra_settings = Some(crate::config::PolyhedraSettings::default());
        }
        let settings = tab.style.polyhedra_settings.as_mut().unwrap();
        settings.enabled_elements = detected;
        settings.show_polyhedra = !settings.enabled_elements.is_empty();

        drop(st);
        if let Some(nb) = nb_auto.upgrade() {
            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                da.queue_draw();
            }
        }
    });
    container.append(&btn_auto);

    // ── Transparency slider (global, shown once) ─────────────────────────────
    {
        let trans_row = GtkBox::new(Orientation::Horizontal, 8);
        trans_row.append(
            &Label::builder()
                .label("Poly Transparency:")
                .halign(Align::Start)
                .build(),
        );

        let current_trans = state
            .borrow()
            .active_tab()
            .style
            .polyhedra_settings
            .as_ref()
            .map(|ps| ps.transparency)
            .unwrap_or(0.3);

        let adj = gtk4::Adjustment::new(current_trans, 0.05, 0.95, 0.05, 0.05, 0.0);
        let trans_scale = gtk4::Scale::new(Orientation::Horizontal, Some(&adj));
        trans_scale.set_digits(2);
        trans_scale.set_draw_value(true);
        trans_scale.set_value_pos(gtk4::PositionType::Right);
        trans_scale.set_hexpand(true);
        trans_scale.add_css_class("thin-slider");

        let s_tr = state.clone();
        let nb_tr = nb_weak.clone();
        trans_scale.connect_value_changed(move |sc| {
            let v = sc.value();
            let mut st = s_tr.borrow_mut();
            let tab = st.active_tab_mut();
            if tab.style.polyhedra_settings.is_none() {
                tab.style.polyhedra_settings = Some(crate::config::PolyhedraSettings::default());
            }
            tab.style.polyhedra_settings.as_mut().unwrap().transparency = v;
            drop(st);
            if let Some(nb) = nb_tr.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });
        trans_row.append(&trans_scale);
        container.append(&trans_row);
    }

    // ── Polyhedra color picker ─────────────────────────────────────────────────
    {
        let color_row = GtkBox::new(Orientation::Horizontal, 8);
        color_row.append(
            &Label::builder()
                .label("Poly Color:")
                .halign(Align::Start)
                .build(),
        );

        // Current custom color (if any)
        let current_custom = state
            .borrow()
            .active_tab()
            .style
            .polyhedra_settings
            .as_ref()
            .and_then(|ps| {
                if let crate::config::PolyhedraColorMode::Custom(r, g, b) = ps.color_mode {
                    Some((r, g, b))
                } else {
                    None
                }
            });

        let is_element_mode = current_custom.is_none();

        // "Use element color" checkbox
        let check_elem_color = gtk4::CheckButton::with_label("Element");
        check_elem_color.set_active(is_element_mode);

        let btn_poly_color = ColorButton::new();
        let default_poly = current_custom.unwrap_or((0.3, 0.6, 0.9));
        btn_poly_color.set_rgba(&gdk::RGBA::new(
            default_poly.0 as f32,
            default_poly.1 as f32,
            default_poly.2 as f32,
            1.0,
        ));
        btn_poly_color.set_sensitive(!is_element_mode);

        let s_pc = state.clone();
        let nb_pc = nb_weak.clone();
        btn_poly_color.connect_color_set(move |b| {
            let c = b.rgba();
            let mut st = s_pc.borrow_mut();
            let tab = st.active_tab_mut();
            if tab.style.polyhedra_settings.is_none() {
                tab.style.polyhedra_settings = Some(crate::config::PolyhedraSettings::default());
            }
            tab.style.polyhedra_settings.as_mut().unwrap().color_mode =
                crate::config::PolyhedraColorMode::Custom(
                    c.red() as f64,
                    c.green() as f64,
                    c.blue() as f64,
                );
            drop(st);
            if let Some(nb) = nb_pc.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });

        let s_ec = state.clone();
        let nb_ec = nb_weak.clone();
        let btn_ref_ec = btn_poly_color.clone();
        check_elem_color.connect_toggled(move |c| {
            let use_element = c.is_active();
            btn_ref_ec.set_sensitive(!use_element);
            let mut st = s_ec.borrow_mut();
            let tab = st.active_tab_mut();
            if tab.style.polyhedra_settings.is_none() {
                tab.style.polyhedra_settings = Some(crate::config::PolyhedraSettings::default());
            }
            let settings = tab.style.polyhedra_settings.as_mut().unwrap();
            if use_element {
                settings.color_mode = crate::config::PolyhedraColorMode::Element;
            } else {
                let rgba = btn_ref_ec.rgba();
                settings.color_mode = crate::config::PolyhedraColorMode::Custom(
                    rgba.red() as f64,
                    rgba.green() as f64,
                    rgba.blue() as f64,
                );
            }
            drop(st);
            if let Some(nb) = nb_ec.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });

        color_row.append(&check_elem_color);
        color_row.append(&btn_poly_color);
        container.append(&color_row);
    }

    // ── Max bond distance slider ──────────────────────────────────────────────
    {
        let dist_row = GtkBox::new(Orientation::Horizontal, 8);
        dist_row.append(
            &Label::builder()
                .label("Bond Range (Å):")
                .halign(Align::Start)
                .build(),
        );

        let current_dist = state
            .borrow()
            .active_tab()
            .style
            .polyhedra_settings
            .as_ref()
            .map(|ps| ps.max_bond_dist)
            .unwrap_or(3.5);

        let adj = gtk4::Adjustment::new(current_dist, 1.5, 6.0, 0.1, 0.1, 0.0);
        let dist_scale = gtk4::Scale::new(Orientation::Horizontal, Some(&adj));
        dist_scale.set_digits(1);
        dist_scale.set_draw_value(true);
        dist_scale.set_value_pos(gtk4::PositionType::Right);
        dist_scale.set_hexpand(true);
        dist_scale.add_css_class("thin-slider");

        let s_dist = state.clone();
        let nb_dist = nb_weak.clone();
        dist_scale.connect_value_changed(move |sc| {
            let v = sc.value();
            let mut st = s_dist.borrow_mut();
            let tab = st.active_tab_mut();
            if tab.style.polyhedra_settings.is_none() {
                tab.style.polyhedra_settings = Some(crate::config::PolyhedraSettings::default());
            }
            tab.style.polyhedra_settings.as_mut().unwrap().max_bond_dist = v;
            drop(st);
            if let Some(nb) = nb_dist.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });
        dist_row.append(&dist_scale);
        container.append(&dist_row);
        container.append(&Separator::new(Orientation::Horizontal));
    }

    // ── Per-element rows ─────────────────────────────────────────────────────
    for elem in elements {
        // Outer column: color row + poly row stacked vertically
        let col = GtkBox::new(Orientation::Vertical, 2);
        col.set_margin_bottom(4);

        // --- Color row ---
        let row = GtkBox::new(Orientation::Horizontal, 10);

        let lbl = Label::new(Some(&elem));
        lbl.set_width_chars(3);
        lbl.set_xalign(0.0);
        row.append(&lbl);

        let current_color = {
            let st = state.borrow();
            let tab = st.active_tab();
            if let Some(c) = tab.style.element_colors.get(&elem) {
                *c
            } else {
                get_element_color(&elem, st.config.color_scheme)
            }
        };

        let btn = gtk4::ColorButton::new();
        btn.set_rgba(&gdk::RGBA::new(
            current_color.0 as f32,
            current_color.1 as f32,
            current_color.2 as f32,
            1.0,
        ));

        let s = state.clone();
        let nb_inner = nb_weak.clone();
        let elem_key = elem.clone();
        btn.connect_color_set(move |b| {
            let c = b.rgba();
            let mut st = s.borrow_mut();
            let tab = st.active_tab_mut();
            tab.style.element_colors.insert(
                elem_key.clone(),
                (c.red() as f64, c.green() as f64, c.blue() as f64),
            );
            let e = elem_key.clone();
            tab.style
                .atom_cache
                .borrow_mut()
                .clear_matching(|key| key.starts_with(&format!("{}_", e)));
            if let Some(nb) = nb_inner.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });
        row.append(&btn);

        let btn_reset = Button::with_label("↺");
        let s_r = state.clone();
        let nb_r = nb_weak.clone();
        let elem_key_r = elem.clone();
        let btn_ref = btn.clone();
        btn_reset.connect_clicked(move |_| {
            let mut st = s_r.borrow_mut();
            let scheme = st.config.color_scheme;
            let tab = st.active_tab_mut();
            tab.style.element_colors.remove(&elem_key_r);
            let e = elem_key_r.clone();
            tab.style
                .atom_cache
                .borrow_mut()
                .clear_matching(|key| key.starts_with(&format!("{}_", e)));
            let def = get_element_color(&elem_key_r, scheme);
            btn_ref.set_rgba(&gdk::RGBA::new(
                def.0 as f32,
                def.1 as f32,
                def.2 as f32,
                1.0,
            ));
            if let Some(nb) = nb_r.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });
        row.append(&btn_reset);
        col.append(&row);

        // --- Polyhedra row ---
        let poly_row = GtkBox::new(Orientation::Horizontal, 6);
        poly_row.set_margin_start(3);

        // CN label
        let cn_label = {
            use crate::rendering::polyhedra::average_cn_for_element;
            match average_cn_for_element(&scene_atoms, &elem, bond_cutoff, poly_max_bond_dist) {
                Some(avg) => Label::new(Some(&format!("CN≈{:.0}", avg))),
                None => Label::new(Some("CN=?")),
            }
        };
        cn_label.set_opacity(0.6);
        cn_label.set_width_chars(6);

        // Polyhedra checkbox
        let is_poly_active = state
            .borrow()
            .active_tab()
            .style
            .polyhedra_settings
            .as_ref()
            .map(|ps| ps.enabled_elements.contains(&elem))
            .unwrap_or(false);

        let check_poly = gtk4::CheckButton::with_label("Polyhedra");
        check_poly.set_active(is_poly_active);

        let s_poly = state.clone();
        let elem_poly = elem.clone();
        let nb_poly = nb_weak.clone();
        check_poly.connect_toggled(move |c| {
            let mut st = s_poly.borrow_mut();
            let tab = st.active_tab_mut();
            if tab.style.polyhedra_settings.is_none() {
                tab.style.polyhedra_settings = Some(crate::config::PolyhedraSettings::default());
            }
            let settings = tab.style.polyhedra_settings.as_mut().unwrap();
            if c.is_active() {
                if !settings.enabled_elements.contains(&elem_poly) {
                    settings.enabled_elements.push(elem_poly.clone());
                }
                settings.show_polyhedra = true;
            } else {
                settings.enabled_elements.retain(|e| e != &elem_poly);
                if settings.enabled_elements.is_empty() {
                    settings.show_polyhedra = false;
                }
            }
            drop(st);
            if let Some(nb) = nb_poly.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
        });

        poly_row.append(&cn_label);
        poly_row.append(&check_poly);
        col.append(&poly_row);

        container.append(&col);
    }
}
