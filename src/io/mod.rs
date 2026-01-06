pub mod cif;
pub mod poscar;
pub mod sprkkr;

use std::io;
use crate::structure::Structure;

pub fn load_structure(path: &str) -> io::Result<Structure> {
    let p = path.to_lowercase();
    if p.ends_with(".cif") {
        cif::parse(path)
    } else if p.ends_with(".inp") || p.ends_with(".pot") || p.ends_with(".sys") {
        sprkkr::parse(path)
    } else {
        poscar::parse(path)
    }
}

pub fn save_structure(path: &str, structure: &Structure) -> io::Result<()> {
    let p = path.to_lowercase();
    if p.ends_with(".cif") {
        cif::write(path, structure)
    } else if p.ends_with(".inp") || p.ends_with(".pot") || p.ends_with(".sys") {
        sprkkr::write(path, structure)
    } else {
        // Default to POSCAR for .vasp or unknown extensions
        poscar::write(path, structure)
    }
}
