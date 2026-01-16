// src/rendering/export.rs

use super::{painter, scene};
use crate::state::{AppState, TabState};
use gtk4::cairo;
use gtk4::prelude::*;
use gtk4::{DrawingArea, GestureClick, PropagationPhase};
use std::cell::RefCell;
use std::rc::Rc;

// --- Screen Rendering (Interactive) ---
// This might be used by secondary windows or dialogs
pub fn setup_drawing(drawing_area: &DrawingArea, state: Rc<RefCell<AppState>>) {
    // 1. CLICK HANDLER (Ray Picking)
    let gesture = GestureClick::new();
    gesture.set_button(0);
    gesture.set_propagation_phase(PropagationPhase::Capture);

    let state_clone = state.clone();
    let da_clone = drawing_area.clone();

    gesture.connect_pressed(move |gesture, _n_press, x, y| {
        let widget = gesture.widget();
        let width = widget.width() as f64;
        let height = widget.height() as f64;

        let mut st = state_clone.borrow_mut();

        // FIX: Access Active Tab
        // We need to calculate scene to know where atoms are
        let (render_atoms, _, _) = scene::calculate_scene(
            st.active_tab(),
            &st.config,
            width,
            height,
            false,
            None,
            None,
        );

        let mut closest_index = None;
        let mut min_dist = 40.0;

        for atom in render_atoms.iter() {
            let dx = atom.screen_pos[0] - x;
            let dy = atom.screen_pos[1] - y;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist < min_dist {
                min_dist = dist;
                closest_index = Some(atom.original_index);
            }
        }

        if let Some(idx) = closest_index {
            // FIX: Toggle selection via AppState helper
            st.toggle_selection(idx);
            da_clone.queue_draw();
        }
    });

    drawing_area.add_controller(gesture);

    // 2. DRAW FUNCTION
    drawing_area.set_draw_func(move |_, cr: &cairo::Context, width_px, height_px| {
        let st = state.borrow();
        let tab = st.active_tab();

        // Background (Use Tab Style)
        let (r, g, b) = tab.style.background_color;
        cr.set_source_rgb(r, g, b);
        cr.paint().expect("paint failed");

        let w = width_px as f64;
        let h = height_px as f64;

        let (render_atoms, lattice_corners, bounds) =
            scene::calculate_scene(tab, &st.config, w, h, false, None, None);

        painter::draw_unit_cell(cr, &lattice_corners, false);
        painter::draw_structure(cr, &render_atoms, tab, bounds.scale, false);
        painter::draw_axes(cr, tab, w, h);
    });
}

// --- EXPORT FUNCTIONS (Split for clarity and fix E0432) ---

pub fn export_png(state: Rc<RefCell<AppState>>, width: f64, height: f64, path: &str) -> String {
    let st = state.borrow();
    let tab = st.active_tab();

    // 1. Define High Res Scale or use dimensions
    // If width/height are 0, we could auto-crop, but for now we rely on passed dims
    // or we calculate based on the bounding box if we wanted "Export Selection".
    // Here we stick to "Export Viewport" logic but at high res.

    // For auto-crop logic (similar to your old export_image):
    let export_scale = 100.0; // High res

    // Calculate Scene (Dry Run for Bounds)
    let (render_atoms, lattice_corners, bounds) = scene::calculate_scene(
        tab,
        &st.config,
        0.0,
        0.0,
        true, // is_export
        Some(export_scale),
        None,
    );

    let img_width = bounds.width;
    let img_height = bounds.height;

    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, img_width as i32, img_height as i32)
            .expect("Couldn't create PNG surface");
    let cr = cairo::Context::new(&surface).expect("Couldn't create context");

    // Transparent Background
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.paint().unwrap();

    painter::draw_unit_cell(&cr, &lattice_corners, true);
    painter::draw_structure(&cr, &render_atoms, tab, export_scale, true);
    painter::draw_miller_planes(
        &cr,
        tab,
        &lattice_corners,
        export_scale,
        img_width,
        img_height,
    );
    painter::draw_axes(&cr, tab, img_width, img_height);

    let mut file = std::fs::File::create(path).expect("Couldn't create file");
    match surface.write_to_png(&mut file) {
        Ok(_) => format!("Exported PNG to: {}", path),
        Err(e) => format!("Failed to write PNG: {}", e),
    }
}

pub fn export_pdf(state: Rc<RefCell<AppState>>, path: &str) -> String {
    let st = state.borrow();
    let tab = st.active_tab();

    let export_scale = 80.0; // Reasonable vector scale

    let (render_atoms, lattice_corners, bounds) =
        scene::calculate_scene(tab, &st.config, 0.0, 0.0, true, Some(export_scale), None);

    let img_width = bounds.width;
    let img_height = bounds.height;

    let surface =
        cairo::PdfSurface::new(img_width, img_height, path).expect("Couldn't create PDF surface");
    let cr = cairo::Context::new(&surface).expect("Couldn't create context");

    // Background (White for PDF usually, or Transparent)
    // cr.set_source_rgb(1.0, 1.0, 1.0);
    // cr.paint().unwrap();

    painter::draw_unit_cell(&cr, &lattice_corners, true);
    painter::draw_structure(&cr, &render_atoms, tab, export_scale, true);
    painter::draw_miller_planes(
        &cr,
        tab,
        &lattice_corners,
        export_scale,
        img_width,
        img_height,
    );

    // Axes often look weird in PDF export if they are "screen space overlays",
    // but we can include them if desired:
    // painter::draw_axes(&cr, tab, img_width, img_height);

    surface.finish();
    format!("Exported PDF to: {}", path)
}
