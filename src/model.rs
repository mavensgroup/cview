//src/model/mod.rs
// pub mod bs_data;
pub mod bvs;
pub mod elements;
pub mod miller;
pub mod structure;

// Re-exports for cleaner imports
// pub use bs_data::BrillouinZoneData;
pub use elements::{ColorScheme, get_element_color};
pub use structure::{Atom, Structure};
