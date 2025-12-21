use gtk4::{self as gtk, prelude::*};
use gtk4::gdk;
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;

pub fn show_preferences_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: gtk::DrawingArea,
) {
    let window = gtk::Window::builder()
        .title("Preferences")
        .transient_for(parent)
        .modal(false)
        .default_width(360)
        .default_height(450)
        .resizable(false)
        .build();

    // Main container
    let main_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // The Tab Widget (Notebook)
    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true); // Fill available space

    // --- TAB 1: APPEARANCE ---
    let appearance_tab = build_appearance_tab(state.clone(), drawing_area.clone());
    notebook.append_page(&appearance_tab, Some(&gtk::Label::new(Some("Appearance"))));

    // --- TAB 2: SYSTEM (Future Proofing) ---
    // In the future, you can add physics/performance settings here.
    let system_tab = build_system_tab();
    notebook.append_page(&system_tab, Some(&gtk::Label::new(Some("System"))));

    main_vbox.append(&notebook);

    // --- FOOTER (Close Button) ---
    let footer_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    // FIXED: Replaced set_margin_all with explicit setters
    footer_box.set_margin_top(10);
    footer_box.set_margin_bottom(10);
    footer_box.set_margin_start(10);
    footer_box.set_margin_end(10);
    footer_box.set_halign(gtk::Align::End);

    let close_btn = gtk::Button::with_label("Close");
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    footer_box.append(&close_btn);
    main_vbox.append(&footer_box);

    window.set_child(Some(&main_vbox));
    window.present();
}

/// Helper: Builds the "Appearance" Tab content
fn build_appearance_tab(
    state: Rc<RefCell<AppState>>,
    drawing_area: gtk::DrawingArea
) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 15);
    // FIXED: Replaced set_margin_all
    container.set_margin_top(15);
    container.set_margin_bottom(15);
    container.set_margin_start(15);
    container.set_margin_end(15);

    // --- Helper for sliders ---
    fn add_slider(
        label: &str,
        val: f64,
        min: f64,
        max: f64,
        step: f64,
        box_cont: &gtk::Box,
        cb: impl Fn(f64) + 'static
    ) {
        let lbl = gtk::Label::new(Some(label));
        lbl.set_halign(gtk::Align::Start);
        lbl.set_margin_top(5);
        box_cont.append(&lbl);

        let sc = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
        sc.set_value(val);
        sc.set_margin_bottom(5);
        sc.connect_value_changed(move |s| cb(s.value()));
        box_cont.append(&sc);
    }

    // --- GROUP 1: ATOMS ---
    let frame_atoms = gtk::Frame::new(Some("Atoms"));
    let vbox_atoms = gtk::Box::new(gtk::Orientation::Vertical, 5);
    // FIXED: Replaced set_margin_all
    vbox_atoms.set_margin_top(10);
    vbox_atoms.set_margin_bottom(10);
    vbox_atoms.set_margin_start(10);
    vbox_atoms.set_margin_end(10);

    // 1.1 Scale
    let s = state.clone(); let da = drawing_area.clone();
    add_slider("Size (Scale)", state.borrow().style.atom_scale, 0.1, 1.2, 0.05, &vbox_atoms, move |v| {
        s.borrow_mut().style.atom_scale = v;
        da.queue_draw();
    });

    // 1.2 Shine
    let s = state.clone(); let da = drawing_area.clone();
    add_slider("Shine / Glossiness", state.borrow().style.shine_strength, 0.0, 1.0, 0.1, &vbox_atoms, move |v| {
        s.borrow_mut().style.shine_strength = v;
        da.queue_draw();
    });

    vbox_atoms.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // 1.3 Uniform Color
    let box_atom_col = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    box_atom_col.set_margin_top(5);

    let check_uniform = gtk::CheckButton::with_label("Uniform Color");
    check_uniform.set_active(state.borrow().style.use_uniform_atom_color);

    let btn_atom = gtk::ColorButton::new();
    let (ar, ag, ab) = state.borrow().style.atom_color;
    btn_atom.set_rgba(&gdk::RGBA::new(ar as f32, ag as f32, ab as f32, 1.0));
    btn_atom.set_sensitive(state.borrow().style.use_uniform_atom_color);
    btn_atom.set_hexpand(true);
    btn_atom.set_halign(gtk::Align::End);

    // Logic
    let s = state.clone();
    let da = drawing_area.clone();
    let btn_atom_clone = btn_atom.clone();
    check_uniform.connect_toggled(move |btn| {
        let is_active = btn.is_active();
        s.borrow_mut().style.use_uniform_atom_color = is_active;
        btn_atom_clone.set_sensitive(is_active);
        da.queue_draw();
    });

    let s = state.clone();
    let da = drawing_area.clone();
    btn_atom.connect_color_set(move |b| {
        let c = b.rgba();
        s.borrow_mut().style.atom_color = (c.red() as f64, c.green() as f64, c.blue() as f64);
        da.queue_draw();
    });

    box_atom_col.append(&check_uniform);
    box_atom_col.append(&btn_atom);
    vbox_atoms.append(&box_atom_col);
    frame_atoms.set_child(Some(&vbox_atoms));
    container.append(&frame_atoms);


    // --- GROUP 2: BONDS ---
    let frame_bonds = gtk::Frame::new(Some("Bonds"));
    let vbox_bonds = gtk::Box::new(gtk::Orientation::Vertical, 5);
    // FIXED: Replaced set_margin_all
    vbox_bonds.set_margin_top(10);
    vbox_bonds.set_margin_bottom(10);
    vbox_bonds.set_margin_start(10);
    vbox_bonds.set_margin_end(10);

    // 2.1 Radius
    let s = state.clone(); let da = drawing_area.clone();
    add_slider("Thickness (Radius)", state.borrow().style.bond_radius, 0.01, 0.3, 0.01, &vbox_bonds, move |v| {
        s.borrow_mut().style.bond_radius = v;
        da.queue_draw();
    });

    vbox_bonds.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // 2.2 Color
    let box_bond_col = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    box_bond_col.set_margin_top(5);

    let lbl_bond_c = gtk::Label::new(Some("Color"));
    lbl_bond_c.set_halign(gtk::Align::Start);

    let btn_bond = gtk::ColorButton::new();
    let (br, bg, bb) = state.borrow().style.bond_color;
    btn_bond.set_rgba(&gdk::RGBA::new(br as f32, bg as f32, bb as f32, 1.0));
    btn_bond.set_hexpand(true);
    btn_bond.set_halign(gtk::Align::End);

    let s = state.clone(); let da = drawing_area.clone();
    btn_bond.connect_color_set(move |b| {
        let c = b.rgba();
        s.borrow_mut().style.bond_color = (c.red() as f64, c.green() as f64, c.blue() as f64);
        da.queue_draw();
    });

    box_bond_col.append(&lbl_bond_c);
    box_bond_col.append(&btn_bond);
    vbox_bonds.append(&box_bond_col);
    frame_bonds.set_child(Some(&vbox_bonds));
    container.append(&frame_bonds);

    container
}

/// Helper: Builds a placeholder "System" Tab
fn build_system_tab() -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 15);
    // FIXED: Replaced set_margin_all
    container.set_margin_top(20);
    container.set_margin_bottom(20);
    container.set_margin_start(20);
    container.set_margin_end(20);

    // Placeholder content
    let lbl = gtk::Label::new(Some("System settings (Physics, Performance) will go here."));
    lbl.set_opacity(0.5); // Make it look disabled/placeholder
    container.append(&lbl);

    container
}
