// src/io/poscar.rs

use std::io::Write;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use crate::model::{Atom, Structure};

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

            // atoms.push(Atom { element: elem_name.clone(), position: [x, y, z] });
            let idx = atoms.len(); // <--- Capture index
            atoms.push(Atom {
                element: elem_name.clone(),
                position: [x, y, z],
                original_index: idx
            });
        }
    }

    Ok(Structure { lattice, atoms,formula: "POSCAR Import".to_string() })
}



pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    // 1. Header
    writeln!(file, "Exported by CView")?;
    writeln!(file, "1.0")?; // Universal scaling factor

    // 2. Lattice Vectors
    for vec in &structure.lattice {
        writeln!(file, " {:12.8} {:12.8} {:12.8}", vec[0], vec[1], vec[2])?;
    }

    // 3. Group Atoms by Element
    // We need to sort atoms to group them (e.g. Fe Fe Fe O O)
    // and count them for the header.
    let mut sorted_atoms = structure.atoms.clone();
    sorted_atoms.sort_by(|a, b| a.element.cmp(&b.element));

    let mut counts: Vec<(String, usize)> = Vec::new();
    if !sorted_atoms.is_empty() {
        let mut current_el = sorted_atoms[0].element.clone();
        let mut count = 0;
        for atom in &sorted_atoms {
            if atom.element == current_el {
                count += 1;
            } else {
                counts.push((current_el, count));
                current_el = atom.element.clone();
                count = 1;
            }
        }
        counts.push((current_el, count));
    }

    // Write Element Labels
    for (label, _) in &counts {
        write!(file, " {:<4}", label)?;
    }
    writeln!(file, "")?;

    // Write Counts
    for (_, count) in &counts {
        write!(file, " {:<4}", count)?;
    }
    writeln!(file, "")?;

    // 4. Atomic Positions (Direct/Fractional)
    writeln!(file, "Direct")?;

    // Calculate Inverse Lattice for Cartesian -> Fractional conversion
    let inv = inverse_matrix(structure.lattice);

    for atom in &sorted_atoms {
        let p = atom.position;
        // frac = p * inv
        let u = p[0]*inv[0][0] + p[1]*inv[1][0] + p[2]*inv[2][0];
        let v = p[0]*inv[0][1] + p[1]*inv[1][1] + p[2]*inv[2][1];
        let w = p[0]*inv[0][2] + p[1]*inv[1][2] + p[2]*inv[2][2];

        // Wrap to [0, 1) usually, but raw is fine too
        writeln!(file, " {:12.8} {:12.8} {:12.8}", u, v, w)?;
    }

    Ok(())
}

// Simple 3x3 Matrix Inversion helper
pub fn inverse_matrix(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) -
              m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) +
              m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-8 { return [[0.0; 3]; 3]; } // Degenerate
    let inv_det = 1.0 / det;

    [
        [
            (m[1][1] * m[2][2] - m[2][1] * m[1][2]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[1][0] * m[0][2] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[2][0] * m[1][1]) * inv_det,
            (m[2][0] * m[0][1] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[1][0] * m[0][1]) * inv_det,
        ],
    ]
}
