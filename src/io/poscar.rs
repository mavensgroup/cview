// src/io/poscar.rs

use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use crate::structure::{Atom, Structure};

pub fn parse(path: &str) -> io::Result<Structure> {
    let path = Path::new(path);
    let file = File::open(path)?;
    let mut lines = io::BufReader::new(file).lines();

    let _ = lines.next(); // Comment

    // Scale
    let scale_line = lines.next().ok_or(io::Error::new(io::ErrorKind::InvalidData, "Unexpected EOF"))??;
    let scale: f64 = scale_line.trim().parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid Scale"))?;

    // Lattice
    let mut lattice = [[0.0; 3]; 3];
    for i in 0..3 {
        let line = lines.next().ok_or(io::Error::new(io::ErrorKind::InvalidData, "Missing Lattice"))??;
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.len() < 3 { return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid Lattice Line")); }
        lattice[i][0] = parts[0].parse::<f64>().unwrap() * scale;
        lattice[i][1] = parts[1].parse::<f64>().unwrap() * scale;
        lattice[i][2] = parts[2].parse::<f64>().unwrap() * scale;
    }

    // Elements & Counts
    let line6 = lines.next().ok_or(io::Error::new(io::ErrorKind::InvalidData, "Missing Elements"))??;
    let line7 = lines.next().ok_or(io::Error::new(io::ErrorKind::InvalidData, "Missing Counts"))??;

    let (elements, counts_line) = if line6.trim().chars().next().unwrap().is_alphabetic() {
        (line6, line7)
    } else {
        ("Xx".to_string(), line6)
    };

    let element_names: Vec<&str> = elements.trim().split_whitespace().collect();
    let counts: Vec<usize> = counts_line.trim().split_whitespace().map(|x| x.parse().unwrap()).collect();

    // Mode
    let mode_line = lines.next().ok_or(io::Error::new(io::ErrorKind::InvalidData, "Missing Mode"))??;
    let is_direct = mode_line.trim().to_lowercase().starts_with('d');

    // Atoms
    let mut atoms = Vec::new();
    for (elem_idx, &count) in counts.iter().enumerate() {
        let elem_name = element_names.get(elem_idx).unwrap_or(&"X").to_string();
        for _ in 0..count {
            let line = lines.next().ok_or(io::Error::new(io::ErrorKind::InvalidData, "Missing Atom Pos"))??;
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            let mut x = parts[0].parse::<f64>().unwrap();
            let mut y = parts[1].parse::<f64>().unwrap();
            let mut z = parts[2].parse::<f64>().unwrap();

            if is_direct {
                let lx = x*lattice[0][0] + y*lattice[1][0] + z*lattice[2][0];
                let ly = x*lattice[0][1] + y*lattice[1][1] + z*lattice[2][1];
                let lz = x*lattice[0][2] + y*lattice[1][2] + z*lattice[2][2];
                x = lx; y = ly; z = lz;
            }

            atoms.push(Atom { element: elem_name.clone(), position: [x, y, z] });
        }
    }

    Ok(Structure { lattice, atoms })
}
