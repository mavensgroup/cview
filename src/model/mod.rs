//src/model/mod.rs
pub mod structure;
pub mod elements;
pub mod miller;

// Re-exports for cleaner imports
pub use structure::{Atom, Structure};
pub use elements::get_atom_properties;
