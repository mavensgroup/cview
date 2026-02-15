// src/ui/preferences.rs
// COMPREHENSIVE PREFERENCES - All 31 settings with UI

use crate::config::{AntialiasLevel, RenderQuality, RotationCenter};
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

    // TAB 3: BVS
    let bvs_tab = build_bvs_tab(state.clone());
    notebook.append_page(&bvs_tab, Some(&gtk::Label::new(Some("Bond Valence"))));

    // TAB 4: Performance
    let perf_tab = build_performance_tab(state.clone());
    notebook.append_page(&perf_tab, Some(&gtk::Label::new(Some("Performance"))));

    // TAB 5: Advanced
    let adv_tab = build_advanced_tab(state.clone());
    notebook.append_page(&adv_tab, Some(&gtk::Label::new(Some("Advanced"))));

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

    vbox
}

// ============================================================================
// TAB 2: APPEARANCE (9 settings)
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

    vbox
}

// ============================================================================
// TAB 3: BVS (5 settings)
// ============================================================================

fn build_bvs_tab(state: Rc<RefCell<AppState>>) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 15);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    let good_label = gtk::Label::new(Some("Good Threshold (Green):"));
    good_label.set_halign(gtk::Align::Start);
    vbox.append(&good_label);
    let good_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.05, 0.30, 0.01);
    good_scale.set_value(state.borrow().config.bvs_threshold_good);
    good_scale.set_draw_value(true);
    let s_good = state.clone();
    good_scale.connect_value_changed(move |sc| {
        let mut st = s_good.borrow_mut();
        st.config.bvs_threshold_good = sc.value();
        st.save_config();
    });
    vbox.append(&good_scale);

    let warn_label = gtk::Label::new(Some("Warning Threshold (Yellow→Red):"));
    warn_label.set_halign(gtk::Align::Start);
    vbox.append(&warn_label);
    let warn_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.20, 0.60, 0.01);
    warn_scale.set_value(state.borrow().config.bvs_threshold_warn);
    warn_scale.set_draw_value(true);
    let s_warn = state.clone();
    warn_scale.connect_value_changed(move |sc| {
        let mut st = s_warn.borrow_mut();
        st.config.bvs_threshold_warn = sc.value();
        st.save_config();
    });
    vbox.append(&warn_scale);

    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let check1 = gtk::CheckButton::with_label("Auto-Calculate BVS on File Load");
    check1.set_active(state.borrow().config.auto_calc_bvs);
    let s1 = state.clone();
    check1.connect_toggled(move |c| {
        s1.borrow_mut().config.auto_calc_bvs = c.is_active();
        s1.borrow_mut().save_config();
    });
    vbox.append(&check1);

    let check2 = gtk::CheckButton::with_label("Show BVS Report When Switching Modes");
    check2.set_active(state.borrow().config.show_bvs_report);
    let s2 = state.clone();
    check2.connect_toggled(move |c| {
        s2.borrow_mut().config.show_bvs_report = c.is_active();
        s2.borrow_mut().save_config();
    });
    vbox.append(&check2);

    let check3 = gtk::CheckButton::with_label("Warn on Poor BVS Match");
    check3.set_active(state.borrow().config.warn_poor_bvs);
    let s3 = state.clone();
    check3.connect_toggled(move |c| {
        s3.borrow_mut().config.warn_poor_bvs = c.is_active();
        s3.borrow_mut().save_config();
    });
    vbox.append(&check3);

    vbox
}

// ============================================================================
// TAB 4: PERFORMANCE (5 settings)
// ============================================================================

fn build_performance_tab(state: Rc<RefCell<AppState>>) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 15);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    let aa_label = gtk::Label::new(Some("Antialiasing Level:"));
    aa_label.set_halign(gtk::Align::Start);
    vbox.append(&aa_label);
    let aa_dropdown = gtk::DropDown::from_strings(&["None", "Fast", "Good", "Best"]);
    aa_dropdown.set_selected(match state.borrow().config.antialias_level {
        AntialiasLevel::None => 0,
        AntialiasLevel::Fast => 1,
        AntialiasLevel::Good => 2,
        AntialiasLevel::Best => 3,
    });
    let s_aa = state.clone();
    aa_dropdown.connect_selected_notify(move |d| {
        let mut st = s_aa.borrow_mut();
        st.config.antialias_level = match d.selected() {
            0 => AntialiasLevel::None,
            1 => AntialiasLevel::Fast,
            3 => AntialiasLevel::Best,
            _ => AntialiasLevel::Good,
        };
        st.save_config();
    });
    vbox.append(&aa_dropdown);

    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let max_label = gtk::Label::new(Some("Maximum Atoms to Display:"));
    max_label.set_halign(gtk::Align::Start);
    vbox.append(&max_label);
    let max_spin = gtk::SpinButton::with_range(100.0, 50000.0, 100.0);
    max_spin.set_value(state.borrow().config.max_atoms_display as f64);
    let s_max = state.clone();
    max_spin.connect_value_changed(move |sp| {
        s_max.borrow_mut().config.max_atoms_display = sp.value() as usize;
        s_max.borrow_mut().save_config();
    });
    vbox.append(&max_spin);

    vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let check1 = gtk::CheckButton::with_label("Use Hardware Acceleration");
    check1.set_active(state.borrow().config.use_hardware_acceleration);
    let s1 = state.clone();
    check1.connect_toggled(move |c| {
        s1.borrow_mut().config.use_hardware_acceleration = c.is_active();
        s1.borrow_mut().save_config();
    });
    vbox.append(&check1);

    let check2 = gtk::CheckButton::with_label("Enable Sprite Caching");
    check2.set_active(state.borrow().config.enable_sprite_cache);
    let s2 = state.clone();
    check2.connect_toggled(move |c| {
        s2.borrow_mut().config.enable_sprite_cache = c.is_active();
        s2.borrow_mut().save_config();
    });
    vbox.append(&check2);

    let cache_label = gtk::Label::new(Some("Cache Size (MB):"));
    cache_label.set_halign(gtk::Align::Start);
    vbox.append(&cache_label);
    let cache_spin = gtk::SpinButton::with_range(50.0, 500.0, 10.0);
    cache_spin.set_value(state.borrow().config.cache_size_mb as f64);
    let s_cache = state.clone();
    cache_spin.connect_value_changed(move |sp| {
        s_cache.borrow_mut().config.cache_size_mb = sp.value() as usize;
        s_cache.borrow_mut().save_config();
    });
    vbox.append(&cache_spin);

    vbox
}

// ============================================================================
// TAB 5: ADVANCED (5 settings)
// ============================================================================

fn build_advanced_tab(state: Rc<RefCell<AppState>>) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 15);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    let check1 = gtk::CheckButton::with_label("Show FPS Counter");
    check1.set_active(state.borrow().config.show_fps);
    let s1 = state.clone();
    check1.connect_toggled(move |c| {
        s1.borrow_mut().config.show_fps = c.is_active();
        s1.borrow_mut().save_config();
    });
    vbox.append(&check1);

    let check2 = gtk::CheckButton::with_label("Verbose Console Logging");
    check2.set_active(state.borrow().config.verbose_logging);
    let s2 = state.clone();
    check2.connect_toggled(move |c| {
        s2.borrow_mut().config.verbose_logging = c.is_active();
        s2.borrow_mut().save_config();
    });
    vbox.append(&check2);

    let check3 = gtk::CheckButton::with_label("Enable Experimental Features");
    check3.set_active(state.borrow().config.enable_experimental);
    let s3 = state.clone();
    check3.connect_toggled(move |c| {
        s3.borrow_mut().config.enable_experimental = c.is_active();
        s3.borrow_mut().save_config();
    });
    vbox.append(&check3);

    let check4 = gtk::CheckButton::with_label("Show Measurement Labels");
    check4.set_active(state.borrow().config.show_measurement_labels);
    let s4 = state.clone();
    check4.connect_toggled(move |c| {
        s4.borrow_mut().config.show_measurement_labels = c.is_active();
        s4.borrow_mut().save_config();
    });
    vbox.append(&check4);

    let check5 = gtk::CheckButton::with_label("Auto-Detect File Format");
    check5.set_active(state.borrow().config.auto_detect_format);
    let s5 = state.clone();
    check5.connect_toggled(move |c| {
        s5.borrow_mut().config.auto_detect_format = c.is_active();
        s5.borrow_mut().save_config();
    });
    vbox.append(&check5);

    vbox
}
