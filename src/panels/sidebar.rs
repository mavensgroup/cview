use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Orientation, Scale, Adjustment, Label, Align, DrawingArea,
    Expander, ScrolledWindow, PolicyType, Frame, ColorButton, Button, Separator
};
use gtk4::gdk;

use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use crate::model::elements::get_atom_properties;

/// Builds the sidebar and returns (The ScrolledWindow, The Atom List Container Box)
pub fn build(state: Rc<RefCell<AppState>>, drawing_area: &DrawingArea) -> (ScrolledWindow, GtkBox) {
    // 1. Root Container (Scrollable)
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_width(280)
        .build();

    let root_vbox = GtkBox::new(Orientation::Vertical, 10);
    root_vbox.set_margin_start(10);
    root_vbox.set_margin_end(10);
    root_vbox.set_margin_top(10);
    root_vbox.set_margin_bottom(10);
    scroll.set_child(Some(&root_vbox));

    // --- Helper for Sliders (Fixed Snapping) ---
    let create_slider = |label: &str, min: f64, max: f64, step: f64, val: f64, callback: Box<dyn Fn(f64)>| {
        let b = GtkBox::new(Orientation::Vertical, 2);
        b.append(&Label::builder().label(label).halign(Align::Start).build());

        let adj = Adjustment::new(val, min, max, step, step, 0.0);
        let scale = Scale::new(Orientation::Horizontal, Some(&adj));
        scale.set_digits(2);
        scale.set_draw_value(true);
        scale.set_value_pos(gtk4::PositionType::Right);

        scale.connect_value_changed(move |sc| {
            let raw = sc.value();

            // LOGIC FIX: Always snap, regardless of step size
            let snapped = (raw / step).round() * step;

            // If the slider is not currently at the snapped value, move it there
            if (raw - snapped).abs() > 0.0001 {
                sc.set_value(snapped);
                return; // Stop here, the set_value will trigger this callback again with the correct number
            }

            callback(snapped);
        });
        b.append(&scale);
        b
    };

    // ============================================================
    // SECTION 1: VIEW CONTROLS
    // ============================================================
    let controls_expander = Expander::new(Some("View Controls"));
    controls_expander.set_expanded(true);

    let controls_box = GtkBox::new(Orientation::Vertical, 15);
    controls_box.set_margin_top(10);
    controls_box.set_margin_bottom(10);
    controls_box.set_margin_start(5);

    // Zoom
    let s_z = state.clone(); let da_z = drawing_area.clone();
    controls_box.append(&create_slider("Zoom", 0.1, 5.0, 0.1, state.borrow().zoom, Box::new(move |v| {
        s_z.borrow_mut().zoom = v;
        da_z.queue_draw();
    })));

    // Rot X
    let s_rx = state.clone(); let da_rx = drawing_area.clone();
    controls_box.append(&create_slider("Rotation X", 0.0, 360.0, 15.0, state.borrow().rot_x, Box::new(move |v| {
        s_rx.borrow_mut().rot_x = v;
        da_rx.queue_draw();
    })));

    // Rot Y
    let s_ry = state.clone(); let da_ry = drawing_area.clone();
    controls_box.append(&create_slider("Rotation Y", 0.0, 360.0, 15.0, state.borrow().rot_y, Box::new(move |v| {
        s_ry.borrow_mut().rot_y = v;
        da_ry.queue_draw();
    })));

    // Rot Z
    let s_rz = state.clone(); let da_rz = drawing_area.clone();
    controls_box.append(&create_slider("Rotation Z", 0.0, 360.0, 15.0, state.borrow().rot_z, Box::new(move |v| {
        s_rz.borrow_mut().rot_z = v;
        da_rz.queue_draw();
    })));

    controls_expander.set_child(Some(&controls_box));
    root_vbox.append(&controls_expander);


    // ============================================================
    // SECTION 2: APPEARANCE
    // ============================================================
    let style_expander = Expander::new(Some("Appearance"));

    // FIX 2: Set expanded to false by default
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
    let s_met = state.clone(); let da_met = drawing_area.clone();
    vbox_mat.append(&create_slider("Metallic", 0.0, 1.0, 0.05, state.borrow().style.metallic, Box::new(move |v| {
        s_met.borrow_mut().style.metallic = v;
        da_met.queue_draw();
    })));

    // Roughness
    let s_rgh = state.clone(); let da_rgh = drawing_area.clone();
    vbox_mat.append(&create_slider("Roughness", 0.0, 1.0, 0.05, state.borrow().style.roughness, Box::new(move |v| {
        s_rgh.borrow_mut().style.roughness = v;
        da_rgh.queue_draw();
    })));

    // Transmission
    let s_tr = state.clone(); let da_tr = drawing_area.clone();
    vbox_mat.append(&create_slider("Transmission", 0.0, 1.0, 0.05, state.borrow().style.transmission, Box::new(move |v| {
        s_tr.borrow_mut().style.transmission = v;
        da_tr.queue_draw();
    })));

    frame_mat.set_child(Some(&vbox_mat));
    style_box.append(&frame_mat);


    // --- ATOMS ---
    let frame_atoms = Frame::new(Some("Atoms"));
    let vbox_atoms = GtkBox::new(Orientation::Vertical, 10);

    vbox_atoms.set_margin_top(10);
    vbox_atoms.set_margin_bottom(10);
    vbox_atoms.set_margin_start(10);
    vbox_atoms.set_margin_end(10);

    // 1. Global Scale Slider
    let s_as = state.clone(); let da_as = drawing_area.clone();
    vbox_atoms.append(&create_slider("Size Scale", 0.1, 2.0, 0.05, state.borrow().style.atom_scale, Box::new(move |v| {
        s_as.borrow_mut().style.atom_scale = v;
        da_as.queue_draw();
    })));

    vbox_atoms.append(&Separator::new(Orientation::Horizontal));

    // 2. Dynamic Element List Container
    let atoms_list_container = GtkBox::new(Orientation::Vertical, 5);
    vbox_atoms.append(&atoms_list_container);

    // Initial Population
    refresh_atom_list(&atoms_list_container, state.clone(), drawing_area);

    frame_atoms.set_child(Some(&vbox_atoms));
    style_box.append(&frame_atoms);

    // --- BONDS ---
    let frame_bonds = Frame::new(Some("Bonds"));
    let vbox_bonds = GtkBox::new(Orientation::Vertical, 10);

    vbox_bonds.set_margin_top(10);
    vbox_bonds.set_margin_bottom(10);
    vbox_bonds.set_margin_start(10);
    vbox_bonds.set_margin_end(10);

    // Radius
    let s_br = state.clone(); let da_br = drawing_area.clone();
    vbox_bonds.append(&create_slider("Radius", 0.01, 0.5, 0.01, state.borrow().style.bond_radius, Box::new(move |v| {
        s_br.borrow_mut().style.bond_radius = v;
        da_br.queue_draw();
    })));

    // Color
    let box_bcol = GtkBox::new(Orientation::Horizontal, 10);
    box_bcol.append(&Label::new(Some("Color")));

    let btn_bcol = ColorButton::new();
    let (br, bg, bb) = state.borrow().style.bond_color;
    btn_bcol.set_rgba(&gdk::RGBA::new(br as f32, bg as f32, bb as f32, 1.0));

    let s_bc = state.clone(); let da_bc = drawing_area.clone();
    btn_bcol.connect_color_set(move |b| {
        let c = b.rgba();
        s_bc.borrow_mut().style.bond_color = (c.red() as f64, c.green() as f64, c.blue() as f64);
        da_bc.queue_draw();
    });
    box_bcol.append(&btn_bcol);
    vbox_bonds.append(&box_bcol);

    frame_bonds.set_child(Some(&vbox_bonds));
    style_box.append(&frame_bonds);

    style_expander.set_child(Some(&style_box));
    root_vbox.append(&style_expander);

    // RETURN both components
    (scroll, atoms_list_container)
}


/// Public helper to rebuild the list of atom colors dynamically
pub fn refresh_atom_list(container: &GtkBox, state: Rc<RefCell<AppState>>, drawing_area: &DrawingArea) {
    // 1. Clear existing list
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    // 2. Get Elements
    let elements = if let Some(structure) = &state.borrow().structure {
        let mut unique: Vec<String> = structure.atoms.iter().map(|a| a.element.clone()).collect();
        unique.sort();
        unique.dedup();
        unique
    } else {
        vec![]
    };

    // 3. Rebuild UI
    if elements.is_empty() {
        let lbl = Label::new(Some("(Load file to see elements)"));
        lbl.set_opacity(0.6);
        container.append(&lbl);
    } else {
        for elem in elements {
            let row = GtkBox::new(Orientation::Horizontal, 10);

            // Label
            let lbl = Label::new(Some(&format!("{}", elem)));
            lbl.set_width_chars(3);
            lbl.set_xalign(0.0);
            row.append(&lbl);

            // Color Button
            let current_color = {
                let st = state.borrow();
                if let Some(c) = st.style.element_colors.get(&elem) { *c }
                else {
                    let (_, def) = get_atom_properties(&elem);
                    def
                }
            };

            let btn = ColorButton::new();
            btn.set_rgba(&gdk::RGBA::new(
                current_color.0 as f32, current_color.1 as f32, current_color.2 as f32, 1.0
            ));

            let s = state.clone(); let da = drawing_area.clone(); let elem_key = elem.clone();
            btn.connect_color_set(move |b| {
                let c = b.rgba();
                s.borrow_mut().style.element_colors.insert(elem_key.clone(), (c.red() as f64, c.green() as f64, c.blue() as f64));
                da.queue_draw();
            });
            row.append(&btn);

            // Reset Button
            let btn_reset = Button::with_label("↺");
            let s_r = state.clone(); let da_r = drawing_area.clone();
            let elem_key_r = elem.clone(); let btn_ref = btn.clone();

            btn_reset.connect_clicked(move |_| {
                s_r.borrow_mut().style.element_colors.remove(&elem_key_r);
                let (_, def) = get_atom_properties(&elem_key_r);
                btn_ref.set_rgba(&gdk::RGBA::new(def.0 as f32, def.1 as f32, def.2 as f32, 1.0));
                da_r.queue_draw();
            });
            row.append(&btn_reset);

            container.append(&row);
        }
    }
}
