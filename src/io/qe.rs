// src/io/qe.rs

use crate::model::{Atom, Structure};
use crate::utils::linalg::frac_to_cart;
use std::fs;
use std::io;
use std::io::Write;

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

        // lattice parameter (alat)  =      10.2000  a.u.
        if ascii_contains_ci(line, "lattice parameter (alat)") {
            if let Some(val) = extract_val(line, "=") {
                alat = val * BOHR_TO_ANG;
            }
        }

        if ascii_starts_with_ci(line, "cell_parameters") {
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

        if ascii_starts_with_ci(line, "atomic_positions") {
            let (unit, scale_factor) = parse_header_unit(line, alat);
            atoms.clear(); // Keep only the latest step

            i += 1;
            while i < lines.len() {
                let atom_line = lines[i].trim();
                if atom_line.is_empty()
                    || atom_line.starts_with("End")
                    || ascii_contains_ci(atom_line, "total energy")
                {
                    break;
                }

                let parts: Vec<&str> = atom_line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let el = parts[0].to_string();
                    let coords = parse_vec3(atom_line);
                    let (x, y, z) = (coords[0], coords[1], coords[2]);

                    let pos = if unit == "crystal" {
                        // Convert fractional to Cartesian using nalgebra
                        if let Some(lat) = lattice {
                            frac_to_cart([x, y, z], lat)
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
        is_periodic: true,
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
    //
    // Uses ASCII-insensitive helpers instead of allocating a lowercased
    // copy of every line. Celldm tokenization is handled with a local
    // whitespace-stripping scan rather than `lower.replace(" ", "")`.
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with('!') || trimmed.starts_with('#') {
            continue;
        }

        // Detect "celldm(1) = ..." / "celldm (1) = ..." without allocating.
        if contains_celldm_one(trimmed) {
            if let Some(val) = extract_val(line, "=") {
                alat = val * BOHR_TO_ANG;
            }
        }
        // Handle "A = ..." / " a " / "a=" / "a ="
        else if ascii_contains_ci(trimmed, " a ")
            || ascii_starts_with_ci(trimmed, "a=")
            || ascii_starts_with_ci(trimmed, "a =")
        {
            if let Some(val) = extract_val(line, "=") {
                alat = val;
            }
        }

        if ascii_contains_ci(trimmed, "ibrav") {
            if let Some(val) = extract_val(line, "=") {
                ibrav = val as i32;
            }
        }
    }

    // Pass 2: Blocks
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Explicit Lattice
        if ascii_starts_with_ci(line, "cell_parameters") {
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
        if ascii_starts_with_ci(line, "atomic_positions") {
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
                    || ascii_starts_with_ci(atom_line, "k_points")
                {
                    break;
                }

                let parts: Vec<&str> = atom_line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let el = parts[0].to_string();
                    let coords = parse_vec3(atom_line);
                    let (x, y, z) = (coords[0], coords[1], coords[2]);

                    let pos = if unit == "crystal" {
                        // Convert fractional to Cartesian using nalgebra
                        if let Some(lat) = lattice {
                            frac_to_cart([x, y, z], lat)
                        } else {
                            [x, y, z] // Fallback
                        }
                    } else if unit == "alat" {
                        [x * alat, y * alat, z * alat]
                    } else if unit == "bohr" {
                        [x * BOHR_TO_ANG, y * BOHR_TO_ANG, z * BOHR_TO_ANG]
                    } else {
                        // Angstrom
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
        is_periodic: true,
    })
}

// =======================
//   HELPERS
// =======================

fn generate_lattice_from_ibrav(ibrav: i32, a: f64) -> Option<[[f64; 3]; 3]> {
    if a <= 1e-6 {
        return None;
    }

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
            // BCC
            let h = a / 2.0;
            Some([[-h, h, h], [h, -h, h], [h, h, -h]])
        }
        4 => {
            // Hexagonal
            let c = a * 1.633; // Fallback c/a ratio
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
    let mut nums = Vec::with_capacity(3);
    for p in line.split_whitespace() {
        if nums.len() == 3 {
            break;
        }
        // Strip commas locally without allocating a String per token until
        // necessary. `clean_and_parse_first_number` already handles the
        // Fortran 'd' notation + comment splitting.
        let clean: String = p.replace(',', "");
        if let Some(val) = clean_and_parse_first_number(&clean) {
            nums.push(val);
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

    // 2. Remove commas
    let no_comma = pre_comment.replace(',', " ");

    // 3. Find first token
    let token = no_comma.split_whitespace().next()?;

    // 4. Replace Fortran 'd'/'D' with 'e' (e.g. 1.0d-8 -> 1.0e-8)
    let float_str = token.to_lowercase().replace('d', "e");

    float_str.parse::<f64>().ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// ASCII-insensitive helpers
//
// QE keywords (CELL_PARAMETERS, ATOMIC_POSITIONS, alat, ibrav, …) are pure
// ASCII. Per-line `to_lowercase()` allocates a fresh String every call,
// which dominates parse time for large relax/MD output files. These helpers
// do the comparisons in-place against an already-ASCII-lowercased needle.
// ─────────────────────────────────────────────────────────────────────────────

/// True iff `haystack.to_ascii_lowercase().starts_with(needle_lower)`
/// without allocating. `needle_lower` MUST already be lowercase.
fn ascii_starts_with_ci(haystack: &str, needle_lower: &str) -> bool {
    let nb = needle_lower.as_bytes();
    let hb = haystack.as_bytes();
    if hb.len() < nb.len() {
        return false;
    }
    for (h, n) in hb.iter().zip(nb.iter()) {
        if h.to_ascii_lowercase() != *n {
            return false;
        }
    }
    true
}

/// True iff `haystack.to_ascii_lowercase().contains(needle_lower)` without
/// allocating. `needle_lower` MUST already be lowercase. Linear scan.
fn ascii_contains_ci(haystack: &str, needle_lower: &str) -> bool {
    let nb = needle_lower.as_bytes();
    if nb.is_empty() {
        return true;
    }
    let hb = haystack.as_bytes();
    if hb.len() < nb.len() {
        return false;
    }
    for start in 0..=(hb.len() - nb.len()) {
        let mut ok = true;
        for (i, n) in nb.iter().enumerate() {
            if hb[start + i].to_ascii_lowercase() != *n {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

/// Whitespace-tolerant check for `celldm(1)=` anywhere in the line.
/// Matches `celldm(1)=`, `celldm (1) =`, `Celldm( 1 )=`, etc., without
/// allocating the "whitespace-stripped" copy the original code built.
fn contains_celldm_one(line: &str) -> bool {
    const NEEDLE: &[u8] = b"celldm(1)=";
    let hb = line.as_bytes();

    // Try every possible starting position. At each start, match NEEDLE
    // while skipping any whitespace in the haystack between matched bytes.
    for start in 0..hb.len() {
        let mut h = start;
        let mut matched = true;
        for &n in NEEDLE {
            while h < hb.len() && (hb[h] as char).is_ascii_whitespace() {
                h += 1;
            }
            if h >= hb.len() || hb[h].to_ascii_lowercase() != n {
                matched = false;
                break;
            }
            h += 1;
        }
        if matched {
            return true;
        }
    }
    false
}

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    // Basic Control Block
    writeln!(file, "&CONTROL")?;
    writeln!(file, "  calculation = 'scf'")?;
    writeln!(file, "  pseudo_dir = './'")?;
    writeln!(file, "  outdir = './out'")?;
    writeln!(file, "  prefix = 'calc'")?;
    writeln!(file, "/")?;

    // System Block
    writeln!(file, "&SYSTEM")?;
    writeln!(file, "  ibrav = 0")?;
    writeln!(file, "  nat = {}", structure.atoms.len())?;

    // Count unique types
    let mut unique_els: Vec<String> = Vec::new();
    for atom in &structure.atoms {
        if !unique_els.contains(&atom.element) {
            unique_els.push(atom.element.clone());
        }
    }
    writeln!(file, "  ntyp = {}", unique_els.len())?;
    writeln!(file, "  ecutwfc = 60.0")?;
    writeln!(file, "/")?;

    // Electrons Block
    writeln!(file, "&ELECTRONS")?;
    writeln!(file, "  conv_thr = 1.0d-8")?;
    writeln!(file, "/")?;

    // Atomic Species (Placeholder masses and pseudos)
    writeln!(file, "ATOMIC_SPECIES")?;
    for el in &unique_els {
        writeln!(file, " {:<3}  1.000  {}.UPF", el, el)?;
    }

    // Cell Parameters
    writeln!(file, "CELL_PARAMETERS (angstrom)")?;
    for vec in &structure.lattice {
        writeln!(file, "  {:15.9} {:15.9} {:15.9}", vec[0], vec[1], vec[2])?;
    }

    // Atomic Positions
    writeln!(file, "ATOMIC_POSITIONS (angstrom)")?;
    for atom in &structure.atoms {
        writeln!(
            file,
            "  {:<3}  {:15.9} {:15.9} {:15.9}",
            atom.element, atom.position[0], atom.position[1], atom.position[2]
        )?;
    }

    Ok(())
}
