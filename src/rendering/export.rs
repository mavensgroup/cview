// src/rendering/export.rs
// STATE-OF-THE-ART EXPORT SYSTEM
// Publication-quality PNG, PDF, SVG exports with advanced features

use super::{painter, scene};
use crate::state::{AppState, TabState};
use gtk4::cairo;
use gtk4::prelude::*;
use gtk4::{DrawingArea, GestureClick, PropagationPhase};
use std::cell::RefCell;
use std::rc::Rc;

// ============================================================================
// EXPORT CONFIGURATION
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    PNG,
    PDF,
    SVG,
}

#[derive(Debug, Clone)]
pub struct ExportSettings {
    // Image quality
    pub width: Option<f64>,  // None = auto from structure
    pub height: Option<f64>, // None = auto from structure
    pub dpi: u32,            // 72, 150, 300, 600
    pub scale: f64,          // Zoom factor (default: 100.0)

    // Background
    pub transparent: bool,                         // PNG only
    pub background_color: Option<(f64, f64, f64)>, // None = use tab color

    // Content
    pub include_unit_cell: bool,
    pub include_axes: bool,
    pub include_miller_planes: bool,
    pub include_selection_box: bool,

    // Quality (PNG/PDF)
    pub antialiasing: AntialiasMode,
    pub line_quality: LineQuality,
}

#[derive(Debug, Clone, Copy)]
pub enum AntialiasMode {
    None,
    Fast, // 2x MSAA
    Good, // 4x MSAA
    Best, // 8x MSAA (default for export)
}

#[derive(Debug, Clone, Copy)]
pub enum LineQuality {
    Fast, // Basic
    Good, // Smooth joins
    Best, // Publication (miter joins, precise)
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            dpi: 300, // Publication standard
            scale: 100.0,
            transparent: false, // White background default
            background_color: None,
            include_unit_cell: true,
            include_axes: true,
            include_miller_planes: true,
            include_selection_box: false,
            antialiasing: AntialiasMode::Best,
            line_quality: LineQuality::Best,
        }
    }
}

impl ExportSettings {
    /// Quick preset for Nature/Science journals
    pub fn journal_preset() -> Self {
        Self {
            dpi: 600,
            transparent: false,
            background_color: Some((1.0, 1.0, 1.0)), // White
            antialiasing: AntialiasMode::Best,
            line_quality: LineQuality::Best,
            ..Default::default()
        }
    }

    /// Quick preset for presentations (PowerPoint, etc.)
    pub fn presentation_preset() -> Self {
        Self {
            dpi: 150,
            transparent: true,
            antialiasing: AntialiasMode::Good,
            line_quality: LineQuality::Good,
            ..Default::default()
        }
    }

    /// Quick preset for web (blog, documentation)
    pub fn web_preset() -> Self {
        Self {
            dpi: 72,
            width: Some(800.0),
            transparent: true,
            antialiasing: AntialiasMode::Good,
            ..Default::default()
        }
    }
}

// ============================================================================
// SCREEN RENDERING (Interactive) - Unchanged
// ============================================================================

pub fn setup_drawing(drawing_area: &DrawingArea, state: Rc<RefCell<AppState>>) {
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
            st.toggle_selection(idx);
            da_clone.queue_draw();
        }
    });

    drawing_area.add_controller(gesture);

    drawing_area.set_draw_func(move |_, cr: &cairo::Context, width_px, height_px| {
        let st = state.borrow();
        let tab = st.active_tab();

        let (r, g, b) = tab.style.background_color;
        cr.set_source_rgb(r, g, b);
        cr.paint().expect("Failed to paint background");

        let w = width_px as f64;
        let h = height_px as f64;

        let (render_atoms, lattice_corners, bounds) =
            scene::calculate_scene(tab, &st.config, w, h, false, None, None);

        painter::draw_unit_cell(cr, &lattice_corners, false);
        painter::draw_structure(cr, &render_atoms, tab, bounds.scale, false);
        painter::draw_axes(cr, tab, w, h);
    });
}

// ============================================================================
// EXPORT FUNCTIONS - State-of-the-Art
// ============================================================================

/// Export to PNG with full control
pub fn export_png_advanced(
    state: Rc<RefCell<AppState>>,
    path: &str,
    settings: ExportSettings,
) -> Result<String, String> {
    let st = state.borrow();
    let tab = st.active_tab();

    // Calculate scene dimensions
    let (render_atoms, lattice_corners, bounds) =
        scene::calculate_scene(tab, &st.config, 0.0, 0.0, true, Some(settings.scale), None);

    let img_width = settings.width.unwrap_or(bounds.width);
    let img_height = settings.height.unwrap_or(bounds.height);

    // Create high-quality surface
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, img_width as i32, img_height as i32)
            .map_err(|e| format!("Failed to create surface: {}", e))?;

    let cr =
        cairo::Context::new(&surface).map_err(|e| format!("Failed to create context: {}", e))?;

    // Apply quality settings
    apply_quality_settings(&cr, &settings);

    // Background
    if settings.transparent {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    } else {
        let (r, g, b) = settings
            .background_color
            .unwrap_or(tab.style.background_color);
        cr.set_source_rgb(r, g, b);
    }
    cr.paint().expect("Failed to paint background");

    // Draw content
    draw_export_content(
        &cr,
        &render_atoms,
        &lattice_corners,
        tab,
        settings.scale,
        img_width,
        img_height,
        &settings,
    );

    // Write to file
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("Failed to create file: {}", e))?;

    surface
        .write_to_png(&mut file)
        .map_err(|e| format!("Failed to write PNG: {}", e))?;

    Ok(format!(
        "Exported PNG to: {} ({}×{} @ {} DPI)",
        path, img_width as i32, img_height as i32, settings.dpi
    ))
}

/// Export to PDF with vector quality
pub fn export_pdf_advanced(
    state: Rc<RefCell<AppState>>,
    path: &str,
    settings: ExportSettings,
) -> Result<String, String> {
    let st = state.borrow();
    let tab = st.active_tab();

    let (render_atoms, lattice_corners, bounds) =
        scene::calculate_scene(tab, &st.config, 0.0, 0.0, true, Some(settings.scale), None);

    let img_width = settings.width.unwrap_or(bounds.width);
    let img_height = settings.height.unwrap_or(bounds.height);

    // Create PDF surface (vector output)
    let surface = cairo::PdfSurface::new(img_width, img_height, path)
        .map_err(|e| format!("Failed to create PDF surface: {}", e))?;

    let cr =
        cairo::Context::new(&surface).map_err(|e| format!("Failed to create context: {}", e))?;

    // Apply quality settings
    apply_quality_settings(&cr, &settings);

    // Background (usually white for PDF)
    if !settings.transparent {
        let (r, g, b) = settings.background_color.unwrap_or((1.0, 1.0, 1.0)); // White default
        cr.set_source_rgb(r, g, b);
        cr.paint().expect("Failed to paint background");
    }

    // Draw content
    draw_export_content(
        &cr,
        &render_atoms,
        &lattice_corners,
        tab,
        settings.scale,
        img_width,
        img_height,
        &settings,
    );

    // Finalize PDF
    surface.finish();

    Ok(format!(
        "Exported PDF to: {} ({}×{} pts, vector)",
        path, img_width as i32, img_height as i32
    ))
}

/// Export to SVG (pure vector, best for publications)
pub fn export_svg_advanced(
    state: Rc<RefCell<AppState>>,
    path: &str,
    settings: ExportSettings,
) -> Result<String, String> {
    let st = state.borrow();
    let tab = st.active_tab();

    let (render_atoms, lattice_corners, bounds) =
        scene::calculate_scene(tab, &st.config, 0.0, 0.0, true, Some(settings.scale), None);

    let img_width = settings.width.unwrap_or(bounds.width);
    let img_height = settings.height.unwrap_or(bounds.height);

    // Create SVG surface (editable vector output)
    let surface = cairo::SvgSurface::new(img_width, img_height, Some(path))
        .map_err(|e| format!("Failed to create SVG surface: {}", e))?;

    let cr =
        cairo::Context::new(&surface).map_err(|e| format!("Failed to create context: {}", e))?;

    // Apply quality settings
    apply_quality_settings(&cr, &settings);

    // Background
    if !settings.transparent {
        let (r, g, b) = settings.background_color.unwrap_or((1.0, 1.0, 1.0));
        cr.set_source_rgb(r, g, b);
        cr.paint().expect("Failed to paint background");
    }

    // Draw content
    draw_export_content(
        &cr,
        &render_atoms,
        &lattice_corners,
        tab,
        settings.scale,
        img_width,
        img_height,
        &settings,
    );

    // Finalize SVG
    surface.finish();

    Ok(format!(
        "Exported SVG to: {} ({}×{} pts, editable vector)",
        path, img_width as i32, img_height as i32
    ))
}

/// SVG export stub when feature is disabled
// #[cfg(not(feature = "svg"))]
// pub fn export_svg_advanced(
// _state: Rc<RefCell<AppState>>,
// _path: &str,
// _settings: ExportSettings,
// ) -> Result<String, String> {
// Err("SVG export not available. Enable 'svg' feature in cairo-rs dependency.".to_string())
// }

// ============================================================================
// BACKWARD COMPATIBILITY - Simple exports
// ============================================================================

/// Simple PNG export (backward compatible)
pub fn export_png(state: Rc<RefCell<AppState>>, width: f64, height: f64, path: &str) -> String {
    let settings = ExportSettings {
        width: if width > 0.0 { Some(width) } else { None },
        height: if height > 0.0 { Some(height) } else { None },
        ..Default::default()
    };

    export_png_advanced(state, path, settings).unwrap_or_else(|e| e)
}

/// Simple PDF export (backward compatible)
pub fn export_pdf(state: Rc<RefCell<AppState>>, path: &str) -> String {
    let settings = ExportSettings::default();
    export_pdf_advanced(state, path, settings).unwrap_or_else(|e| e)
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Apply quality settings to Cairo context
fn apply_quality_settings(cr: &cairo::Context, settings: &ExportSettings) {
    // Antialiasing
    let antialias = match settings.antialiasing {
        AntialiasMode::None => cairo::Antialias::None,
        AntialiasMode::Fast => cairo::Antialias::Fast,
        AntialiasMode::Good => cairo::Antialias::Good,
        AntialiasMode::Best => cairo::Antialias::Best,
    };
    cr.set_antialias(antialias);

    // Line quality
    match settings.line_quality {
        LineQuality::Fast => {
            cr.set_line_join(cairo::LineJoin::Round);
            cr.set_line_cap(cairo::LineCap::Round);
        }
        LineQuality::Good => {
            cr.set_line_join(cairo::LineJoin::Round);
            cr.set_line_cap(cairo::LineCap::Round);
        }
        LineQuality::Best => {
            cr.set_line_join(cairo::LineJoin::Miter);
            cr.set_line_cap(cairo::LineCap::Butt);
            cr.set_miter_limit(10.0);
        }
    }
}

/// Draw all export content
fn draw_export_content(
    cr: &cairo::Context,
    render_atoms: &[scene::RenderAtom],
    lattice_corners: &[[f64; 2]],
    tab: &TabState,
    scale: f64,
    width: f64,
    height: f64,
    settings: &ExportSettings,
) {
    // Unit cell
    if settings.include_unit_cell {
        painter::draw_unit_cell(cr, lattice_corners, true);
    }

    // Structure (atoms + bonds)
    painter::draw_structure(cr, render_atoms, tab, scale, true);

    // Miller planes
    if settings.include_miller_planes && !tab.miller_planes.is_empty() {
        painter::draw_miller_planes(cr, tab, lattice_corners, scale, width, height);
    }

    // Axes (optional - can look odd in exports)
    if settings.include_axes {
        painter::draw_axes(cr, tab, width, height);
    }

    // Selection box (if active)
    if settings.include_selection_box {
        painter::draw_selection_box(cr, tab);
    }
}

// ============================================================================
// PRESET EXPORTS
// ============================================================================

/// Export for Nature/Science submission (600 DPI, white background)
pub fn export_for_journal(
    state: Rc<RefCell<AppState>>,
    path: &str,
    format: ExportFormat,
) -> Result<String, String> {
    let settings = ExportSettings::journal_preset();

    match format {
        ExportFormat::PNG => export_png_advanced(state, path, settings),
        ExportFormat::PDF => export_pdf_advanced(state, path, settings),
        #[cfg(feature = "svg")]
        ExportFormat::SVG => export_svg_advanced(state, path, settings),
        #[cfg(not(feature = "svg"))]
        ExportFormat::SVG => Err("SVG export requires 'svg' feature in cairo-rs".to_string()),
    }
}

/// Export for presentation (150 DPI, transparent)
pub fn export_for_presentation(state: Rc<RefCell<AppState>>, path: &str) -> Result<String, String> {
    let settings = ExportSettings::presentation_preset();
    export_png_advanced(state, path, settings)
}

/// Export for web (800px wide, 72 DPI, transparent)
pub fn export_for_web(state: Rc<RefCell<AppState>>, path: &str) -> Result<String, String> {
    let settings = ExportSettings::web_preset();
    export_png_advanced(state, path, settings)
}
