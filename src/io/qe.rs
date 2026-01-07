// src/io/qe.rs
use std::fs;
use std::io::{self, BufRead};
use crate::model::{Atom, Structure};

const BOHR_TO_ANG: f64 = 0.5291772109;

pub fn parse(path: &str) -> io::Result<Structure> {
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);

    // State
    let mut lattice = [[0.0; 3]; 3];
    let mut atoms = Vec::new();

    // Global scaling factor (alat) found in the file
    let mut global_alat = 0.0;
    let mut is_output = false;

    // Read all lines into a vector so we can peek ahead if needed
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        let line_lower = line.to_lowercase();

        // --- 1. Detect Global Scaling (alat) ---
        // Output format: "lattice parameter (alat)  =      10.2000  a.u."
        if line_lower.contains("lattice parameter (alat)") {
            is_output = true;
            if let Some(val) = extract_value_after_eq(line) {
                global_alat = val;
            }
        }
        // Input format: "celldm(1) = 10.2" or "A = 10.2"
        else if line_lower.starts_with("celldm(1)") || line_lower.starts_with("a ") || line_lower.starts_with("a=") {
            if let Some(val) = extract_value_after_eq(line) {
                if global_alat == 0.0 { global_alat = val; }
            }
        }

        // --- 2. Parse Lattice Block ---
        // CASE A: "CELL_PARAMETERS (alat= 10.5)" or just "CELL_PARAMETERS (angstrom)"
        if line_lower.starts_with("cell_parameters") {
            let (unit, local_scale) = parse_header_unit(line, global_alat);

            // Read next 3 lines
            if i + 3 < lines.len() {
                let v1 = parse_vector(&lines[i+1]);
                let v2 = parse_vector(&lines[i+2]);
                let v3 = parse_vector(&lines[i+3]);

                lattice = [
                    [v1[0]*local_scale, v1[1]*local_scale, v1[2]*local_scale],
                    [v2[0]*local_scale, v2[1]*local_scale, v2[2]*local_scale],
                    [v3[0]*local_scale, v3[1]*local_scale, v3[2]*local_scale],
                ];
                i += 3;
            }
        }
        // CASE B: Standard Output Header "crystal axes: (cart. coord. in units of alat)"
        else if line_lower.contains("crystal axes:") {
            is_output = true;
            // Next 3 lines are vectors
            if i + 3 < lines.len() {
                let v1 = parse_vector(&lines[i+1]);
                let v2 = parse_vector(&lines[i+2]);
                let v3 = parse_vector(&lines[i+3]);

                // Usually these are in units of 'alat'
                let scale = if global_alat > 0.0 { global_alat * BOHR_TO_ANG } else { BOHR_TO_ANG };

                lattice = [
                    [v1[0]*scale, v1[1]*scale, v1[2]*scale],
                    [v2[0]*scale, v2[1]*scale, v2[2]*scale],
                    [v3[0]*scale, v3[1]*scale, v3[2]*scale],
                ];
                i += 3;
            }
        }

        // --- 3. Parse Atoms Block ---
        else if line_lower.starts_with("atomic_positions") {
            // "Last Win": Clear previous atoms to store the new step
            atoms.clear();

            let (unit, local_scale) = parse_header_unit(line, global_alat);

            // Advance to read atom lines
            i += 1;
            while i < lines.len() {
                let atom_line = lines[i].trim();
                if atom_line.is_empty() || atom_line.starts_with("End") || atom_line.starts_with('/') {
                    break;
                }

                let parts: Vec<&str> = atom_line.split_whitespace().collect();
                if parts.len() < 4 { break; } // Not an atom line

                // Try parsing coords
                if let (Ok(x), Ok(y), Ok(z)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>(), parts[3].parse::<f64>()) {
                     let element = parts[0].to_string();
                     let mut pos = [x, y, z];

                     if unit == "crystal" {
                        // Fractional -> Cartesian
                        pos = [
                            pos[0]*lattice[0][0] + pos[1]*lattice[1][0] + pos[2]*lattice[2][0],
                            pos[0]*lattice[0][1] + pos[1]*lattice[1][1] + pos[2]*lattice[2][1],
                            pos[0]*lattice[0][2] + pos[1]*lattice[1][2] + pos[2]*lattice[2][2],
                        ];
                     } else {
                         // Cartesian scaled (angstrom, bohr, or alat)
                         pos = [pos[0]*local_scale, pos[1]*local_scale, pos[2]*local_scale];
                     }

                     atoms.push(Atom {
                         element,
                         position: pos,
                         original_index: atoms.len(),
                     });
                }
                i += 1;
            }
            continue; // Loop already advanced `i`, skip the default increment
        }

        i += 1;
    }

    // Fallback for missing lattice (e.g. if file is truncated or weird format)
    if lattice[0][0] == 0.0 && lattice[1][1] == 0.0 {
        lattice = [[20.0, 0.0, 0.0], [0.0, 20.0, 0.0], [0.0, 0.0, 20.0]];
    }

    Ok(Structure {
        lattice,
        atoms,
        formula: if is_output { "QE Output".to_string() } else { "QE Input".to_string() },
    })
}

// --- Helpers ---

fn extract_value_after_eq(line: &str) -> Option<f64> {
    if let Some(idx) = line.find('=') {
        let val_part = &line[idx+1..];
        let clean: String = val_part.chars()
            .take_while(|c| c.is_numeric() || *c == '.' || *c == '-' || *c == 'e' || *c == 'E')
            .collect();
        return clean.parse().ok();
    }
    None
}

fn parse_vector(line: &str) -> [f64; 3] {
    // clean up parenthesis from output like "a(1) = ( 1.0 0.0 0.0 )"
    let clean = line.replace("(", " ").replace(")", " ")
                    .replace("a(1)", "").replace("a(2)", "").replace("a(3)", "").replace("=", "");

    let parts: Vec<f64> = clean.split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();

    // grab last 3 numbers
    if parts.len() >= 3 {
        let n = parts.len();
        [parts[n-3], parts[n-2], parts[n-1]]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Returns (unit_type, conversion_factor_to_angstrom)
fn parse_header_unit(line: &str, global_alat: f64) -> (String, f64) {
    let lower = line.to_lowercase();

    // Check for explicit local alat: "CELL_PARAMETERS (alat= 12.5)"
    if lower.contains("alat=") {
        if let Some(idx) = lower.find("alat=") {
            let val_str: String = lower[idx+5..].chars()
                .take_while(|c| c.is_numeric() || *c == '.')
                .collect();
            if let Ok(val) = val_str.parse::<f64>() {
                return ("alat".to_string(), val * BOHR_TO_ANG);
            }
        }
    }

    if lower.contains("angstrom") {
        ("angstrom".to_string(), 1.0)
    } else if lower.contains("bohr") {
        ("bohr".to_string(), BOHR_TO_ANG)
    } else if lower.contains("crystal") {
        ("crystal".to_string(), 1.0) // Factor irrelevant for fractional
    } else if lower.contains("alat") {
        // Use global alat if available, otherwise assume 1.0 or Bohr
        let scale = if global_alat > 0.0 { global_alat * BOHR_TO_ANG } else { BOHR_TO_ANG };
        ("alat".to_string(), scale)
    } else {
        // Default behavior if unit missing
        ("alat".to_string(), if global_alat > 0.0 { global_alat * BOHR_TO_ANG } else { 1.0 })
    }
}
