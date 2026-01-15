use crate::model::{Atom, Structure};
use std::fs;
use std::io;

const BOHR_TO_ANG: f64 = 0.5291772109;

pub fn parse(path: &str) -> io::Result<Structure> {
    let content = fs::read_to_string(path)?;

    // Heuristic: Output files contain execution markers
    if content.contains("Program PWSCF")
        || content.contains("JOB DONE")
        || content.contains("unit-cell volume")
    {
        parse_output(&content)
    } else {
        parse_input(&content)
    }
}

// =======================
//   QE OUTPUT PARSER
// =======================
fn parse_output(content: &str) -> io::Result<Structure> {
    let mut lattice = None;
    let mut atoms = Vec::new();
    let mut alat = 0.0;

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        let lower = line.to_lowercase();

        // lattice parameter (alat)  =      10.2000  a.u.
        if lower.contains("lattice parameter (alat)") {
            if let Some(val) = extract_val(line, "=") {
                alat = val * BOHR_TO_ANG;
            }
        }

        if lower.starts_with("cell_parameters") {
            let (unit, scale) = parse_header_unit(line, alat);

            if i + 3 < lines.len() {
                let v1 = parse_vec3(lines[i + 1]);
                let v2 = parse_vec3(lines[i + 2]);
                let v3 = parse_vec3(lines[i + 3]);

                let factor = if unit == "alat" {
                    scale
                } else if unit == "bohr" {
                    BOHR_TO_ANG
                } else {
                    1.0
                };

                lattice = Some([
                    [v1[0] * factor, v1[1] * factor, v1[2] * factor],
                    [v2[0] * factor, v2[1] * factor, v2[2] * factor],
                    [v3[0] * factor, v3[1] * factor, v3[2] * factor],
                ]);
            }
        }

        if lower.starts_with("atomic_positions") {
            let (unit, scale_factor) = parse_header_unit(line, alat);
            atoms.clear(); // Keep only the latest step

            i += 1;
            while i < lines.len() {
                let atom_line = lines[i].trim();
                if atom_line.is_empty()
                    || atom_line.starts_with("End")
                    || atom_line.contains("total energy")
                {
                    break;
                }

                let parts: Vec<&str> = atom_line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let el = parts[0].to_string();
                    let coords = parse_vec3(atom_line); // Use robust parsing for coords too
                    let (x, y, z) = (coords[0], coords[1], coords[2]);

                    let pos = if unit == "crystal" {
                        if let Some(lat) = lattice {
                            let lx = lat[0];
                            let ly = lat[1];
                            let lz = lat[2];
                            [
                                x * lx[0] + y * ly[0] + z * lz[0],
                                x * lx[1] + y * ly[1] + z * lz[1],
                                x * lx[2] + y * ly[2] + z * lz[2],
                            ]
                        } else {
                            [0.0, 0.0, 0.0]
                        }
                    } else if unit == "alat" {
                        [x * scale_factor, y * scale_factor, z * scale_factor]
                    } else if unit == "bohr" {
                        [x * BOHR_TO_ANG, y * BOHR_TO_ANG, z * BOHR_TO_ANG]
                    } else {
                        [x, y, z] // Angstrom
                    };

                    atoms.push(Atom {
                        element: el,
                        position: pos,
                        original_index: atoms.len(),
                    });
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }

    let final_lattice = lattice.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "No CELL_PARAMETERS found in output",
        )
    })?;

    Ok(Structure {
        lattice: final_lattice,
        formula: generate_formula(&atoms),
        atoms,
    })
}

// =======================
//   QE INPUT PARSER
// =======================
fn parse_input(content: &str) -> io::Result<Structure> {
    let mut alat = 0.0;
    let mut ibrav = 0;
    let mut lattice = None;
    let mut atoms = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Pass 1: Global params
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('!') || trimmed.starts_with('#') {
            continue;
        } // Skip comments

        let lower = trimmed.to_lowercase();

        // Handle "celldm(1) = ..." and "celldm (1) = ..."
        // We remove spaces to make matching "celldm(1)" robust against "celldm (1)"
        let clean_line = lower.replace(" ", "");

        if clean_line.contains("celldm(1)=") {
            if let Some(val) = extract_val(line, "=") {
                alat = val * BOHR_TO_ANG;
            }
        }
        // Handle "A = ..."
        else if lower.contains(" a ") || lower.starts_with("a=") || lower.starts_with("a =") {
            if let Some(val) = extract_val(line, "=") {
                alat = val;
            }
        }

        if lower.contains("ibrav") {
            if let Some(val) = extract_val(line, "=") {
                ibrav = val as i32;
            }
        }
    }

    // Pass 2: Blocks
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let lower = line.to_lowercase();

        // Explicit Lattice
        if lower.starts_with("cell_parameters") {
            let (unit, _) = parse_header_unit(line, alat);
            if i + 3 < lines.len() {
                let v1 = parse_vec3(lines[i + 1]);
                let v2 = parse_vec3(lines[i + 2]);
                let v3 = parse_vec3(lines[i + 3]);

                let factor = if unit == "bohr" {
                    BOHR_TO_ANG
                } else if unit == "alat" {
                    alat
                } else {
                    1.0
                };

                lattice = Some([
                    [v1[0] * factor, v1[1] * factor, v1[2] * factor],
                    [v2[0] * factor, v2[1] * factor, v2[2] * factor],
                    [v3[0] * factor, v3[1] * factor, v3[2] * factor],
                ]);
            }
        }

        // Atoms
        if lower.starts_with("atomic_positions") {
            // Determine default unit based on ibrav presence
            let default_unit = if ibrav != 0 { "alat" } else { "angstrom" };
            let (unit, _) = parse_header_unit_with_default(line, alat, default_unit);

            i += 1;
            while i < lines.len() {
                let atom_line = lines[i].trim();
                // Block ends with / or new namelist or K_POINTS
                if atom_line.is_empty()
                    || atom_line.starts_with('/')
                    || atom_line.starts_with('&')
                    || atom_line.starts_with("K_POINTS")
                    || atom_line.to_lowercase().starts_with("k_points")
                {
                    break;
                }

                let parts: Vec<&str> = atom_line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let el = parts[0].to_string();
                    let coords = parse_vec3(atom_line); // Use robust vec3 parsing (handles 1.0d-8 and commas)
                    let (x, y, z) = (coords[0], coords[1], coords[2]);

                    let pos = if unit == "alat" {
                        [x * alat, y * alat, z * alat]
                    } else if unit == "bohr" {
                        [x * BOHR_TO_ANG, y * BOHR_TO_ANG, z * BOHR_TO_ANG]
                    } else {
                        // Angstrom or Crystal (if raw)
                        [x, y, z]
                    };

                    atoms.push(Atom {
                        element: el,
                        position: pos,
                        original_index: atoms.len(),
                    });
                }
                i += 1;
            }
        }
        i += 1;
    }

    // Determine Final Lattice
    let final_lattice = if let Some(l) = lattice {
        l
    } else {
        generate_lattice_from_ibrav(ibrav, alat).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Unsupported ibrav {} or missing CELL_PARAMETERS (alat={})",
                    ibrav, alat
                ),
            )
        })?
    };

    Ok(Structure {
        lattice: final_lattice,
        formula: generate_formula(&atoms),
        atoms,
    })
}

// =======================
//   HELPERS
// =======================

fn generate_lattice_from_ibrav(ibrav: i32, a: f64) -> Option<[[f64; 3]; 3]> {
    if a <= 1e-6 {
        return None;
    }

    // Standard QE Vectors (Symmetric)

    // [Image of primitive vectors for BCC lattice]

    match ibrav {
        1 => {
            // Simple Cubic
            Some([[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]])
        }
        2 => {
            // FCC
            let h = a / 2.0;
            Some([[-h, 0.0, h], [0.0, h, h], [-h, h, 0.0]])
        }
        3 => {
            // BCC - Standard Symmetric
            let h = a / 2.0;
            Some([[-h, h, h], [h, -h, h], [h, h, -h]])
        }
        4 => {
            // Hexagonal
            let c = a * 1.633; // Fallback c/a
            Some([[a, 0.0, 0.0], [-0.5 * a, 0.866 * a, 0.0], [0.0, 0.0, c]])
        }
        _ => None,
    }
}

fn generate_formula(atoms: &[Atom]) -> String {
    use std::collections::HashMap;
    let mut counts = HashMap::new();
    for a in atoms {
        *counts.entry(a.element.clone()).or_insert(0) += 1;
    }
    let mut parts: Vec<_> = counts.into_iter().collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    parts
        .iter()
        .map(|(el, c)| {
            if *c > 1 {
                format!("{}{}", el, c)
            } else {
                el.clone()
            }
        })
        .collect()
}

fn parse_header_unit(header: &str, global_alat: f64) -> (String, f64) {
    parse_header_unit_with_default(header, global_alat, "angstrom")
}

fn parse_header_unit_with_default(header: &str, global_alat: f64, default: &str) -> (String, f64) {
    let lower = header.to_lowercase();

    if lower.contains("alat=") || lower.contains("alat =") {
        if let Some(val) = extract_val(&lower, "=") {
            return ("alat".to_string(), val * BOHR_TO_ANG);
        }
    }

    if lower.contains("angstrom") {
        ("angstrom".to_string(), 1.0)
    } else if lower.contains("bohr") {
        ("bohr".to_string(), BOHR_TO_ANG)
    } else if lower.contains("crystal") {
        ("crystal".to_string(), 1.0)
    } else if lower.contains("alat") {
        ("alat".to_string(), global_alat)
    } else {
        (
            default.to_string(),
            if default == "alat" { global_alat } else { 1.0 },
        )
    }
}

/// Robust extraction: Handles comments, commas, and Fortran 'd' notation
fn extract_val(line: &str, delimiter: &str) -> Option<f64> {
    let part = line.split(delimiter).nth(1)?;
    clean_and_parse_first_number(part)
}

/// Robust Vec3 parsing: Handles whitespace, commas, 'd' notation
fn parse_vec3(line: &str) -> [f64; 3] {
    let parts: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();

    // We need to find the first 3 numeric-looking strings
    let mut nums = Vec::new();
    for p in parts {
        if nums.len() == 3 {
            break;
        }
        // Skip Element label (alphabetic) but allow scientific E/e/d/D
        // Logic: if it contains alphabetic chars that are NOT 'e'/'d', it's likely a label
        let clean_p = p.to_lowercase().replace(',', "");

        let is_numeric = clean_and_parse_first_number(&clean_p).is_some();
        if is_numeric {
            if let Some(val) = clean_and_parse_first_number(&clean_p) {
                nums.push(val);
            }
        }
    }

    if nums.len() >= 3 {
        [nums[0], nums[1], nums[2]]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Cleans a string chunk (e.g. "1.0d-8,") and parses it
fn clean_and_parse_first_number(raw: &str) -> Option<f64> {
    // 1. Remove comments
    let pre_comment = raw.split('!').next()?.split('#').next()?;

    // 2. Remove commas (QE often uses commas as delimiters)
    let no_comma = pre_comment.replace(',', " ");

    // 3. Find first token
    let token = no_comma.trim().split_whitespace().next()?;

    // 4. Replace Fortran 'd'/'D' with 'e' (e.g. 1.0d-8 -> 1.0e-8)
    let float_str = token.to_lowercase().replace('d', "e");

    float_str.parse::<f64>().ok()
}
