// src/io/mod.rs
pub mod cif;
pub mod poscar;
pub mod sprkkr;
pub mod xyz;
pub mod qe;
pub mod xrd_exp;

use std::io;
use crate::model::Structure;

pub fn load_structure(path: &str) -> io::Result<Structure> {
    let p = path.to_lowercase();

    if p.ends_with(".cif") {
        cif::parse(path)
    } else if p.ends_with(".xyz") {
        xyz::parse(path)
    } else if p.ends_with(".in") || p.ends_with(".pwi") || p.ends_with(".qe")
           || p.ends_with(".out") || p.ends_with(".log") { // <--- Added .out and .log here
        qe::parse(path)
    } else if p.ends_with(".inp") || p.ends_with(".pot") || p.ends_with(".sys") {
        sprkkr::parse(path)
    } else {
        // Fallback to POSCAR
        poscar::parse(path)
    }
}

// ... save_structure remains the same ...

pub fn save_structure(path: &str, structure: &Structure) -> io::Result<()> {
    let p = path.to_lowercase();

    if p.ends_with(".cif") {
        cif::write(path, structure)
    } else if p.ends_with(".inp") || p.ends_with(".pot") || p.ends_with(".sys") {
        sprkkr::write(path, structure)
    } else {
        // Default to POSCAR
        poscar::write(path, structure)
    }
}
