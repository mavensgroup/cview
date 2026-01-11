pub mod scene;
pub mod painter;
pub mod export;
pub mod primitives;

// Re-export specific functions to keep the API clean for the rest of the app
pub use export::setup_drawing;
pub use export::export_image;
