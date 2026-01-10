use gtk4::prelude::*;
use gtk4::{Box, Orientation, Label, TextView, ScrolledWindow, DrawingArea, Frame, GestureDrag, Align, PolicyType};
use std::rc::Rc;
use std::cell::RefCell;
use std::f64::consts::PI;
use crate::state::AppState;
use crate::physics::kpath; // Removed unused KPathResult import

// Local state for the 3D Viewer inside this tab
struct ViewerState {
    rot_x: f64,
    rot_y: f64,
}

pub fn build(state: Rc<RefCell<AppState>>) -> Box {
    // Root Layout (Horizontal)
    let root = Box::new(Orientation::Horizontal, 15);
    root.set_margin_top(15);
    root.set_margin_bottom(15);
    root.set_margin_start(15);
    root.set_margin_end(15);

    // 1. Calculate K-Path immediately
    let st = state.borrow();
    let k_result = if let Some(structure) = &st.structure {
        kpath::calculate_kpath(structure)
    } else {
        None
    };

    if let Some(res) = k_result {
        // ================= LEFT PANE: 3D Visualization =================
        let left_pane = Box::new(Orientation::Vertical, 5);
        left_pane.set_hexpand(true); // Take available width

        let frame_vis = Frame::new(Some("Brillouin Zone & K-Path"));

        let da = DrawingArea::new();
        da.set_content_height(400);
        da.set_content_width(500);
        da.set_hexpand(true);
        da.set_vexpand(true);

        // Create a local state for rotation
        let view_state = Rc::new(RefCell::new(ViewerState {
            rot_x: 0.3, // Initial tilt
            rot_y: 0.3,
        }));

        let res_clone = res.clone();
        let view_state_draw = view_state.clone();

        // --- DRAWING FUNCTION (Reciprocal Space) ---
        da.set_draw_func(move |_, cr, w, h| {
            // A. Background (White)
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();

            let width = w as f64;
            let height = h as f64;
            let cx = width / 2.0;
            let cy = height / 2.0;
            let scale = f64::min(width, height) * 0.40; // Scale factor

            let vs = view_state_draw.borrow();
            let (sin_x, cos_x) = vs.rot_x.sin_cos();
            let (sin_y, cos_y) = vs.rot_y.sin_cos();

            // Helper: 3D Rotation -> 2D Screen Projection
            let project = |v: [f64; 3]| -> (f64, f64) {
                let x = v[0]; let y = v[1]; let z = v[2];
                // Rotate around X
                let y1 = y * cos_x - z * sin_x;
                let z1 = y * sin_x + z * cos_x;
                // Rotate around Y
                let x2 = x * cos_y - z1 * sin_y;

                // Screen coordinates (Y flipped for GTK)
                (cx + x2 * scale, cy - y1 * scale)
            };

            // B. Draw Reciprocal Axes (XYZ)
            let axes = [
                ([1.2, 0.0, 0.0], (0.8, 0.2, 0.2)), // b1 (Red)
                ([0.0, 1.2, 0.0], (0.2, 0.6, 0.2)), // b2 (Green)
                ([0.0, 0.0, 1.2], (0.2, 0.2, 0.8)), // b3 (Blue)
            ];
            cr.set_line_width(2.0);
            for (v_end, color) in axes {
                let (sx, sy) = project([0.0, 0.0, 0.0]);
                let (ex, ey) = project(v_end);
                cr.set_source_rgb(color.0, color.1, color.2);
                cr.move_to(sx, sy);
                cr.line_to(ex, ey);
                cr.stroke().unwrap();
            }

            // C. Draw Brillouin Zone Wireframe (Gray)
            cr.set_line_width(1.0);
            cr.set_source_rgba(0.4, 0.4, 0.4, 0.6);

            for (start, end) in &res_clone.bz_lines {
                let (x1, y1) = project(*start);
                let (x2, y2) = project(*end);
                cr.move_to(x1, y1);
                cr.line_to(x2, y2);
                cr.stroke().unwrap();
            }

            // D. Draw K-Path (Thick Red Lines)
            cr.set_line_width(2.5);
            cr.set_source_rgb(0.9, 0.1, 0.1); // Bright Red

            let kpts = &res_clone.kpoints;
            if kpts.len() > 1 {
                for i in 0..kpts.len()-1 {
                    let (x1, y1) = project(kpts[i].coords);
                    let (x2, y2) = project(kpts[i+1].coords);
                    cr.move_to(x1, y1);
                    cr.line_to(x2, y2);
                    cr.stroke().unwrap();
                }
            }

            // E. Draw K-Points Labels (Nodes)
            for pt in kpts {
                let (px, py) = project(pt.coords);

                // Draw Dot
                cr.set_source_rgb(0.0, 0.0, 0.0);
                cr.arc(px, py, 3.5, 0.0, 2.0 * PI);
                cr.fill().unwrap();

                // Draw Label
                cr.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
                cr.set_font_size(13.0);
                cr.move_to(px + 6.0, py - 6.0);
                cr.show_text(&pt.label).unwrap();
            }
        });

        // 2. INTERACTION (Drag to Rotate)
        let drag = GestureDrag::new();
        let view_state_drag = view_state.clone();
        let da_drag = da.clone();

        drag.connect_drag_update(move |_gesture, dx, dy| {
            let mut vs = view_state_drag.borrow_mut();
            vs.rot_y += dx * 0.01;
            vs.rot_x += dy * 0.01;
            da_drag.queue_draw();
        });

        da.add_controller(drag);

        frame_vis.set_child(Some(&da));
        left_pane.append(&frame_vis);
        root.append(&left_pane);


        // ================= RIGHT PANE: Output & Info =================
        let right_pane = Box::new(Orientation::Vertical, 10);
        right_pane.set_width_request(300); // Fixed sidebar width

        // Header
        let title = Label::new(Some("K-Path Generator"));
        title.add_css_class("title-2");
        title.set_halign(Align::Start);
        right_pane.append(&title);

        let lbl_sg = Label::new(Some(&format!("Space Group: {}", res.spacegroup)));
        lbl_sg.set_halign(Align::Start);
        right_pane.append(&lbl_sg);

        let lbl_path = Label::new(Some(&format!("Path: {}", res.path_string)));
        lbl_path.set_halign(Align::Start);
        lbl_path.set_wrap(true);
        lbl_path.set_margin_bottom(10);
        right_pane.append(&lbl_path);

        // VASP Output Area
        right_pane.append(&Label::new(Some("VASP KPOINTS (Line Mode):")));

        let tv = TextView::new();
        tv.set_editable(false);
        tv.set_monospace(true);

        let mut vasp_str = String::new();
        vasp_str.push_str("K-Path generated by CView\n");
        vasp_str.push_str("20  ! Intersections\n");
        vasp_str.push_str("Line_mode\n");
        vasp_str.push_str("Reciprocal\n");

        if res.kpoints.len() > 1 {
            for i in 0..res.kpoints.len()-1 {
                let p1 = &res.kpoints[i];
                let p2 = &res.kpoints[i+1];
                vasp_str.push_str(&format!("{:.5} {:.5} {:.5} ! {}\n", p1.coords[0], p1.coords[1], p1.coords[2], p1.label));
                vasp_str.push_str(&format!("{:.5} {:.5} {:.5} ! {}\n", p2.coords[0], p2.coords[1], p2.coords[2], p2.label));
                vasp_str.push_str("\n");
            }
        }

        tv.buffer().set_text(&vasp_str);

        let scroll = ScrolledWindow::new();
        scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
        scroll.set_child(Some(&tv));
        scroll.set_vexpand(true);

        // Add a nice frame around the text
        let text_frame = Frame::new(None);
        text_frame.set_child(Some(&scroll));
        right_pane.append(&text_frame);

        root.append(&right_pane);

    } else {
        // Fallback if no structure/path found
        let msg = Label::new(Some("No K-Path detected.\nLoad a structure or check symmetry."));
        msg.set_justify(gtk4::Justification::Center);
        msg.set_valign(gtk4::Align::Center);
        msg.set_vexpand(true);
        msg.set_hexpand(true);
        root.append(&msg);
    }

    root
}
