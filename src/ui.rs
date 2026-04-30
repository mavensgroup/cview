// src/ui.rs

pub mod analysis;
pub mod dialogs;
pub mod export_dialog;
pub mod interactions;
pub mod preferences;

// Re-exports
pub use interactions::setup_interactions;
pub use preferences::show_preferences_window;

use crate::config::ColorMode;
use crate::rendering;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, DrawingArea, Label, Notebook, Orientation, Widget};
use std::cell::RefCell;
use std::rc::Rc;

/// Helper: Creates the DrawingArea and wrapping Box for a specific Tab ID.
pub fn create_tab_content(state: Rc<RefCell<AppState>>, tab_id: usize) -> (DrawingArea, GtkBox) {
  let drawing_area = DrawingArea::new();
  drawing_area.set_vexpand(true);
  drawing_area.set_hexpand(true);

  let s = state.clone();
  let tid = tab_id;

  drawing_area.set_draw_func(move |_, cr, w, h| {
    // Pre-calculate BVS before immutable borrow
    {
      let mut st = s.borrow_mut();
      if tid < st.tabs.len() {
        let tab = &mut st.tabs[tid];
        if matches!(tab.style.color_mode, ColorMode::BondValence) {
          let _ = tab.get_bvs_values();
        }
      }
    }

    let st = s.borrow();

    if tid >= st.tabs.len() {
      return;
    }

    let tab = &st.tabs[tid];

    // 1. Background
    let (bg_r, bg_g, bg_b) = tab.style.background_color;
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.paint().unwrap();

    // 2. Calculate Scene
    let (atoms, lattice_corners, bounds) =
      rendering::scene::calculate_scene(tab, &st.config, w as f64, h as f64, false, None, None);

    // 3. Draw Elements
    rendering::painter::draw_unit_cell(cr, &lattice_corners, false);
    rendering::painter::draw_structure(cr, &atoms, tab, bounds.scale, false, st.config.color_scheme);
    rendering::painter::draw_miller_planes(
      cr,
      tab,
      &lattice_corners,
      bounds.scale,
      w as f64,
      h as f64,
    );
    rendering::painter::draw_axes(cr, tab, w as f64, h as f64);
    rendering::painter::draw_selection_box(cr, tab);
  });

  let container = GtkBox::new(Orientation::Vertical, 0);
  container.append(&drawing_area);

  (drawing_area, container)
}

/// HELPER: closing tab
pub fn add_closable_tab(
  notebook: &Notebook,
  content: &impl IsA<Widget>,
  title: &str,
  state: Rc<RefCell<AppState>>,
) {
  let label_box = GtkBox::new(Orientation::Horizontal, 8);
  let label_text = Label::new(Some(title));

  let close_btn = Button::from_icon_name("window-close-symbolic");
  close_btn.set_has_frame(false);
  close_btn.set_tooltip_text(Some("Close Tab"));
  close_btn.set_valign(gtk4::Align::Center);

  label_box.append(&label_text);
  label_box.append(&close_btn);
  label_box.show();

  notebook.append_page(content, Some(&label_box));

  let nb_weak = notebook.downgrade();
  let content_weak = content.downgrade();
  let state_weak = Rc::downgrade(&state);

  close_btn.connect_clicked(move |_| {
    if let (Some(nb), Some(content_widget), Some(st)) = (
      nb_weak.upgrade(),
      content_weak.upgrade(),
      state_weak.upgrade(),
    ) {
      if let Some(page_num) = nb.page_num(&content_widget) {
        st.borrow_mut().remove_tab(page_num as usize);
        nb.remove_page(Some(page_num));
      }
    }
  });
}

/// Finds the DrawingArea inside the currently active Notebook tab.
pub fn get_active_drawing_area(notebook: &Notebook) -> Option<DrawingArea> {
  if let Some(page) = notebook.nth_page(notebook.current_page()) {
    if let Some(da) = page.downcast_ref::<DrawingArea>() {
      return Some(da.clone());
    }

    let mut child = page.first_child();
    while let Some(widget) = child {
      if let Some(da) = widget.downcast_ref::<DrawingArea>() {
        return Some(da.clone());
      }

      let mut sub_child = widget.first_child();
      while let Some(sub) = sub_child {
        if let Some(da) = sub.downcast_ref::<DrawingArea>() {
          return Some(da.clone());
        }
        sub_child = sub.next_sibling();
      }

      child = widget.next_sibling();
    }
  }
  None
}
