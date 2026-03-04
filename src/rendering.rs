pub mod export;
pub mod painter;
pub mod polyhedra;
pub mod polyhedra_lighting;
pub mod primitives;
pub mod scene;
pub mod sprite_cache;

// Re-export specific functions to keep the API clean for the rest of the app
pub use export::setup_drawing;
pub use export::{export_pdf, export_png};

// NOTE: rendering_charge_density.rs has been removed.
// All charge density rendering is now consolidated in
// ui::analysis::charge_density_tab (draw_scene, draw_colorbar, etc.)
