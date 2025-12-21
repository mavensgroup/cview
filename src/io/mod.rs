// src/io/mod.rs
pub mod cif;
pub mod poscar;

use std::io;
use crate::structure::Structure;

pub fn load_structure(path: &str) -> io::Result<Structure> {
    if path.to_lowercase().ends_with(".cif") {
        cif::parse(path)
    } else {
        poscar::parse(path)
    }
}
