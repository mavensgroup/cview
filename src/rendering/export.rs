use gtk4::DrawingArea;
use gtk4::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use super::{scene, painter}; // Access sibling modules

// --- Screen Rendering Entry Point ---
pub fn setup_drawing(drawing_area: &DrawingArea, state: Rc<RefCell<AppState>>) {
    drawing_area.set_draw_func(move |_, cr: &cairo::Context, width, height| {
        let state = state.borrow();

        // Background
        cr.set_source_rgb(0.05, 0.05, 0.1);
        cr.paint().expect("paint failed");

        let (render_atoms, lattice_corners, bounds) = scene::calculate_scene(&state, width as f64, height as f64, false, None, None);

        // 1. Draw Unit Cell (Behind everything mostly)
        painter::draw_unit_cell(cr, &lattice_corners, false);

        // 2. Draw Structure (Atoms + Bonds sorted)
        painter::draw_structure(cr, &render_atoms, &state, bounds.scale, false);

        // 3. Draw Axes
        painter::draw_axes(cr, &state, width as f64, height as f64);
    });
}

// --- File Export Entry Point ---
pub fn export_image(
    state: &AppState,
    path: &str,
    _req_w: i32,
    _req_h: i32,
    format_pdf: bool
) -> Result<(), String> {

    let (render_atoms, lattice_corners, bounds) = scene::calculate_scene(state, 0.0, 0.0, true, Some(80.0 * state.zoom), None);
    let width = bounds.width as i32;
    let height = bounds.height as i32;

    if format_pdf {
        let surface = cairo::PdfSurface::new(width as f64, height as f64, path).map_err(|e| e.to_string())?;
        let cr = cairo::Context::new(&surface).expect("Failed to create context");

        cr.set_source_rgb(1.0, 1.0, 1.0); // White BG for PDF
        cr.paint().unwrap();

        painter::draw_unit_cell(&cr, &lattice_corners, true);
        painter::draw_structure(&cr, &render_atoms, state, bounds.scale, true);

        surface.finish();
    } else {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).map_err(|e| e.to_string())?;
        let cr = cairo::Context::new(&surface).expect("Failed to create context");

        // cr.set_source_rgb(1.0, 1.0, 1.0); cr.paint().unwrap(); // Uncomment for White PNG BG

        painter::draw_unit_cell(&cr, &lattice_corners, true);
        painter::draw_structure(&cr, &render_atoms, state, bounds.scale, true);

        let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        surface.write_to_png(&mut file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
