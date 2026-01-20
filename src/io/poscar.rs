use crate::model::structure::{Atom, Structure};
use std::fs::File;
use std::io::{self, BufRead, Write};
// use std::path::Path;

/// Calculates the inverse of a 3x3 matrix (Used by CIF parser and internally)
pub fn inverse_matrix(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

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

pub fn parse(path: &str) -> io::Result<Structure> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut lines = reader.lines();

    // 1. Comment Line
    let _comment = lines
        .next()
        .ok_or(io::Error::new(io::ErrorKind::InvalidData, "Empty file"))??;

    // 2. Scaling Factor
    let scale_line = lines.next().ok_or(io::Error::new(
        io::ErrorKind::InvalidData,
        "Missing scaling factor",
    ))??;
    let scale: f64 = scale_line
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid scaling factor"))?;

    // 3. Lattice Vectors
    let mut lattice = [[0.0; 3]; 3];
    for i in 0..3 {
        let line = lines.next().ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing lattice vector",
        ))??;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid lattice vector",
            ));
        }
        for j in 0..3 {
            let val: f64 = parts[j].parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid float in lattice")
            })?;
            lattice[i][j] = val * scale;
        }
    }

    // 4. Elements & Counts
    let line_a = lines.next().ok_or(io::Error::new(
        io::ErrorKind::InvalidData,
        "Missing atoms info",
    ))??;
    let parts_a: Vec<&str> = line_a.split_whitespace().collect();

    let mut elements = Vec::new();
    let mut counts = Vec::new();

    // Check if line A contains letters (Elements) or numbers (Counts)
    let first_char = parts_a.get(0).unwrap_or(&"").chars().next().unwrap_or(' ');

    if first_char.is_alphabetic() {
        // Line A is Symbols
        for s in parts_a {
            elements.push(s.to_string());
        }
        // Read next line for counts
        let line_b = lines.next().ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing atom counts",
        ))??;
        let parts_b: Vec<&str> = line_b.split_whitespace().collect();
        for s in parts_b {
            counts.push(
                s.parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid atom count")
                })?,
            );
        }
    } else {
        // Line A is Counts (VASP 4 style)
        for s in parts_a {
            counts.push(
                s.parse::<usize>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid atom count")
                })?,
            );
        }
        // Generate placeholder elements
        for i in 0..counts.len() {
            elements.push(format!("El{}", i + 1));
        }
    }

    // 5. Check for "Selective dynamics"
    let mut line_mode = lines.next().ok_or(io::Error::new(
        io::ErrorKind::InvalidData,
        "Unexpected end of file",
    ))??;

    if line_mode.trim().to_lowercase().starts_with("s") {
        // Skip this line and read the next one
        line_mode = lines.next().ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing mode after Selective dynamics",
        ))??;
    }

    // 6. Coordinate Mode
    let is_fractional = match line_mode.trim().to_lowercase().chars().next() {
        Some('d') => true,              // Direct
        Some('c') | Some('k') => false, // Cartesian
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown coordinate mode",
            ))
        }
    };

    // 7. Read Atoms
    let mut atoms = Vec::new();
    let mut atom_id = 0;

    for (elem_idx, &count) in counts.iter().enumerate() {
        let element = &elements[elem_idx];

        for _ in 0..count {
            let line = lines.next().ok_or(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not enough atom lines",
            ))??;
            let parts: Vec<&str> = line.split_whitespace().collect();

            // Fix: Only take first 3 parts (ignores "T T T" flags)
            if parts.len() < 3 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid atom line",
                ));
            }

            let c1: f64 = parts[0]
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid coordinate"))?;
            let c2: f64 = parts[1]
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid coordinate"))?;
            let c3: f64 = parts[2]
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid coordinate"))?;

            let mut position = [c1, c2, c3];

            if is_fractional {
                let x = c1 * lattice[0][0] + c2 * lattice[1][0] + c3 * lattice[2][0];
                let y = c1 * lattice[0][1] + c2 * lattice[1][1] + c3 * lattice[2][1];
                let z = c1 * lattice[0][2] + c2 * lattice[1][2] + c3 * lattice[2][2];
                position = [x, y, z];
            } else {
                position[0] *= scale;
                position[1] *= scale;
                position[2] *= scale;
            }

            atoms.push(Atom {
                element: element.clone(),
                position,
                original_index: atom_id,
                // Removed is_ghost field
            });
            atom_id += 1;
        }
    }

    // Generate Formula String
    let formula = elements
        .iter()
        .zip(counts.iter())
        .map(|(e, c)| format!("{}{}", e, c))
        .collect::<Vec<String>>()
        .join("");

    Ok(Structure {
        lattice, // Used 'lattice', not 'cell'
        atoms,
        formula, // Added formula
    })
}

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    // 1. Comment
    writeln!(file, "Exported by CView")?;

    // 2. Scale
    writeln!(file, "1.0")?;

    // 3. Lattice
    for vec in &structure.lattice {
        writeln!(file, "  {:15.9} {:15.9} {:15.9}", vec[0], vec[1], vec[2])?;
    }

    // 4. Species logic (VASP 5)
    let mut groups: Vec<(String, usize)> = Vec::new();
    if !structure.atoms.is_empty() {
        // Group consecutive atoms of same type?
        // VASP expects grouped atoms. We must sort or group them.
        // Simple approach: Collect unique elements in order of appearance
        let mut temp_atoms = structure.atoms.clone();
        // Stable sort by element to ensure they are grouped
        temp_atoms.sort_by(|a, b| a.element.cmp(&b.element));

        let mut current_el = temp_atoms[0].element.clone();
        let mut count = 0;

        // Count groups
        for atom in &temp_atoms {
            if atom.element == current_el {
                count += 1;
            } else {
                groups.push((current_el.clone(), count));
                current_el = atom.element.clone();
                count = 1;
            }
        }
        groups.push((current_el, count));

        // Write Element Symbols
        for (el, _) in &groups {
            write!(file, " {:>4} ", el)?;
        }
        writeln!(file)?;

        // Write Counts
        for (_, c) in &groups {
            write!(file, " {:>4} ", c)?;
        }
        writeln!(file)?;

        // 5. Mode
        writeln!(file, "Cartesian")?;

        // 6. Coordinates (Must match sorted order!)
        for atom in &temp_atoms {
            writeln!(
                file,
                "  {:15.9} {:15.9} {:15.9}",
                atom.position[0], atom.position[1], atom.position[2]
            )?;
        }
    } else {
        // Empty case
        writeln!(file, "Cartesian")?;
    }

    Ok(())
}
