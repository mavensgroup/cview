// src/panels/sidebar.rs

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
  Adjustment, Align, Box as GtkBox, Button, ColorButton, CssProvider, DrawingArea, Expander, Frame,
  Label, Notebook, Orientation, PolicyType, Scale, ScrolledWindow, Separator,
  STYLE_PROVIDER_PRIORITY_APPLICATION,
};

use crate::model::elements::get_atom_properties;
use crate::state::AppState;
use std::cell::RefCell;
use std::rc::Rc;

/// Builds the sidebar and returns (The ScrolledWindow, The Atom List Container Box)
/// UPDATED: Accepts &Notebook instead of &DrawingArea
pub fn build(state: Rc<RefCell<AppState>>, notebook: &Notebook) -> (ScrolledWindow, GtkBox) {
  // --- 0. INJECT CUSTOM CSS FOR "BOLD LINE" SLIDERS ---
  let provider = CssProvider::new();
  provider.load_from_data(
    "
        scale.thin-slider slider {
            min-width: 6px;       /* Width of the line */
            min-height: 18px;     /* Height of the line */
            margin-top: -7px;     /* Center vertically on track */
            margin-bottom: -7px;
            border-radius: 2px;   /* Slight rounding */
            background-color: #555555; /* Dark Bold Line */
            box-shadow: none;
            outline: none;
        }
        scale.thin-slider slider:hover {
            background-color: #3584e4; /* Blue when hovering */
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

  // 1. Root Container (Scrollable)
  let scroll = ScrolledWindow::builder()
    .hscrollbar_policy(PolicyType::Never)
    .vscrollbar_policy(PolicyType::Automatic)
    .min_content_width(200)
    .build();

  let root_vbox = GtkBox::new(Orientation::Vertical, 10);
  root_vbox.set_margin_start(10);
  root_vbox.set_margin_end(10);
  root_vbox.set_margin_top(10);
  root_vbox.set_margin_bottom(10);
  scroll.set_child(Some(&root_vbox));

  // --- Helper for Sliders (Fixed Snapping & Styling) ---
  let create_slider =
    |label: &str, min: f64, max: f64, step: f64, val: f64, callback: Box<dyn Fn(f64)>| {
      let b = GtkBox::new(Orientation::Vertical, 2);
      b.append(&Label::builder().label(label).halign(Align::Start).build());

      let adj = Adjustment::new(val, min, max, step, step, 0.0);
      let scale = Scale::new(Orientation::Horizontal, Some(&adj));

      // Apply our custom class for the thin line look
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
  // SECTION 1: VIEW CONTROLS
  // ============================================================
  let controls_expander = Expander::new(Some("View Controls"));
  controls_expander.set_expanded(true);

  let controls_box = GtkBox::new(Orientation::Vertical, 15);
  controls_box.set_margin_top(10);
  controls_box.set_margin_bottom(10);
  controls_box.set_margin_start(5);

  // Common weak ref generator for closures
  // Instead of cloning DrawingArea, we downgrade the Notebook
  let nb_weak = notebook.downgrade();

  // Helper to queue draw on the ACTIVE tab
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

  // --- ATOMS ---
  let frame_atoms = Frame::new(Some("Atoms"));
  let vbox_atoms = GtkBox::new(Orientation::Vertical, 10);
  vbox_atoms.set_margin_top(10);
  vbox_atoms.set_margin_bottom(10);
  vbox_atoms.set_margin_start(10);
  vbox_atoms.set_margin_end(10);

  // Size Scale
  let s_as = state.clone();
  let nb_as = nb_weak.clone();
  let cb_as = queue_active_draw.clone();
  vbox_atoms.append(&create_slider(
    "Size Scale",
    0.1,
    2.0,
    0.05,
    state.borrow().active_tab().style.atom_scale,
    Box::new(move |v| {
      s_as.borrow_mut().active_tab_mut().style.atom_scale = v;
      cb_as(&nb_as);
    }),
  ));

  vbox_atoms.append(&Separator::new(Orientation::Horizontal));

  // Element List
  let atoms_list_container = GtkBox::new(Orientation::Vertical, 5);
  vbox_atoms.append(&atoms_list_container);

  // UPDATED CALL: Pass notebook
  refresh_atom_list(&atoms_list_container, state.clone(), notebook);

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

  // Tolerance
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
  box_bcol.append(&Label::new(Some("Color")));

  let btn_bcol = ColorButton::new();
  let (br, bg, bb) = state.borrow().active_tab().style.bond_color;
  btn_bcol.set_rgba(&gdk::RGBA::new(br as f32, bg as f32, bb as f32, 1.0));

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

  (scroll, atoms_list_container)
}

/// Public helper to rebuild the list of atom colors dynamically
/// UPDATED: Accepts &Notebook instead of &DrawingArea
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
    let nb_weak = notebook.downgrade(); // Capture weak ref for the loop

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
        tab.style.atom_cache.borrow_mut().remove(&elem_key);

        // Redraw active tab
        if let Some(nb) = nb_inner.upgrade() {
          if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
            da.queue_draw();
          }
        }
      });
      row.append(&btn);

      // Reset Button
      let btn_reset = Button::with_label("↺");
      let s_r = state.clone();
      let nb_r = nb_weak.clone();
      let elem_key_r = elem.clone();
      let btn_ref = btn.clone();

      btn_reset.connect_clicked(move |_| {
        let mut st = s_r.borrow_mut();
        let tab = st.active_tab_mut();

        tab.style.element_colors.remove(&elem_key_r);
        tab.style.atom_cache.borrow_mut().remove(&elem_key_r);

        let (_, def) = get_atom_properties(&elem_key_r);
        btn_ref.set_rgba(&gdk::RGBA::new(
          def.0 as f32,
          def.1 as f32,
          def.2 as f32,
          1.0,
        ));

        // Redraw active tab
        if let Some(nb) = nb_r.upgrade() {
          if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
            da.queue_draw();
          }
        }
      });
      row.append(&btn_reset);

      container.append(&row);
    }
  }
}
