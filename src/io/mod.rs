// src/io/mod.rs
pub mod cif;
pub mod poscar;
pub mod qe;
pub mod sprkkr;
pub mod xrd_exp;
pub mod xyz;

use crate::model::Structure;
use std::io;

pub fn load_structure(path: &str) -> io::Result<Structure> {
    let p = path.to_lowercase();

    if p.ends_with(".cif") {
        cif::parse(path)
    } else if p.ends_with(".xyz") {
        xyz::parse(path)
    } else if p.ends_with(".in")
        || p.ends_with(".pwi")
        || p.ends_with(".qe")
        || p.ends_with(".out")
        || p.ends_with(".log")
    {
        qe::parse(path)
    } else if p.ends_with(".inp") || p.ends_with(".pot") || p.ends_with(".sys") {
        sprkkr::parse(path)
    } else {
        // Fallback to POSCAR for unknown or explicit POSCAR/CONTCAR
        poscar::parse(path)
    }
}

pub fn save_structure(path: &str, structure: &Structure) -> io::Result<()> {
    let p = path.to_lowercase();

    if p.ends_with(".cif") {
        cif::write(path, structure)
    } else if p.ends_with(".xyz") {
        xyz::write(path, structure)
    } else if p.ends_with(".in") || p.ends_with(".qe") {
        qe::write(path, structure)
    } else if p.ends_with(".inp") || p.ends_with(".pot") || p.ends_with(".sys") {
        sprkkr::write(path, structure)
    } else if p.ends_with(".vasp") || p.ends_with("poscar") || p.ends_with("contcar") {
        poscar::write(path, structure)
    } else {
        // Default fallback
        poscar::write(path, structure)
    }
}
