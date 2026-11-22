// src/io/qe.rs

use crate::model::trajectory::{Frame, Trajectory};
use crate::model::{Atom, Structure};
use crate::utils::linalg::frac_to_cart;
use std::fs;
use std::io;
use std::io::Write;

const BOHR_TO_ANG: f64 = 0.5291772109;

pub fn parse(path: &str) -> io::Result<Structure> {
    let content = fs::read_to_string(path)?;

    // Heuristic: Output files contain execution markers
    if is_output(&content) {
        parse_output(&content)
    } else {
        parse_input(&content)
    }
}

// =======================
//   QE OUTPUT PARSER
// =======================

const RY_TO_EV: f64 = 13.605693122994;
/// Ry/bohr to eV/Å.
const RY_BOHR_TO_EV_ANG: f64 = RY_TO_EV / BOHR_TO_ANG;

/// True if `content` is pw.x output rather than input.
fn is_output(content: &str) -> bool {
    content.contains("Program PWSCF") || content.contains("JOB DONE") || content.contains("unit-cell volume")
}

/// All geometries of a pw.x output, or `None` if `path` is an input file.
pub fn parse_trajectory(path: &str) -> io::Result<Option<Trajectory>> {
    let content = fs::read_to_string(path)?;
    if !is_output(&content) {
        return Ok(None);
    }
    parse_output_frames(&content).map(Some)
}

/// The three numbers inside `= ( ... )`, as in `a(1) = ( 1.0 0.0 0.0 )`.
fn paren_vec3(line: &str) -> Option<[f64; 3]> {
    let inner = line.rsplit_once("= (").or_else(|| line.rsplit_once("=("))?.1;
    let inner = inner.split(')').next()?;
    let v: Vec<f64> = inner.split_whitespace().filter_map(|t| t.replace(['d', 'D'], "e").parse().ok()).collect();
    (v.len() >= 3).then(|| [v[0], v[1], v[2]])
}

/// Every geometry of a pw.x run, in order: the input structure from the
/// header, then one frame per `ATOMIC_POSITIONS` block of a relax, vc-relax
/// or MD run. Energies and forces belong to the geometry they were computed
/// for, which is the latest frame when they are printed. The block repeated
/// between "Begin/End final coordinates" is not a new geometry and is skipped.
fn parse_output_frames(content: &str) -> io::Result<Trajectory> {
    let lines: Vec<&str> = content.lines().collect();
    let mut alat = 0.0;
    let mut lattice: Option<[[f64; 3]; 3]> = None;
    let mut species: Vec<String> = Vec::new();
    let mut traj = Trajectory {
        species: vec![],
        frames: vec![],
        is_periodic: true,
        format: "QE output",
    };
    let mut in_final = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if ascii_contains_ci(line, "lattice parameter (alat)") {
            if let Some(val) = extract_val(line, "=") {
                alat = val * BOHR_TO_ANG;
            }
        } else if line.starts_with("crystal axes:") && lattice.is_none() {
            // a(1..3) in units of alat: the starting cell.
            let rows: Vec<[f64; 3]> = (1..=3).filter_map(|k| lines.get(i + k).and_then(|l| paren_vec3(l))).collect();
            if rows.len() == 3 {
                lattice = Some([0, 1, 2].map(|r| rows[r].map(|x| x * alat)));
            }
        } else if line.starts_with("site n.") && traj.frames.is_empty() {
            // Starting positions:   1   C   tau(   1) = ( x y z )   (alat units)
            let mut positions = Vec::new();
            let mut k = i + 1;
            while let Some(l) = lines.get(k) {
                let Some(v) = paren_vec3(l).filter(|_| l.contains("tau(")) else { break };
                let el = element_of_label(l.split_whitespace().nth(1).unwrap_or("X"));
                species.push(el);
                positions.push(v.map(|x| x * alat));
                k += 1;
            }
            if let (Some(lat), false) = (lattice, positions.is_empty()) {
                traj.species = species.clone();
                traj.frames.push(Frame { lattice: lat, positions: positions.into(), energy: None, max_force: None, step: Some(0), time: None, magmoms: None });
            }
            i = k;
            continue;
        } else if line.starts_with("Begin final coordinates") {
            in_final = true;
        } else if line.starts_with("End final coordinates") {
            in_final = false;
        } else if ascii_starts_with_ci(line, "cell_parameters") {
            let (unit, scale) = parse_header_unit(line, alat);
            if i + 3 < lines.len() {
                let factor = match unit.as_str() {
                    "alat" => scale,
                    "bohr" => BOHR_TO_ANG,
                    _ => 1.0,
                };
                lattice = Some([1, 2, 3].map(|k| parse_vec3(lines[i + k]).map(|x| x * factor)));
            }
        } else if line.starts_with("Magnetic moment per site") {
            // Printed every SCF iteration; the last block before the next
            // geometry is the converged one, so later blocks overwrite.
            let (m, next) = super::magmom::qe_site_moments(&lines, i);
            if let Some(f) = traj.frames.last_mut() {
                if !m.is_empty() && m.len() == f.positions.len() {
                    f.magmoms = Some(m);
                }
            }
            i = next;
            continue;
        } else if line.starts_with('!') && ascii_contains_ci(line, "total energy") {
            if let (Some(e), Some(f)) = (extract_val(line, "="), traj.frames.last_mut()) {
                f.energy = Some(e * RY_TO_EV);
            }
        } else if line.starts_with("Forces acting on atoms") {
            let mut max2: f64 = 0.0;
            let mut k = i + 1;
            while let Some(l) = lines.get(k) {
                let t = l.trim();
                if t.starts_with("Total force") || t.starts_with("The non-local") {
                    break;
                }
                if t.starts_with("atom") {
                    if let Some(rest) = t.split("force =").nth(1) {
                        let v = parse_vec3(rest);
                        max2 = max2.max(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
                    }
                }
                k += 1;
            }
            if let Some(f) = traj.frames.last_mut() {
                f.max_force = Some(max2.sqrt() * RY_BOHR_TO_EV_ANG);
            }
            i = k;
            continue;
        } else if ascii_starts_with_ci(line, "atomic_positions") {
            let (unit, scale) = parse_header_unit(line, alat);
            let lat = lattice.ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "ATOMIC_POSITIONS before any cell in QE output")
            })?;
            let mut els = Vec::new();
            let mut positions = Vec::new();
            let mut k = i + 1;
            while let Some(l) = lines.get(k) {
                let t = l.trim();
                let parts: Vec<&str> = t.split_whitespace().collect();
                if t.is_empty() || t.starts_with("End") || parts.len() < 4 || parts[1].parse::<f64>().is_err() {
                    break;
                }
                let c = parse_vec3(&parts[1..].join(" "));
                positions.push(match unit.as_str() {
                    "crystal" => frac_to_cart(c, lat),
                    "alat" => c.map(|x| x * scale),
                    "bohr" => c.map(|x| x * BOHR_TO_ANG),
                    _ => c,
                });
                els.push(element_of_label(parts[0]));
                k += 1;
            }
            i = k;
            if traj.frames.is_empty() {
                // No header geometry found: this block starts the path.
                traj.species = els.clone();
            }
            let frame = Frame { lattice: lat, positions: positions.into(), energy: None, max_force: None, step: Some(traj.frames.len() as i64), time: None, magmoms: None };
            if in_final {
                if let Some(last) = traj.frames.last() {
                    let same = last.positions.len() == frame.positions.len()
                        && last.positions.iter().zip(frame.positions.iter()).all(|(a, b)| (0..3).all(|d| (a[d] - b[d]).abs() < 1e-6));
                    if same {
                        continue;
                    }
                }
            }
            if !traj.push_checked(frame) {
                crate::utils::console::log_warn("QE output: atom count changes between steps; later steps were not read");
                break;
            }
            continue;
        }
        i += 1;
    }

    if traj.frames.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "No atomic positions found in QE output"));
    }
    Ok(traj)
}

/// The last geometry of a pw.x output (the relaxed structure).
fn parse_output(content: &str) -> io::Result<Structure> {
    let t = parse_output_frames(content)?;
    let mut s = t.structure_at(t.len() - 1).expect("non-empty");
    s.formula = generate_formula(&s.atoms);
    Ok(s)
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
                    let el = element_of_label(parts[0]);
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
                        oxidation: None,
                        occupancy: 1.0,
                        magmom: None,
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

/// One pw.x species: a label and, when the structure has moments, the
/// moment its atoms share.
struct Species {
    label: String,
    element: String,
    moment: [f64; 3],
}

/// pw.x's limit on the number of species (`ntypx` in Modules/parameters.f90).
pub const MAX_SPECIES: usize = 10;

/// Species for `structure`: one per element, split by moment when an
/// element's atoms carry different moments (Fe1 up, Fe2 down), since pw.x
/// sets the starting magnetization per species. A texture with more
/// distinct moments than pw.x allows species (a skyrmion, a spiral) is
/// grouped: each magnetic element gets a share of the free species, its
/// moments are clustered into that many groups, and each group starts from
/// its mean moment. Returns the species, the species index of each atom,
/// and whether moments were grouped.
fn species_of(structure: &Structure) -> (Vec<Species>, Vec<usize>, bool) {
    let mut elements: Vec<String> = Vec::new();
    for a in &structure.atoms {
        if !elements.contains(&a.element) {
            elements.push(a.element.clone());
        }
    }
    let moment = |i: usize| structure.atoms[i].magmom.unwrap_or([0.0; 3]);
    let key = |m: [f64; 3]| m.map(|x| (x * 1000.0).round() as i64);

    // Distinct moments per element, in atom order.
    let distinct: Vec<Vec<[i64; 3]>> = elements
        .iter()
        .map(|el| {
            let mut d: Vec<[i64; 3]> = Vec::new();
            for (i, a) in structure.atoms.iter().enumerate() {
                if &a.element == el && !d.contains(&key(moment(i))) {
                    d.push(key(moment(i)));
                }
            }
            d
        })
        .collect();
    let exact: usize = distinct.iter().map(Vec::len).sum();

    // Groups per element: exact when it fits, else the free slots go to
    // the elements with the most distinct moments, one at a time.
    let mut groups: Vec<usize> = distinct.iter().map(Vec::len).collect();
    let grouped = exact > MAX_SPECIES && elements.len() <= MAX_SPECIES;
    if grouped {
        groups = vec![1; elements.len()];
        for _ in elements.len()..MAX_SPECIES {
            let best = (0..elements.len())
                .filter(|&e| groups[e] < distinct[e].len())
                .max_by_key(|&e| (distinct[e].len() * 1000) / groups[e]);
            match best {
                Some(e) => groups[e] += 1,
                None => break,
            }
        }
    }

    // Cluster each element's atoms into its groups: farthest-point centres
    // over the moment vectors, then every atom to its nearest centre.
    let dist2 = |a: [f64; 3], b: [f64; 3]| (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>();
    let mut out: Vec<Species> = Vec::new();
    let mut of_atom = vec![0usize; structure.atoms.len()];
    for (e, el) in elements.iter().enumerate() {
        let atoms: Vec<usize> = (0..structure.atoms.len()).filter(|&i| &structure.atoms[i].element == el).collect();
        let mut centres: Vec<[f64; 3]> = Vec::new();
        if groups[e] == distinct[e].len() {
            // Exact: one centre per distinct moment, in order of appearance.
            for &i in &atoms {
                if !centres.iter().any(|c| key(*c) == key(moment(i))) {
                    centres.push(moment(i));
                }
            }
        } else {
            centres.push(moment(atoms[0]));
            while centres.len() < groups[e] {
                let far = atoms
                    .iter()
                    .map(|&i| (i, centres.iter().map(|c| dist2(*c, moment(i))).fold(f64::MAX, f64::min)))
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                match far {
                    Some((i, d)) if d > 1e-12 => centres.push(moment(i)),
                    _ => break,
                }
            }
        }
        let first = out.len();
        let mut sums = vec![([0.0; 3], 0usize); centres.len()];
        for &i in &atoms {
            let c = (0..centres.len())
                .min_by(|&a, &b| dist2(centres[a], moment(i)).total_cmp(&dist2(centres[b], moment(i))))
                .unwrap_or(0);
            of_atom[i] = first + c;
            for (acc, x) in sums[c].0.iter_mut().zip(moment(i)) {
                *acc += x;
            }
            sums[c].1 += 1;
        }
        let split = centres.len() > 1;
        for (c, (sum, n)) in sums.iter().enumerate() {
            out.push(Species {
                label: if split { format!("{}{}", el, c + 1) } else { el.clone() },
                element: el.clone(),
                moment: if grouped { sum.map(|x| x / (*n).max(1) as f64) } else { centres[c] },
            });
        }
    }
    (out, of_atom, grouped)
}

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    if structure.atoms.iter().any(|a| a.occupancy < 0.99) {
        crate::utils::console::log_warn(
            "QE input format has no occupancy field — partial occupancies are discarded on export",
        );
    }
    let (species, of_atom, grouped) = species_of(structure);
    if species.len() > MAX_SPECIES {
        crate::utils::console::log_warn(&format!(
            "QE input: {} species, but pw.x allows at most {} unless rebuilt with a larger ntypx",
            species.len(),
            MAX_SPECIES
        ));
    }
    if grouped {
        crate::utils::console::log_warn(&format!(
            "QE input: more distinct moments than pw.x allows species ({}); grouped into {} species, \
             each starting from its group's mean moment",
            MAX_SPECIES,
            species.len()
        ));
    }
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
    writeln!(file, "  ntyp = {}", species.len())?;
    writeln!(file, "  ecutwfc = 60.0")?;
    if grouped {
        writeln!(
            file,
            "  ! Moments grouped into {} species (pw.x allows {}): each atom starts from its\n  \
             ! group's mean moment. Run with a magnetic constraint or let the SCF relax it.",
            species.len(),
            MAX_SPECIES
        )?;
    }
    write_magnetization(&mut file, structure, &species)?;
    writeln!(file, "/")?;

    // Electrons Block
    writeln!(file, "&ELECTRONS")?;
    writeln!(file, "  conv_thr = 1.0d-8")?;
    writeln!(file, "/")?;

    // Atomic Species (placeholder masses and pseudos, as before)
    writeln!(file, "ATOMIC_SPECIES")?;
    for sp in &species {
        writeln!(file, " {:<4}  1.000  {}.UPF", sp.label, sp.element)?;
    }

    // Cell Parameters
    writeln!(file, "CELL_PARAMETERS (angstrom)")?;
    for vec in &structure.lattice {
        writeln!(file, "  {:15.9} {:15.9} {:15.9}", vec[0], vec[1], vec[2])?;
    }

    // Atomic Positions
    writeln!(file, "ATOMIC_POSITIONS (angstrom)")?;
    for (atom, &sp) in structure.atoms.iter().zip(&of_atom) {
        writeln!(
            file,
            "  {:<4}  {:15.9} {:15.9} {:15.9}",
            species[sp].label, atom.position[0], atom.position[1], atom.position[2]
        )?;
    }

    Ok(())
}

/// The &SYSTEM magnetization: nspin = 2 and a signed starting_magnetization
/// per species for collinear moments, noncolin with angle1/angle2 otherwise.
/// starting_magnetization is a fraction (−1..1) of the species' valence
/// charge, not μB, so the moments are scaled: the largest becomes 0.5.
fn write_magnetization(file: &mut std::fs::File, structure: &Structure, species: &[Species]) -> io::Result<()> {
    let Some(axis) = structure.magnetic_axis() else { return Ok(()) };
    let norm = |m: [f64; 3]| (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    let max = species.iter().map(|s| norm(s.moment)).fold(0.0, f64::max);
    if max < 1e-6 {
        return Ok(());
    }
    let collinear = structure.moments_collinear();
    writeln!(
        file,
        "  ! starting_magnetization: fraction of valence charge, not muB; CView scales the\n  \
         ! largest moment ({:.2} muB) to 0.5 and keeps the others in proportion. Adjust.",
        max
    )?;
    if collinear {
        writeln!(file, "  nspin = 2")?;
        if axis[2] < 1.0 - 1e-6 {
            writeln!(
                file,
                "  ! moments collinear along ({:.4}, {:.4}, {:.4}); signs are along that axis",
                axis[0], axis[1], axis[2]
            )?;
        }
    } else {
        writeln!(file, "  noncolin = .true.")?;
    }
    for (i, sp) in species.iter().enumerate() {
        let m = sp.moment;
        let n = norm(m);
        if n < 1e-6 {
            continue;
        }
        if collinear {
            let along = m[0] * axis[0] + m[1] * axis[1] + m[2] * axis[2];
            writeln!(file, "  starting_magnetization({}) = {:.3}", i + 1, 0.5 * along / max)?;
        } else {
            // angle1: from z; angle2: azimuth in the xy plane, degrees.
            let theta = (m[2] / n).clamp(-1.0, 1.0).acos().to_degrees();
            let phi = m[1].atan2(m[0]).to_degrees();
            writeln!(file, "  starting_magnetization({}) = {:.3}", i + 1, 0.5 * n / max)?;
            writeln!(file, "  angle1({}) = {:.2}", i + 1, theta)?;
            writeln!(file, "  angle2({}) = {:.2}", i + 1, phi)?;
        }
    }
    Ok(())
}

/// The element a pw.x species label stands for: "Fe1", "Fe_up", "Fe2" → Fe.
/// Labels that start with no element symbol are kept as they are.
fn element_of_label(label: &str) -> String {
    use crate::physics::operations::spacegroup_build::normalize_element;
    let letters: String = label.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    normalize_element(&letters)
        .or_else(|| letters.get(..1).and_then(normalize_element))
        .unwrap_or_else(|| label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TmpFile(std::path::PathBuf);
    impl TmpFile {
        fn new(contents: &str) -> Self {
            let mut p = std::env::temp_dir();
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            p.push(format!("cview_qe_{}_{}.in", std::process::id(), n));
            std::fs::write(&p, contents).unwrap();
            TmpFile(p)
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }
    impl Drop for TmpFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn input_explicit_cell_angstrom() {
        let f = TmpFile::new(
            "&system\n  ibrav = 0\n  nat = 2\n/\n\
             CELL_PARAMETERS angstrom\n\
             4.0 0.0 0.0\n0.0 4.0 0.0\n0.0 0.0 4.0\n\
             ATOMIC_POSITIONS angstrom\n\
             Si 0.0 0.0 0.0\nSi 2.0 2.0 2.0\n",
        );
        let s = parse(f.path()).unwrap();
        assert!(s.is_periodic);
        assert_eq!(s.atoms.len(), 2);
        approx(s.lattice[1][1], 4.0);
        approx(s.atoms[1].position[0], 2.0);
    }

    #[test]
    fn input_crystal_coords_convert_to_cartesian() {
        // Fractional (crystal) positions must be converted using the cell.
        let f = TmpFile::new(
            "&system\n  ibrav = 0\n/\n\
             CELL_PARAMETERS angstrom\n\
             4.0 0.0 0.0\n0.0 4.0 0.0\n0.0 0.0 4.0\n\
             ATOMIC_POSITIONS crystal\n\
             Na 0.5 0.5 0.5\n",
        );
        let s = parse(f.path()).unwrap();
        // frac (0.5,0.5,0.5) in a 4 Å cube → cart (2,2,2).
        approx(s.atoms[0].position[0], 2.0);
        approx(s.atoms[0].position[1], 2.0);
        approx(s.atoms[0].position[2], 2.0);
    }

    #[test]
    fn input_ibrav1_generates_cubic_lattice() {
        // No CELL_PARAMETERS: lattice derived from ibrav=1 + celldm(1) (bohr).
        let f = TmpFile::new(
            "&system\n  ibrav = 1\n  celldm(1) = 10.0\n/\n\
             ATOMIC_POSITIONS alat\n\
             H 0.0 0.0 0.0\n",
        );
        let s = parse(f.path()).unwrap();
        // celldm(1)=10 bohr → a = 10 * 0.5291772109 Å.
        let a = 10.0 * BOHR_TO_ANG;
        approx(s.lattice[0][0], a);
        approx(s.lattice[1][1], a);
        approx(s.lattice[2][2], a);
    }

    #[test]
    fn write_then_parse_roundtrips() {
        let original = Structure {
            lattice: [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]],
            atoms: vec![Atom {
                element: "C".into(),
                position: [1.0, 2.0, 3.0],
                original_index: 0,
                oxidation: None,
                occupancy: 1.0,
                magmom: None,
            }],
            formula: String::new(),
            is_periodic: true,
        };
        let f = TmpFile::new("");
        write(f.path(), &original).unwrap();
        let s = parse(f.path()).unwrap();
        assert_eq!(s.atoms.len(), 1);
        assert_eq!(s.atoms[0].element, "C");
        approx(s.atoms[0].position[1], 2.0);
        approx(s.lattice[2][2], 5.0);
    }
}

#[cfg(test)]
mod trajectory_tests {
    use super::*;

    const RELAX: &str = "     Program PWSCF v.7.0 starts on ...
     lattice parameter (alat)  =      10.0000  a.u.
     crystal axes: (cart. coord. in units of alat)
               a(1) = (   1.000000   0.000000   0.000000 )
               a(2) = (   0.000000   1.000000   0.000000 )
               a(3) = (   0.000000   0.000000   1.000000 )

     site n.     atom                  positions (alat units)
         1           C   tau(   1) = (   0.1000000   0.0000000   0.0000000  )
         2           O   tau(   2) = (   0.0000000   0.0000000   0.0000000  )

!    total energy              =     -43.0 Ry
     Forces acting on atoms (cartesian axes, Ry/au):

     atom    1 type  2   force =    -0.10000000    0.00000000    0.00000000
     atom    2 type  1   force =     0.10000000    0.00000000    0.00000000

     Total force =     0.141421     Total SCF correction =     0.000092
ATOMIC_POSITIONS (bohr)
C        2.000000000   0.000000000   0.000000000
O        0.000000000   0.000000000   0.000000000

!    total energy              =     -43.1 Ry
Begin final coordinates

ATOMIC_POSITIONS (bohr)
C        2.000000000   0.000000000   0.000000000
O        0.000000000   0.000000000   0.000000000
End final coordinates
     JOB DONE.
";

    #[test]
    fn relax_frames_energies_forces() {
        let t = parse_output_frames(RELAX).unwrap();
        assert_eq!(t.len(), 2, "header geometry + one step; final block is a repeat");
        assert_eq!(t.species, vec!["C", "O"]);
        let a = 10.0 * BOHR_TO_ANG;
        assert!((t.frames[0].lattice[0][0] - a).abs() < 1e-9);
        assert!((t.frames[0].positions[0][0] - 0.1 * a).abs() < 1e-9);
        assert!((t.frames[1].positions[0][0] - 2.0 * BOHR_TO_ANG).abs() < 1e-9);
        assert!((t.frames[0].energy.unwrap() + 43.0 * RY_TO_EV).abs() < 1e-9);
        assert!((t.frames[1].energy.unwrap() + 43.1 * RY_TO_EV).abs() < 1e-9);
        assert!((t.frames[0].max_force.unwrap() - 0.1 * RY_BOHR_TO_EV_ANG).abs() < 1e-9);
        // Fixed-cell relax has no CELL_PARAMETERS: the cell comes from the header.
        let s = parse_output(RELAX).unwrap();
        assert_eq!(s.atoms.len(), 2);
        assert!(s.atoms[0].magmom.is_none(), "no moments printed");
    }

    #[test]
    fn site_moments_go_to_the_geometry_being_computed_last_scf_wins() {
        let block = |m1: f64, m2: f64| {
            format!(
                "     Magnetic moment per site  (integrated on atomic sphere of radius R)\n\
                 \x20    atom   1 (R=0.177)  charge=  6.6  magn=  {m1}  constr=    0.0000\n\
                 \x20    atom   2 (R=0.177)  charge=  6.6  magn=  {m2}  constr=    0.0000\n\n"
            )
        };
        // Two SCF iterations for the first geometry, one for the second.
        let text = RELAX
            .replacen("!    total energy              =     -43.0", &format!("{}{}!    total energy              =     -43.0", block(1.0, -1.0), block(2.0, -2.0)), 1)
            .replacen("!    total energy              =     -43.1", &format!("{}!    total energy              =     -43.1", block(2.5, -2.5)), 1);
        let t = parse_output_frames(&text).unwrap();
        assert_eq!(t.frames[0].magmoms.as_ref().unwrap()[1], [0.0, 0.0, -2.0]);
        assert_eq!(t.frames[1].magmoms.as_ref().unwrap()[0], [0.0, 0.0, 2.5]);
        let s = parse_output(&text).unwrap();
        assert_eq!(s.atoms[1].magmom, Some([0.0, 0.0, -2.5]));
    }

    #[test]
    fn afm_writes_split_species_with_opposite_magnetization_and_reads_back() {
        let lat = [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]];
        let atom = |el: &str, x: f64, m: Option<[f64; 3]>| Atom {
            element: el.into(),
            position: [x, 0.0, 0.0],
            original_index: 0,
            oxidation: None,
            occupancy: 1.0,
            magmom: m,
        };
        let s = Structure {
            lattice: lat,
            atoms: vec![
                atom("Fe", 0.0, Some([0.0, 0.0, 3.6])),
                atom("O", 1.0, None),
                atom("Fe", 2.0, Some([0.0, 0.0, -3.6])),
            ],
            formula: String::new(),
            is_periodic: true,
        };
        let p = std::env::temp_dir().join(format!("cview_qe_afm_{}.in", std::process::id()));
        let path = p.to_str().unwrap();
        write(path, &s).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("ntyp = 3"), "{text}");
        assert!(text.contains("nspin = 2"));
        assert!(text.contains("starting_magnetization(1) = 0.500"), "{text}");
        // Species are listed per element: Fe1, Fe2, then O.
        assert!(text.contains("starting_magnetization(2) = -0.500"), "{text}");
        assert!(!text.contains("starting_magnetization(3)"), "O has none");
        assert!(text.contains(" Fe1 ") && text.contains(" Fe2 "));

        let back = parse(path).unwrap();
        let _ = std::fs::remove_file(&p);
        let els: Vec<&str> = back.atoms.iter().map(|a| a.element.as_str()).collect();
        assert_eq!(els, vec!["Fe", "O", "Fe"], "labels Fe1/Fe2 read as Fe");
    }

    #[test]
    fn labels_map_to_elements() {
        assert_eq!(element_of_label("Fe1"), "Fe");
        assert_eq!(element_of_label("Fe_up"), "Fe");
        assert_eq!(element_of_label("O"), "O");
        assert_eq!(element_of_label("Co2"), "Co");
        assert_eq!(element_of_label("C1"), "C");
    }

    #[test]
    fn a_texture_is_grouped_into_at_most_ten_species() {
        // A 12×12 spiral of Fe in the xz plane on an O layer: 144 distinct
        // Fe moments, far more than pw.x allows species.
        let mut atoms = Vec::new();
        for i in 0..144 {
            let t = i as f64 * std::f64::consts::TAU / 12.0;
            atoms.push(Atom {
                element: "Fe".into(),
                position: [(i % 12) as f64, (i / 12) as f64, 0.0],
                original_index: i,
                oxidation: None,
                occupancy: 1.0,
                magmom: Some([2.0 * t.sin(), 0.0, 2.0 * t.cos()]),
            });
        }
        atoms.push(Atom {
            element: "O".into(),
            position: [0.5, 0.5, 1.0],
            original_index: 144,
            oxidation: None,
            occupancy: 1.0,
            magmom: None,
        });
        let s = Structure {
            lattice: [[12.0, 0.0, 0.0], [0.0, 12.0, 0.0], [0.0, 0.0, 10.0]],
            atoms,
            formula: String::new(),
            is_periodic: true,
        };
        let (species, of_atom, grouped) = species_of(&s);
        assert!(grouped);
        assert_eq!(species.len(), MAX_SPECIES);
        assert_eq!(species.iter().filter(|sp| sp.element == "O").count(), 1);
        // Every atom's group moment is within 45° of its own (9 Fe groups
        // over a full circle of 12 directions).
        for (i, a) in s.atoms.iter().enumerate().take(144) {
            let m = a.magmom.unwrap();
            let g = species[of_atom[i]].moment;
            let cos = (m[0] * g[0] + m[2] * g[2]) / (2.0 * (g[0] * g[0] + g[2] * g[2]).sqrt());
            assert!(cos > 0.7, "atom {i}: {:?} vs group {:?}", m, g);
        }

        let p = std::env::temp_dir().join(format!("cview_qe_tex_{}.in", std::process::id()));
        write(p.to_str().unwrap(), &s).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(text.contains("ntyp = 10"));
        assert!(text.contains("noncolin = .true."));
        assert!(text.contains("Moments grouped into 10 species"));
    }

    #[test]
    fn few_moments_stay_exact() {
        let atom = |el: &str, m: Option<[f64; 3]>| Atom {
            element: el.into(),
            position: [0.0; 3],
            original_index: 0,
            oxidation: None,
            occupancy: 1.0,
            magmom: m,
        };
        let s = Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: vec![atom("Fe", Some([0.0, 0.0, 3.0])), atom("Fe", Some([0.0, 0.0, -3.0])), atom("O", None)],
            formula: String::new(),
            is_periodic: true,
        };
        let (species, of_atom, grouped) = species_of(&s);
        assert!(!grouped);
        let labels: Vec<&str> = species.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["Fe1", "Fe2", "O"]);
        assert_eq!(of_atom, vec![0, 1, 2]);
    }
}
