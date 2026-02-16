// src/panels/sidebar.rs
// COMPLETE VERSION - All features preserved + TextView integration for BVS reports

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Align, Box as GtkBox, Button, ColorButton, CssProvider, DropDown, Expander, Frame,
    Label, Notebook, Orientation, PolicyType, Scale, ScrolledWindow, Separator, TextView,
    STYLE_PROVIDER_PRIORITY_APPLICATION,
};

use crate::config::ColorMode;
use crate::model::elements::get_atom_properties;
use crate::state::AppState;
use std::cell::RefCell;
use std::rc::Rc;

// Helper to log to TextView (matches main.rs and actions_file.rs pattern)
fn log_msg(view: &TextView, text: &str) {
    let buffer = view.buffer();
    let mut end = buffer.end_iter();
    buffer.insert(&mut end, &format!("{}\n", text));
    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
    view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}

/// Builds the sidebar and returns (The ScrolledWindow, The Atom List Container Box)
/// UPDATED: Now accepts TextView references for interactions and system log
pub fn build(
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,
    interactions_view: &TextView,
    system_log_view: &TextView,
) -> (ScrolledWindow, GtkBox) {
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
    controls_expander.set_expanded(true);

    let controls_box = GtkBox::new(Orientation::Vertical, 15);
    controls_box.set_margin_top(10);
    controls_box.set_margin_bottom(10);
    controls_box.set_margin_start(5);

    let nb_weak = notebook.downgrade();
    let interact_weak = interactions_view.downgrade();
    let _syslog_weak = system_log_view.downgrade();

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
    let cb_z = queue_active_draw.clone();
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

    // Rotation X
    let s_rx = state.clone();
    let nb_rx = nb_weak.clone();
    let cb_rx = queue_active_draw.clone();
    controls_box.append(&create_slider(
        "Rotation X",
        0.0,
        360.0,
        1.0,
        state.borrow().active_tab().view.rot_x,
        Box::new(move |v| {
            s_rx.borrow_mut().active_tab_mut().view.rot_x = v;
            cb_rx(&nb_rx);
        }),
    ));

    // Rotation Y
    let s_ry = state.clone();
    let nb_ry = nb_weak.clone();
    let cb_ry = queue_active_draw.clone();
    controls_box.append(&create_slider(
        "Rotation Y",
        0.0,
        360.0,
        1.0,
        state.borrow().active_tab().view.rot_y,
        Box::new(move |v| {
            s_ry.borrow_mut().active_tab_mut().view.rot_y = v;
            cb_ry(&nb_ry);
        }),
    ));

    // Rotation Z
    let s_rz = state.clone();
    let nb_rz = nb_weak.clone();
    let cb_rz = queue_active_draw.clone();
    controls_box.append(&create_slider(
        "Rotation Z",
        0.0,
        360.0,
        1.0,
        state.borrow().active_tab().view.rot_z,
        Box::new(move |v| {
            s_rz.borrow_mut().active_tab_mut().view.rot_z = v;
            cb_rz(&nb_rz);
        }),
    ));

    controls_expander.set_child(Some(&controls_box));
    root_vbox.append(&controls_expander);

    // ============================================================
    // SECTION 2: APPEARANCE
    // ============================================================
    let style_expander = Expander::new(Some("Appearance"));
    style_expander.set_expanded(false);

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
    let cb_met = queue_active_draw.clone();
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
    let cb_rgh = queue_active_draw.clone();
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
    let cb_tr = queue_active_draw.clone();
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
    let cb_as = queue_active_draw.clone();
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
    let cb_br = queue_active_draw.clone();
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
    let cb_tol = queue_active_draw.clone();
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
    let cb_bc = queue_active_draw.clone();
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
    let interact_mode = interact_weak.clone();

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

            // Show BVS report in Interactions tab
            if let Some(ref structure) = tab.structure {
                use crate::utils::report;
                let report_text = report::bvs_analysis(structure);

                if let Some(iv) = interact_mode.upgrade() {
                    log_msg(&iv, &report_text);
                }
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

    // Good Threshold Slider
    let s_good = state.clone();
    let nb_good = nb_weak.clone();
    let cb_good = queue_active_draw.clone();
    bvs_box.append(&create_slider(
        "Good Threshold",
        0.05,
        0.30,
        0.01,
        state.borrow().active_tab().style.bvs_threshold_good,
        Box::new(move |v| {
            s_good
                .borrow_mut()
                .active_tab_mut()
                .style
                .bvs_threshold_good = v;
            cb_good(&nb_good);
        }),
    ));

    // Warning Threshold Slider
    let s_warn = state.clone();
    let nb_warn = nb_weak.clone();
    let cb_warn = queue_active_draw.clone();
    bvs_box.append(&create_slider(
        "Warning Threshold",
        0.20,
        0.60,
        0.01,
        state.borrow().active_tab().style.bvs_threshold_warn,
        Box::new(move |v| {
            s_warn
                .borrow_mut()
                .active_tab_mut()
                .style
                .bvs_threshold_warn = v;
            cb_warn(&nb_warn);
        }),
    ));

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
    // let interact_btn = interact_weak.clone();

    // btn_bvs_report.connect_clicked(move |_| {
    // use crate::utils::report;

    // let st = state_btn.borrow();
    // let tab = st.active_tab();

    // if let Some(ref structure) = tab.structure {
    // let report_text = report::bvs_analysis(structure);

    // if let Some(iv) = interact_btn.upgrade() {
    // log_msg(&iv, &report_text);
    // }
    // }
    // });

    // bvs_box.append(&btn_bvs_report);

    bvs_expander.set_child(Some(&bvs_box));
    root_vbox.append(&bvs_expander);

    (scroll, atoms_list_container)
}

/// Public helper to rebuild the list of atom colors dynamically
pub fn refresh_atom_list(container: &GtkBox, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

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
    } else {
        let nb_weak = notebook.downgrade();

        for elem in elements {
            let row = GtkBox::new(Orientation::Horizontal, 10);

            let lbl = Label::new(Some(&format!("{}", elem)));
            lbl.set_width_chars(3);
            lbl.set_xalign(0.0);
            row.append(&lbl);

            let current_color = {
                let st = state.borrow();
                let tab = st.active_tab();
                if let Some(c) = tab.style.element_colors.get(&elem) {
                    *c
                } else {
                    let (_, def) = get_atom_properties(&elem);
                    def
                }
            };

            let btn = ColorButton::new();
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
                // tab.style.atom_cache.borrow_mut().remove(&elem_key);
                let elem = elem_key.clone();
                tab.style
                    .atom_cache
                    .borrow_mut()
                    .clear_matching(|key| key.starts_with(&format!("{}_", elem)));
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
                let tab = st.active_tab_mut();

                tab.style.element_colors.remove(&elem_key_r);
                // tab.style.atom_cache.borrow_mut().remove(&elem_key_r);
                let elem = elem_key_r.clone();
                tab.style
                    .atom_cache
                    .borrow_mut()
                    .clear_matching(|key| key.starts_with(&format!("{}_", elem)));

                let (_, def) = get_atom_properties(&elem_key_r);
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

            // Polyhedra checkbox
            let check_poly = gtk4::CheckButton::with_label("📐 Poly");
            check_poly.set_active({
                let st = state.borrow();
                st.active_tab()
                    .style
                    .polyhedra_settings
                    .as_ref()
                    .map(|ps| ps.enabled_elements.contains(&elem))
                    .unwrap_or(false)
            });

            let s_poly = state.clone();
            let elem_poly = elem.clone();
            let nb_poly = nb_weak.clone();
            check_poly.connect_toggled(move |c: &gtk4::CheckButton| {
                let mut st = s_poly.borrow_mut();
                let tab = st.active_tab_mut();

                if tab.style.polyhedra_settings.is_none() {
                    tab.style.polyhedra_settings =
                        Some(crate::config::PolyhedraSettings::default());
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
            row.append(&check_poly);

            container.append(&row);
        }
    }
}
