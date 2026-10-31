//src/model/mod.rs
pub mod bonds;
pub mod bvs;
pub mod elements;
pub mod miller;
pub mod structure;
pub mod trajectory;
pub mod vesta_bonds;
pub mod wyckoff_table;

// Re-exports for cleaner imports
pub use elements::{ColorScheme, get_element_color};
pub use structure::{Atom, Structure};
