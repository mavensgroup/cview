// src/ui/preferences.rs
// Preferences window — 3 tabs:
//   1. General      — 7 settings (file load defaults, zoom, rotation)
//   2. Appearance   — 8 settings (colors, toggles, scales)
//   3. Export/Plot  — 6 settings (charge density export font sizes, colormap)
//
// Removed tabs (settings kept in Config for serde backward-compat):
//   - Bond Valence  (3 settings — none were wired to runtime behavior)
//   - Performance   (5 settings — none were wired to runtime behavior)
//   - Advanced      (5 settings — none were wired to runtime behavior)

use crate::config::RotationCenter;
use crate::state::AppState;
use gtk4::{self as gtk, gdk, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

pub fn show_preferences_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: gtk::DrawingArea,
) {
    let window = gtk::Window::builder()
        .title("Preferences")
        .transient_for(parent)
        .modal(true)
        .default_width(550)
        .default_height(500)
        .resizable(false)
        .build();

    let main_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);

    // TAB 1: General
    let general_tab = build_general_tab(state.clone(), drawing_area.clone());
    notebook.append_page(&general_tab, Some(&gtk::Label::new(Some("General"))));

    // TAB 2: Appearance
    let appearance_tab = build_appearance_tab(state.clone(), drawing_area.clone());
    notebook.append_page(&appearance_tab, Some(&gtk::Label::new(Some("Appearance"))));

    // TAB 3: Export / Plot
    let export_tab = build_export_plot_tab(state.clone());
    notebook.append_page(&export_tab, Some(&gtk::Label::new(Some("Export / Plot"))));

    main_vbox.append(&notebook);

    // Footer
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    footer.set_margin_top(10);
    footer.set_margin_bottom(10);
    footer.set_margin_end(10);
    footer.set_halign(gtk::Align::End);

    let btn_close = gtk::Button::with_label("Close");
    let win_clone = window.clone();
    btn_close.connect_clicked(move |_| win_clone.close());
    footer.append(&btn_close);
    main_vbox.append(&footer);

    window.set_child(Some(&main_vbox));
    window.present();
}

// ============================================================================
// TAB 1: GENERAL (7 settings)
// ============================================================================

fn build_general_tab(state: Rc<RefCell<AppState>>, da: gtk::DrawingArea) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 15);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    // 1. Show Full Unit Cell
    let check1 = gtk::CheckButton::with_label("Show Full Unit Cell on Load");
    check1.set_active(state.borrow().config.default_show_full_cell);
    let s1 = state.clone();
    check1.connect_toggled(move |c| {
        let mut st = s1.borrow_mut();
        st.config.default_show_full_cell = c.is_active();
        st.save_config();
    });
    vbox.append(&check1);

    // 2. Show Bonds
    let check2 = gtk::CheckButton::with_label("Show Bonds by Default");
    check2.set_active(state.borrow().config.default_show_bonds);
    let s2 = state.clone();
    check2.connect_toggled(move |c| {
        let mut st = s2.borrow_mut();
        st.config.default_show_bonds = c.is_active();
        st.save_config();
    });
    vbox.append(&check2);

    // 3. Bond Tolerance
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let tol_label = gtk::Label::new(Some("Default Bond Tolerance:"));
    tol_label.set_halign(gtk::Align::Start);
    vbox.append(&tol_label);

    let tol_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.6, 1.6, 0.05);
    tol_scale.set_value(state.borrow().config.default_bond_tolerance);
    tol_scale.set_draw_value(true);
    tol_scale.set_value_pos(gtk::PositionType::Right);
    let s3 = state.clone();
    tol_scale.connect_value_changed(move |sc| {
        let mut st = s3.borrow_mut();
        st.config.default_bond_tolerance = sc.value();
        st.save_config();
    });
    vbox.append(&tol_scale);

    // 4. Rotation Center
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let rot_label = gtk::Label::new(Some("Default Rotation Center:"));
    rot_label.set_halign(gtk::Align::Start);
    vbox.append(&rot_label);

    let rot_dropdown = gtk::DropDown::from_strings(&["Structure Centroid", "Unit Cell Center"]);
    rot_dropdown.set_selected(match state.borrow().config.rotation_mode {
        RotationCenter::Centroid => 0,
        RotationCenter::UnitCell => 1,
    });
    let s4 = state.clone();
    rot_dropdown.connect_selected_notify(move |d| {
        let mut st = s4.borrow_mut();
        st.config.rotation_mode = match d.selected() {
            1 => RotationCenter::UnitCell,
            _ => RotationCenter::Centroid,
        };
        st.save_config();
    });
    vbox.append(&rot_dropdown);

    // 5. Default Zoom
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let zoom_label = gtk::Label::new(Some("Default Zoom Level:"));
    zoom_label.set_halign(gtk::Align::Start);
    vbox.append(&zoom_label);

    let zoom_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.5, 2.0, 0.1);
    zoom_scale.set_value(state.borrow().config.default_zoom);
    zoom_scale.set_draw_value(true);
    zoom_scale.set_value_pos(gtk::PositionType::Right);
    let s5 = state.clone();
    zoom_scale.connect_value_changed(move |sc| {
        let mut st = s5.borrow_mut();
        st.config.default_zoom = sc.value();
        st.save_config();
    });
    vbox.append(&zoom_scale);

    // 6. Auto-Center
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let check6 = gtk::CheckButton::with_label("Auto-Center Structure on Load");
    check6.set_active(state.borrow().config.auto_center_structure);
    let s6 = state.clone();
    check6.connect_toggled(move |c| {
        let mut st = s6.borrow_mut();
        st.config.auto_center_structure = c.is_active();
        st.save_config();
    });
    vbox.append(&check6);

    // 7. Remember Last View
    let check7 = gtk::CheckButton::with_label("Remember Last View (rotation/zoom)");
    check7.set_active(state.borrow().config.remember_last_view);
    let s7 = state.clone();
    check7.connect_toggled(move |c| {
        let mut st = s7.borrow_mut();
        st.config.remember_last_view = c.is_active();
        st.save_config();
    });
    vbox.append(&check7);

    // Suppress unused variable warning for `da` (kept for API consistency)
    let _ = da;

    vbox
}

// ============================================================================
// TAB 2: APPEARANCE (8 settings)
// ============================================================================

fn build_appearance_tab(state: Rc<RefCell<AppState>>, da: gtk::DrawingArea) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 15);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    // 1. Background Color
    let bg_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let bg_label = gtk::Label::new(Some("Background Color:"));
    bg_label.set_hexpand(true);
    bg_label.set_halign(gtk::Align::Start);
    bg_row.append(&bg_label);

    let bg_btn = gtk::ColorButton::new();
    let bg = state.borrow().config.style.background_color;
    bg_btn.set_rgba(&gdk::RGBA::new(bg.0 as f32, bg.1 as f32, bg.2 as f32, 1.0));
    let s_bg = state.clone();
    let da_bg = da.clone();
    bg_btn.connect_color_set(move |btn| {
        let rgba = btn.rgba();
        let mut st = s_bg.borrow_mut();
        st.config.style.background_color =
            (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);

        // Update active tab immediately
        if !st.tabs.is_empty() {
            st.active_tab_mut().style.background_color = st.config.style.background_color;
        }

        st.save_config();
        drop(st);
        da_bg.queue_draw();
    });
    bg_row.append(&bg_btn);
    vbox.append(&bg_row);

    // 2. Bond Color
    let bc_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let bc_label = gtk::Label::new(Some("Default Bond Color:"));
    bc_label.set_hexpand(true);
    bc_label.set_halign(gtk::Align::Start);
    bc_row.append(&bc_label);

    let bc_btn = gtk::ColorButton::new();
    let bc = state.borrow().config.style.bond_color;
    bc_btn.set_rgba(&gdk::RGBA::new(bc.0 as f32, bc.1 as f32, bc.2 as f32, 1.0));
    let s_bc = state.clone();
    let da_bc = da.clone();
    bc_btn.connect_color_set(move |btn| {
        let rgba = btn.rgba();
        let mut st = s_bc.borrow_mut();
        st.config.style.bond_color = (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);

        // Update active tab immediately
        if !st.tabs.is_empty() {
            st.active_tab_mut().style.bond_color = st.config.style.bond_color;
        }

        st.save_config();
        drop(st);
        da_bc.queue_draw();
    });
    bc_row.append(&bc_btn);
    vbox.append(&bc_row);

    // 3. Render Quality
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let rq_label = gtk::Label::new(Some("Screen Render Quality:"));
    rq_label.set_halign(gtk::Align::Start);
    vbox.append(&rq_label);

    let rq_dropdown = gtk::DropDown::from_strings(&["Fast (Sprites)", "High (Vector)"]);
    {
        use crate::config::RenderQuality;
        rq_dropdown.set_selected(match state.borrow().config.render_quality {
            RenderQuality::Fast => 0,
            RenderQuality::High => 1,
        });
        let s_rq = state.clone();
        rq_dropdown.connect_selected_notify(move |d| {
            let mut st = s_rq.borrow_mut();
            st.config.render_quality = match d.selected() {
                1 => RenderQuality::High,
                _ => RenderQuality::Fast,
            };
            st.save_config();
        });
    }
    vbox.append(&rq_dropdown);

    // 4-6. Checkboxes
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let check_axes = gtk::CheckButton::with_label("Show Coordinate Axes by Default");
    check_axes.set_active(state.borrow().config.default_show_axes);
    let s_axes = state.clone();
    check_axes.connect_toggled(move |c| {
        let mut st = s_axes.borrow_mut();
        st.config.default_show_axes = c.is_active();
        st.save_config();
    });
    vbox.append(&check_axes);

    let check_cell = gtk::CheckButton::with_label("Show Unit Cell Box by Default");
    check_cell.set_active(state.borrow().config.default_show_unit_cell);
    let s_cell = state.clone();
    check_cell.connect_toggled(move |c| {
        let mut st = s_cell.borrow_mut();
        st.config.default_show_unit_cell = c.is_active();
        st.save_config();
    });
    vbox.append(&check_cell);

    let check_ghost = gtk::CheckButton::with_label("Show Ghost Atoms (Supercell Boundaries)");
    check_ghost.set_active(state.borrow().config.show_ghost_atoms);
    let s_ghost = state.clone();
    check_ghost.connect_toggled(move |c| {
        let mut st = s_ghost.borrow_mut();
        st.config.show_ghost_atoms = c.is_active();
        st.save_config();
    });
    vbox.append(&check_ghost);

    // 7-8. Sliders
    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let as_label = gtk::Label::new(Some("Default Atom Scale:"));
    as_label.set_halign(gtk::Align::Start);
    vbox.append(&as_label);
    let as_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.5, 0.05);
    as_scale.set_value(state.borrow().config.default_atom_scale);
    as_scale.set_draw_value(true);
    let s_as = state.clone();
    as_scale.connect_value_changed(move |sc| {
        let mut st = s_as.borrow_mut();
        st.config.default_atom_scale = sc.value();
        st.save_config();
    });
    vbox.append(&as_scale);

    let br_label = gtk::Label::new(Some("Default Bond Radius:"));
    br_label.set_halign(gtk::Align::Start);
    vbox.append(&br_label);
    let br_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.01, 0.5, 0.01);
    br_scale.set_value(state.borrow().config.default_bond_radius);
    br_scale.set_draw_value(true);
    let s_br = state.clone();
    br_scale.connect_value_changed(move |sc| {
        let mut st = s_br.borrow_mut();
        st.config.default_bond_radius = sc.value();
        st.save_config();
    });
    vbox.append(&br_scale);

    // Suppress unused variable warning for `da` clone not used beyond color buttons
    let _ = da;

    vbox
}

// ============================================================================
// TAB 3: EXPORT / PLOT (6 settings)
// ============================================================================

fn build_export_plot_tab(state: Rc<RefCell<AppState>>) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    let heading = gtk::Label::new(Some("Charge Density Export Defaults"));
    heading.add_css_class("title-4");
    heading.set_halign(gtk::Align::Start);
    vbox.append(&heading);

    let note = gtk::Label::new(Some(
        "Font sizes and line widths for PNG / PDF export.\n\
         Changes take effect on the next export.",
    ));
    note.set_halign(gtk::Align::Start);
    note.add_css_class("dim-label");
    note.set_wrap(true);
    vbox.append(&note);

    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // --- Axis Label Font Size ---
    let row1 = labeled_spin(
        "Axis Label Font Size (pt):",
        8.0,
        24.0,
        1.0,
        state.borrow().config.export_plot.font_size_axis_label,
    );
    let s1 = state.clone();
    row1.1.connect_value_changed(move |sp| {
        let mut st = s1.borrow_mut();
        st.config.export_plot.font_size_axis_label = sp.value();
        st.save_config();
    });
    vbox.append(&row1.0);

    // --- Tick Label Font Size ---
    let row2 = labeled_spin(
        "Tick Label Font Size (pt):",
        6.0,
        18.0,
        1.0,
        state.borrow().config.export_plot.font_size_tick_label,
    );
    let s2 = state.clone();
    row2.1.connect_value_changed(move |sp| {
        let mut st = s2.borrow_mut();
        st.config.export_plot.font_size_tick_label = sp.value();
        st.save_config();
    });
    vbox.append(&row2.0);

    // --- Annotation Font Size ---
    let row3 = labeled_spin(
        "Plane Annotation Font Size (pt):",
        8.0,
        20.0,
        1.0,
        state.borrow().config.export_plot.font_size_annotation,
    );
    let s3 = state.clone();
    row3.1.connect_value_changed(move |sp| {
        let mut st = s3.borrow_mut();
        st.config.export_plot.font_size_annotation = sp.value();
        st.save_config();
    });
    vbox.append(&row3.0);

    // --- Colorbar Font Size ---
    let row4 = labeled_spin(
        "Colorbar Label Font Size (pt):",
        6.0,
        18.0,
        1.0,
        state.borrow().config.export_plot.font_size_colorbar,
    );
    let s4 = state.clone();
    row4.1.connect_value_changed(move |sp| {
        let mut st = s4.borrow_mut();
        st.config.export_plot.font_size_colorbar = sp.value();
        st.save_config();
    });
    vbox.append(&row4.0);

    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // --- Isoline Line Width ---
    let lbl5 = gtk::Label::new(Some("Isoline Line Width (export):"));
    lbl5.set_halign(gtk::Align::Start);
    vbox.append(&lbl5);
    let scale_iso = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.5, 4.0, 0.1);
    scale_iso.set_value(state.borrow().config.export_plot.isoline_line_width);
    scale_iso.set_draw_value(true);
    scale_iso.set_value_pos(gtk::PositionType::Right);
    let s5 = state.clone();
    scale_iso.connect_value_changed(move |sc| {
        let mut st = s5.borrow_mut();
        st.config.export_plot.isoline_line_width = sc.value();
        st.save_config();
    });
    vbox.append(&scale_iso);

    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // --- Default Colormap ---
    let lbl6 = gtk::Label::new(Some("Default Colormap:"));
    lbl6.set_halign(gtk::Align::Start);
    vbox.append(&lbl6);
    let cmap_dropdown =
        gtk::DropDown::from_strings(&["Viridis", "Plasma", "Blue–White–Red", "Grayscale"]);
    cmap_dropdown.set_selected(state.borrow().config.export_plot.default_colormap as u32);
    let s6 = state.clone();
    cmap_dropdown.connect_selected_notify(move |d| {
        let mut st = s6.borrow_mut();
        st.config.export_plot.default_colormap = d.selected() as usize;
        st.save_config();
    });
    vbox.append(&cmap_dropdown);

    vbox
}

// ============================================================================
// Helpers
// ============================================================================

/// Create a horizontal row with a label and a SpinButton.
/// Returns (container Box, SpinButton) so caller can connect signals.
fn labeled_spin(
    label: &str,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
) -> (gtk::Box, gtk::SpinButton) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_halign(gtk::Align::Start);
    lbl.set_hexpand(true);
    row.append(&lbl);
    let spin = gtk::SpinButton::with_range(min, max, step);
    spin.set_value(value);
    spin.set_width_chars(5);
    row.append(&spin);
    (row, spin)
}
