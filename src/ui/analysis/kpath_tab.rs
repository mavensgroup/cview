// src/ui/analysis/kpath_tab.rs

use crate::physics::analysis::kpath;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{
  Align, Box, DrawingArea, Frame, GestureDrag, Label, Orientation, ScrolledWindow, TextView,
};
use std::cell::RefCell;
use std::rc::Rc;

struct ViewerState {
  rot_x: f64,
  rot_y: f64,
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
  let root = Box::new(Orientation::Horizontal, 15);
  root.set_margin_top(15);
  root.set_margin_bottom(15);
  root.set_margin_start(15);
  root.set_margin_end(15);

  let st = state.borrow();
  let k_result = if let Some(structure) = &st.structure {
    kpath::calculate_kpath(structure)
  } else {
    None
  };

  if let Some(res) = k_result {
    // ================= LEFT PANE: 3D Visualization =================
    let left_pane = Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);
    let frame_vis = Frame::new(Some("Brillouin Zone & K-Path"));

    let da = DrawingArea::new();
    da.set_content_height(400);
    da.set_content_width(400);
    da.set_vexpand(true);

    let view_state = Rc::new(RefCell::new(ViewerState {
      rot_x: 0.2,
      rot_y: 0.2,
    }));

    // Mouse Drag
    let gesture = GestureDrag::new();
    let vs_clone = view_state.clone();
    let da_clone = da.clone();
    gesture.connect_drag_update(move |_, x, y| {
      let mut vs = vs_clone.borrow_mut();
      vs.rot_y += x * 0.01;
      vs.rot_x += y * 0.01;
      da_clone.queue_draw();
    });
    da.add_controller(gesture);

    // Drawing
    let res_rc = Rc::new(res.clone());
    let res_draw = res_rc.clone();

    da.set_draw_func(move |_, cr, w, h| {
      // 1. White Background (Coherence with XRD tab)
      cr.set_source_rgb(1.0, 1.0, 1.0);
      cr.paint().unwrap();

      let vs = view_state.borrow();
      let center_x = w as f64 / 2.0;
      let center_y = h as f64 / 2.0;

      // 2. Adjusted Zoom (Reduced from 0.35 to 0.25)
      // This prevents the BZ from clipping edges during rotation
      let scale = (w as f64).min(h as f64) * 0.15;

      // Simple 3D projection
      let project = |p: [f64; 3]| -> (f64, f64) {
        let x = p[0];
        let y = p[1];
        let z = p[2];
        // Rotate around X
        let y_rot = y * vs.rot_x.cos() - z * vs.rot_x.sin();
        let z_rot = y * vs.rot_x.sin() + z * vs.rot_x.cos();
        // Rotate around Y
        let x_final = x * vs.rot_y.cos() + z_rot * vs.rot_y.sin();
        let y_final = y_rot;
        (center_x + x_final * scale, center_y - y_final * scale)
      };

      // 3. Draw Wireframe (BZ)
      // Use dark grey for lines on white background
      cr.set_source_rgba(0.2, 0.2, 0.2, 1.0);
      cr.set_line_width(1.5);
      for (start, end) in &res_draw.bz_lines {
        let (x1, y1) = project(*start);
        let (x2, y2) = project(*end);
        cr.move_to(x1, y1);
        cr.line_to(x2, y2);
      }
      cr.stroke().unwrap();

      // 4. Draw Path Segments
      cr.set_source_rgba(0.84, 0.0, 0.0, 1.0); // Red path
      cr.set_line_width(2.5);

      for segment in &res_draw.path_segments {
        if segment.is_empty() {
          continue;
        }
        let (sx, sy) = project(segment[0].coords_cart);
        cr.move_to(sx, sy);

        for pt in segment.iter().skip(1) {
          let (px, py) = project(pt.coords_cart);
          cr.line_to(px, py);
        }
      }
      cr.stroke().unwrap();

      // 5. Draw Labels
      cr.set_source_rgba(0.0, 0.84, 0.84, 1.0); // Blue dots/text
      for pt in &res_draw.kpoints {
        let (px, py) = project(pt.coords_cart);
        cr.arc(px, py, 4.0, 0.0, 2.0 * std::f64::consts::PI);
        cr.fill().unwrap();

        // Draw Label Text
        cr.move_to(px + 6.0, py - 6.0);
        cr.set_font_size(16.0);
        cr.show_text(&pt.label).unwrap();
      }
    });

    frame_vis.set_child(Some(&da));
    left_pane.append(&frame_vis);
    root.append(&left_pane);

    // ================= RIGHT PANE: VASP KPOINTS =================
    let right_pane = Box::new(Orientation::Vertical, 10);
    right_pane.set_width_request(350);

    let lbl_sg = Label::new(Some(&format!("Space Group: {}", res.spacegroup_str)));
    lbl_sg.set_halign(Align::Start);

    let lbl_bravais = Label::new(Some(&format!("Lattice: {}", res.lattice_type)));
    lbl_bravais.set_halign(Align::Start);

    // Generate Path String manually for display
    let mut path_display = String::new();
    for (i, segment) in res.path_segments.iter().enumerate() {
      if i > 0 {
        path_display.push_str(" | ");
      }
      let seg_str: Vec<String> = segment.iter().map(|p| p.label.clone()).collect();
      path_display.push_str(&seg_str.join("-"));
    }
    let lbl_path = Label::new(Some(&format!("Path: {}", path_display)));
    lbl_path.set_halign(Align::Start);

    right_pane.append(&lbl_sg);
    right_pane.append(&lbl_bravais);
    right_pane.append(&lbl_path);

    let tv = TextView::builder()
      .monospace(true)
      .editable(false)
      .vexpand(true)
      .build();

    // Generate VASP KPOINTS content
    let mut vasp_str = String::new();
    vasp_str.push_str("KPOINTS file for VASP\n");
    vasp_str.push_str("20 ! intersections\n");
    vasp_str.push_str("Line_mode\n");
    vasp_str.push_str("Reciprocal\n");

    for segment in &res.path_segments {
      for i in 0..segment.len().saturating_sub(1) {
        let p1 = &segment[i];
        let p2 = &segment[i + 1];
        vasp_str.push_str(&format!(
          "{:.6} {:.6} {:.6} ! {}\n",
          p1.coords_frac[0], p1.coords_frac[1], p1.coords_frac[2], p1.label
        ));
        vasp_str.push_str(&format!(
          "{:.6} {:.6} {:.6} ! {}\n\n",
          p2.coords_frac[0], p2.coords_frac[1], p2.coords_frac[2], p2.label
        ));
      }
    }
    tv.buffer().set_text(&vasp_str);

    let scroll = ScrolledWindow::builder().child(&tv).build();
    let frame_txt = Frame::new(Some("VASP KPOINTS"));
    frame_txt.set_child(Some(&scroll));
    frame_txt.set_vexpand(true);

    right_pane.append(&frame_txt);
    root.append(&right_pane);
  } else {
    root.append(&Label::new(Some(
      "No structure loaded or symmetry analysis failed.",
    )));
  }

  root
}
