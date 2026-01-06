use gtk4::DrawingArea;
use gtk4::prelude::*;
use gtk4::{GestureClick, PropagationPhase};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use super::{scene, painter};

// --- Screen Rendering (Interactive) ---
pub fn setup_drawing(drawing_area: &DrawingArea, state: Rc<RefCell<AppState>>) {
    // 1. CLICK HANDLER (Ray Picking)
    let gesture = GestureClick::new();
    gesture.set_button(0);
    gesture.set_propagation_phase(PropagationPhase::Capture);

    let state_clone = state.clone();
    let da_clone = drawing_area.clone();

    gesture.connect_pressed(move |gesture, n_press, x, y| {
        let widget = gesture.widget().expect("Gesture has no widget");
        let width = widget.width() as f64;
        let height = widget.height() as f64;

        // DEBUG: Print current selection state
        let mut state = state_clone.borrow_mut();
        println!("Click at ({:.1}, {:.1}). Screen Size: {}x{}", x, y, width, height);

        let (render_atoms, _, _) = scene::calculate_scene(&state, width, height, false, None, None);

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
            println!(">> HIT Atom {} (Dist: {:.1}px)", idx, min_dist);
            state.toggle_selection(idx);
            da_clone.queue_draw();
        } else {
            println!(">> MISSED all atoms. Closest was {:.1}px away", min_dist);
        }
    });

    drawing_area.add_controller(gesture);

    // 2. DRAW FUNCTION
    drawing_area.set_draw_func(move |_, cr: &cairo::Context, width_px, height_px| {
        let state = state.borrow();
        cr.set_source_rgb(0.05, 0.05, 0.1);
        cr.paint().expect("paint failed");

        let w = width_px as f64;
        let h = height_px as f64;

        let (render_atoms, lattice_corners, bounds) = scene::calculate_scene(&state, w, h, false, None, None);

        painter::draw_unit_cell(cr, &lattice_corners, false);
        painter::draw_structure(cr, &render_atoms, &state, bounds.scale, false);
        painter::draw_axes(cr, &state, w, h);
    });
}

// --- EXPORT FUNCTION ---

pub fn export_image(state: &AppState, path: &str, _w_ignored: f64, _h_ignored: f64, is_pdf: bool) {
    // 1. Define Scale
    // 100 pixels per Angstrom = High Resolution
    let export_scale = 100.0;

    // 2. Calculate Scene FIRST (Dry Run)
    // We pass 0.0, 0.0 for width/height because 'is_export=true' tells scene.rs
    // to ignore those and calculate the tightest bounding box automatically.
    let (render_atoms, lattice_corners, bounds) = scene::calculate_scene(
        state,
        0.0, 0.0, // Ignored
        true,     // is_export
        Some(export_scale),
        None
    );

    // 3. Get the calculated dimensions from the scene bounds
    let img_width = bounds.width;
    let img_height = bounds.height;

    println!("Auto-Crop Size: {:.0}x{:.0} px", img_width, img_height);

    if is_pdf {
        // --- PDF EXPORT ---
        let surface = cairo::PdfSurface::new(img_width, img_height, path)
            .expect("Couldn't create PDF surface");
        let cr = cairo::Context::new(&surface).expect("Couldn't create context");

        // Draw using the pre-calculated, perfectly centered atoms
        painter::draw_unit_cell(&cr, &lattice_corners, true);
        painter::draw_structure(&cr, &render_atoms, state, export_scale, true);

        surface.finish();
        println!("Exported PDF to: {}", path);

    } else {
        // --- PNG EXPORT ---
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, img_width as i32, img_height as i32)
            .expect("Couldn't create PNG surface");
        let cr = cairo::Context::new(&surface).expect("Couldn't create context");

        // Transparent Background
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.paint().unwrap();

        painter::draw_unit_cell(&cr, &lattice_corners, true);
        painter::draw_structure(&cr, &render_atoms, state, export_scale, true);

        let mut file = std::fs::File::create(path).expect("Couldn't create file");
        surface.write_to_png(&mut file).expect("Couldn't write png");
        println!("Exported PNG to: {}", path);
    }
}
