use gtk4::{self as gtk, gdk, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
// use crate::state::{AppState, ExportFormat, RotationCenter};
use crate::config::{ExportFormat, RotationCenter};
use crate::state::AppState;

pub fn show_preferences_window(
  parent: &gtk::ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: gtk::DrawingArea, // <--- UNCOMMENT THIS
) {
  let window = gtk::Window::builder()
    .title("Preferences")
    .transient_for(parent)
    .modal(true) // Setting to true is usually better for Preferences
    .default_width(400)
    .default_height(400)
    .resizable(false)
    .build();

  let main_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
  let notebook = gtk::Notebook::new();
  notebook.set_vexpand(true);

  // --- Tab 1: Appearance ---
  // Pass the drawing_area so the color picker can trigger a redraw
  let appearance_tab = build_appearance_tab(state.clone(), drawing_area);
  notebook.append_page(&appearance_tab, Some(&gtk::Label::new(Some("Appearance"))));

  // --- Tab 2: Export / General ---
  let export_tab = build_export_tab(state.clone());
  notebook.append_page(
    &export_tab,
    Some(&gtk::Label::new(Some("Export & General"))),
  );

  main_vbox.append(&notebook);

  // Footer
  let footer_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
  footer_box.set_margin_top(10);
  footer_box.set_margin_bottom(10);
  footer_box.set_margin_end(10);
  footer_box.set_halign(gtk::Align::End);

  let btn_close = gtk::Button::with_label("Close");
  let win_clone = window.clone();
  btn_close.connect_clicked(move |_| win_clone.close());
  footer_box.append(&btn_close);
  main_vbox.append(&footer_box);

  window.set_child(Some(&main_vbox));
  window.present();
}

fn build_appearance_tab(state: Rc<RefCell<AppState>>, drawing_area: gtk::DrawingArea) -> gtk::Box {
  let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
  vbox.set_margin_top(15);
  vbox.set_margin_bottom(15);
  vbox.set_margin_start(15);
  vbox.set_margin_end(15);

  // --- Background Color Row ---
  let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
  let label = gtk::Label::new(Some("Background Color"));
  label.set_hexpand(true);
  label.set_halign(gtk::Align::Start);
  row.append(&label);

  let color_button = gtk::ColorButton::new();

  // Get initial color from state
  let bg_color = state.borrow().config.style.background_color;
  color_button.set_rgba(&gdk::RGBA::new(
    bg_color.0 as f32,
    bg_color.1 as f32,
    bg_color.2 as f32,
    1.0,
  ));

  let s = state.clone();
  let da = drawing_area.clone();

  color_button.connect_color_set(move |btn| {
    let rgba = btn.rgba();
    let mut st = s.borrow_mut();

    // Update state
    st.config.style.background_color = (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);

    // Save immediately to JSON
    st.save_config();

    // Force the screen to update
    da.queue_draw();
  });

  row.append(&color_button);
  vbox.append(&row);

  vbox
}

// ... build_export_tab stays the same as your snippet ...
//
fn build_export_tab(state: Rc<RefCell<AppState>>) -> gtk::Box {
  let container = gtk::Box::new(gtk::Orientation::Vertical, 10);

  // Margins
  container.set_margin_top(20);
  container.set_margin_bottom(20);
  container.set_margin_start(20);
  container.set_margin_end(20);

  // --- 1. Export Format ---
  let label_format = gtk::Label::builder()
    .label("Default Export Format:")
    .halign(gtk::Align::Start)
    .build();

  // Dropdown for PNG/PDF
  let format_options = ["PNG Image", "PDF Document"];
  let dropdown = gtk::DropDown::from_strings(&format_options);

  // Set initial selection
  let is_pdf = matches!(
    state.borrow().config.default_export_format,
    ExportFormat::Pdf
  );
  dropdown.set_selected(if is_pdf { 1 } else { 0 });

  // Connect Signal
  let s = state.clone();
  dropdown.connect_selected_notify(move |d| {
    let idx = d.selected();
    let mut st = s.borrow_mut();
    st.config.default_export_format = match idx {
      1 => ExportFormat::Pdf,
      _ => ExportFormat::Png,
    };
    st.save_config();
    println!(
      "Preference saved: Default Export Format = {:?}",
      st.config.default_export_format
    );
  });

  // Helper text
  let info_label = gtk::Label::builder()
        .label("This sets the default format selected in the export dialog.\nYou can still change it manually when saving.")
        .wrap(true)
        .css_classes(["dim-label"])
        .margin_top(5)
        .halign(gtk::Align::Start)
        .build();
  info_label.set_opacity(0.7);

  container.append(&label_format);
  container.append(&dropdown);
  container.append(&info_label);

  // --- SEPARATOR ---
  container.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

  // --- 2. Rotation Center (NEW) ---
  let label_rot = gtk::Label::builder()
    .label("Default Rotation Center:")
    .halign(gtk::Align::Start)
    .margin_top(10)
    .build();

  let rot_options = ["Structure Centroid", "Unit Cell Center"];
  let dropdown_rot = gtk::DropDown::from_strings(&rot_options);

  // Set initial selection
  let is_unit_cell = matches!(
    state.borrow().config.rotation_mode,
    RotationCenter::UnitCell
  );
  dropdown_rot.set_selected(if is_unit_cell { 1 } else { 0 });

  // Connect Signal
  let s_rot = state.clone();
  dropdown_rot.connect_selected_notify(move |d| {
    let idx = d.selected();
    let mut st = s_rot.borrow_mut();
    st.config.rotation_mode = match idx {
      1 => RotationCenter::UnitCell,
      _ => RotationCenter::Centroid,
    };
    st.save_config();
    println!(
      "Preference saved: Rotation Mode = {:?}",
      st.config.rotation_mode
    );
  });

  // Helper text for rotation
  let rot_info_label = gtk::Label::builder()
    .label("Defines the pivot point for mouse rotation.")
    .wrap(true)
    .css_classes(["dim-label"])
    .margin_top(5)
    .halign(gtk::Align::Start)
    .build();
  rot_info_label.set_opacity(0.7);

  container.append(&label_rot);
  container.append(&dropdown_rot);
  container.append(&rot_info_label);

  container
}

// fn build_appearance_tab(
// state: Rc<RefCell<AppState>>,
// drawing_area: gtk::DrawingArea,
// ) -> gtk::ScrolledWindow {
// let scroll = gtk::ScrolledWindow::new();
// scroll.set_hscrollbar_policy(gtk::PolicyType::Never);

// let container = gtk::Box::new(gtk::Orientation::Vertical, 10);
// container.set_margin_top(15);
// container.set_margin_bottom(15);
// container.set_margin_start(15);
// container.set_margin_end(15);

// // --- 1. BSDF ---
// let frame_mat = gtk::Frame::new(Some("Material"));
// let vbox_mat = gtk::Box::new(gtk::Orientation::Vertical, 10);
// vbox_mat.set_margin_top(10); vbox_mat.set_margin_bottom(10);
// vbox_mat.set_margin_start(10); vbox_mat.set_margin_end(10);

// // Metallic
// vbox_mat.append(&gtk::Label::new(Some("Metallic")));
// let scale_met = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
// scale_met.set_value(state.borrow().style.metallic);
// let s = state.clone(); let da = drawing_area.clone();
// scale_met.connect_value_changed(move |sc| { s.borrow_mut().style.metallic = sc.value(); da.queue_draw(); });
// vbox_mat.append(&scale_met);

// // Roughness
// vbox_mat.append(&gtk::Label::new(Some("Roughness")));
// let scale_rough = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
// scale_rough.set_value(state.borrow().style.roughness);
// let s = state.clone(); let da = drawing_area.clone();
// scale_rough.connect_value_changed(move |sc| { s.borrow_mut().style.roughness = sc.value(); da.queue_draw(); });
// vbox_mat.append(&scale_rough);

// // Transmission
// vbox_mat.append(&gtk::Label::new(Some("Transmission")));
// let scale_trans = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
// scale_trans.set_value(state.borrow().style.transmission);
// let s = state.clone(); let da = drawing_area.clone();
// scale_trans.connect_value_changed(move |sc| { s.borrow_mut().style.transmission = sc.value(); da.queue_draw(); });
// vbox_mat.append(&scale_trans);

// frame_mat.set_child(Some(&vbox_mat));
// container.append(&frame_mat);

// // --- 2. ATOM COLORS (DYNAMIC LIST) ---
// let frame_atoms = gtk::Frame::new(Some("Element Colors"));
// let vbox_atoms = gtk::Box::new(gtk::Orientation::Vertical, 10);
// vbox_atoms.set_margin_top(10); vbox_atoms.set_margin_bottom(10);
// vbox_atoms.set_margin_start(10); vbox_atoms.set_margin_end(10);

// // Global Scale
// let box_scale = gtk::Box::new(gtk::Orientation::Horizontal, 10);
// box_scale.append(&gtk::Label::new(Some("Atom Size Scale")));
// let scale_atom = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.1, 2.0, 0.05);
// scale_atom.set_hexpand(true);
// scale_atom.set_value(state.borrow().style.atom_scale);
// let s = state.clone(); let da = drawing_area.clone();
// scale_atom.connect_value_changed(move |sc| { s.borrow_mut().style.atom_scale = sc.value(); da.queue_draw(); });
// box_scale.append(&scale_atom);
// vbox_atoms.append(&box_scale);

// vbox_atoms.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

// // Dynamic Element List
// let elements = if let Some(structure) = &state.borrow().structure {
// let mut unique: Vec<String> = structure.atoms.iter().map(|a| a.element.clone()).collect();
// unique.sort();
// unique.dedup();
// unique
// } else {
// vec![]
// };

// if elements.is_empty() {
// vbox_atoms.append(&gtk::Label::new(Some("(No structure loaded)")));
// } else {
// for elem in elements {
// let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);

// // Label (e.g., "C")
// let lbl = gtk::Label::new(Some(&format!("Element {}", elem)));
// lbl.set_width_chars(10);
// lbl.set_xalign(0.0);
// row.append(&lbl);

// // Current Color Lookup
// let current_color = {
// let st = state.borrow();
// if let Some(c) = st.style.element_colors.get(&elem) {
// *c
// } else {
// let (_, def) = get_atom_properties(&elem);
// def
// }
// };

// // Color Button
// let btn = gtk::ColorButton::new();
// btn.set_rgba(&gdk::RGBA::new(
// current_color.0 as f32,
// current_color.1 as f32,
// current_color.2 as f32,
// 1.0
// ));

// let s = state.clone();
// let da = drawing_area.clone();
// let elem_key = elem.clone();

// btn.connect_color_set(move |b| {
// let c = b.rgba();
// let new_col = (c.red() as f64, c.green() as f64, c.blue() as f64);
// s.borrow_mut().style.element_colors.insert(elem_key.clone(), new_col);
// da.queue_draw();
// });

// row.append(&btn);

// // Reset Button
// let btn_reset = gtk::Button::with_label("Reset");
// let s_r = state.clone();
// let da_r = drawing_area.clone();
// let elem_key_r = elem.clone();
// let btn_col_ref = btn.clone();

// btn_reset.connect_clicked(move |_| {
// s_r.borrow_mut().style.element_colors.remove(&elem_key_r);
// let (_, def) = get_atom_properties(&elem_key_r);
// btn_col_ref.set_rgba(&gdk::RGBA::new(def.0 as f32, def.1 as f32, def.2 as f32, 1.0));
// da_r.queue_draw();
// });

// row.append(&btn_reset);
// vbox_atoms.append(&row);
// }
// }

// frame_atoms.set_child(Some(&vbox_atoms));
// container.append(&frame_atoms);

// // --- 3. BONDS ---
// let frame_bonds = gtk::Frame::new(Some("Bonds"));
// let vbox_bonds = gtk::Box::new(gtk::Orientation::Vertical, 10);
// vbox_bonds.set_margin_top(10); vbox_bonds.set_margin_bottom(10);
// vbox_bonds.set_margin_start(10); vbox_bonds.set_margin_end(10);

// let box_brad = gtk::Box::new(gtk::Orientation::Horizontal, 10);
// box_brad.append(&gtk::Label::new(Some("Radius")));
// let scale_brad = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.01, 0.5, 0.01);
// scale_brad.set_hexpand(true);
// scale_brad.set_value(state.borrow().style.bond_radius);
// let s = state.clone(); let da = drawing_area.clone();
// scale_brad.connect_value_changed(move |sc| { s.borrow_mut().style.bond_radius = sc.value(); da.queue_draw(); });
// box_brad.append(&scale_brad);
// vbox_bonds.append(&box_brad);

// let box_bcol = gtk::Box::new(gtk::Orientation::Horizontal, 10);
// box_bcol.append(&gtk::Label::new(Some("Color")));
// let btn_bcol = gtk::ColorButton::new();
// let (br, bg, bb) = state.borrow().style.bond_color;
// btn_bcol.set_rgba(&gdk::RGBA::new(br as f32, bg as f32, bb as f32, 1.0));
// let s = state.clone(); let da = drawing_area.clone();
// btn_bcol.connect_color_set(move |b| {
// let c = b.rgba();
// s.borrow_mut().style.bond_color = (c.red() as f64, c.green() as f64, c.blue() as f64);
// da.queue_draw();
// });
// box_bcol.append(&btn_bcol);
// vbox_bonds.append(&box_bcol);

// frame_bonds.set_child(Some(&vbox_bonds));
// container.append(&frame_bonds);

// scroll.set_child(Some(&container));
// scroll
// }
