// src/io/pdb.rs
//
// PDB (Protein Data Bank) reader/writer.
//
// The format is column-based (PDB v3.30 spec), not whitespace-delimited:
// residue names, chain IDs and atom names may be blank or contain spaces, so
// splitting on whitespace mis-assigns fields on perfectly valid files. Every
// field below is therefore sliced by its documented column range, with a
// whitespace fallback used only when a coordinate field fails to parse.
//
// Supported records: CRYST1 (cell), MODEL/ENDMDL (first model only),
// ATOM/HETATM (sites), END. CONECT is parsed past and ignored — CView derives
// bonds from covalent radii at render time and has no per-structure bond list.

use crate::model::{Atom, Structure};
use crate::utils::console;
use crate::utils::linalg::{cart_to_frac, frac_to_cart};
use std::fs::File;
use std::io::Write;
use std::io::{self, BufRead};

/// Padding (Å) added on every side of the molecular bounding box when a file
/// carries no usable CRYST1 cell. The scene culls atoms further than ~0.55
/// cells outside the box, so the fallback box must actually enclose the
/// molecule — a fixed-size box would hide large ones.
const NONPERIODIC_PAD: f64 = 5.0;

/// Minimum edge length (Å) of the fallback box, so a diatomic doesn't get a
/// degenerate cell.
const MIN_BOX: f64 = 20.0;

// =========================================================================
// Reader
// =========================================================================

pub fn parse(path: &str) -> io::Result<Structure> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut atoms: Vec<Atom> = Vec::new();
    let mut cell: Option<[[f64; 3]; 3]> = None;
    let mut cryst1_rejected = false;

    let mut skipped_coords = 0usize;
    let mut skipped_altloc = 0usize;
    let mut fixed_occupancy = 0usize;
    let mut unknown_elements: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let chars: Vec<char> = line.chars().collect();
        let record = cols(&chars, 1, 6);
        let record = record.trim();

        match record {
            "CRYST1" => {
                match parse_cryst1(&chars) {
                    Some(lat) => cell = Some(lat),
                    // Placeholder/degenerate cells (the "1 1 1 90 90 90 P 1"
                    // convention, or zero-length axes) mean "no cell".
                    None => cryst1_rejected = true,
                }
            }
            // Only the first model is loaded, mirroring the XYZ parser's
            // first-frame rule: an NMR ensemble or MD trajectory would
            // otherwise be merged into one overlapping blob.
            "ENDMDL" => {
                if !atoms.is_empty() {
                    break;
                }
            }
            "MODEL" => {
                if !atoms.is_empty() {
                    break;
                }
            }
            "END" => break,
            "ATOM" | "HETATM" => {
                // altLoc: keep the blank and "A" alternates only, otherwise a
                // disordered site is loaded twice at near-identical positions.
                let alt = cols(&chars, 17, 17);
                let alt = alt.trim();
                if !alt.is_empty() && alt != "A" && alt != "1" {
                    skipped_altloc += 1;
                    continue;
                }

                let position = match parse_coords(&chars) {
                    Some(p) => p,
                    None => {
                        skipped_coords += 1;
                        continue;
                    }
                };

                let element = element_of(&chars);
                if crate::model::elements::get_atomic_number(&element) == 0
                    && !unknown_elements.contains(&element)
                {
                    unknown_elements.push(element.clone());
                }

                // Occupancy outside (0, 1] — blank, malformed, or the 0.00
                // used for unobserved atoms — becomes 1.0: a 0-weight site
                // would silently drop out of XRD and BVS sums.
                let occupancy = match cols(&chars, 55, 60).trim().parse::<f64>() {
                    Ok(v) if v > 0.0 && v <= 1.0 => v,
                    Ok(_) => {
                        fixed_occupancy += 1;
                        1.0
                    }
                    Err(_) => 1.0,
                };

                let oxidation = parse_charge(&cols(&chars, 79, 80));

                atoms.push(Atom {
                    element,
                    position,
                    original_index: atoms.len(),
                    oxidation,
                    occupancy,
                });
            }
            _ => {}
        }
    }

    if atoms.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No ATOM or HETATM records found in PDB file",
        ));
    }

    if skipped_altloc > 0 {
        console::log_info(&format!(
            "PDB: skipped {} alternate-location record(s); kept altLoc A",
            skipped_altloc
        ));
    }
    if skipped_coords > 0 {
        console::log_warn(&format!(
            "PDB: skipped {} record(s) with unreadable coordinates",
            skipped_coords
        ));
    }
    if fixed_occupancy > 0 {
        console::log_warn(&format!(
            "PDB: {} site(s) had occupancy outside (0, 1]; treated as fully occupied",
            fixed_occupancy
        ));
    }
    if !unknown_elements.is_empty() {
        console::log_warn(&format!(
            "PDB: unrecognized element symbol(s): {}",
            unknown_elements.join(", ")
        ));
    }

    let (lattice, is_periodic) = match cell {
        Some(lat) => (lat, true),
        None => {
            if cryst1_rejected {
                console::log_warn(
                    "PDB: CRYST1 record is a placeholder or has zero-length axes \
                     — loading as a non-periodic molecule",
                );
            }
            (fit_box(&mut atoms), false)
        }
    };

    Ok(Structure {
        lattice,
        atoms,
        formula: "PDB Import".to_string(),
        is_periodic,
    })
}

/// 1-indexed, inclusive column slice, clamped to the line length. PDB lines
/// are routinely truncated after the last non-blank field.
fn cols(chars: &[char], start: usize, end: usize) -> String {
    if start == 0 || start > chars.len() {
        return String::new();
    }
    let lo = start - 1;
    let hi = end.min(chars.len());
    chars[lo..hi].iter().collect()
}

/// Coordinates from columns 31-54. Falls back to whitespace tokens from
/// column 31 onward for writers that overflow or shift the fixed fields.
fn parse_coords(chars: &[char]) -> Option<[f64; 3]> {
    let x = cols(chars, 31, 38).trim().parse::<f64>();
    let y = cols(chars, 39, 46).trim().parse::<f64>();
    let z = cols(chars, 47, 54).trim().parse::<f64>();

    if let (Ok(x), Ok(y), Ok(z)) = (x, y, z) {
        return Some([x, y, z]);
    }

    let tail = cols(chars, 31, chars.len());
    let vals: Vec<f64> = tail
        .split_whitespace()
        .take(3)
        .filter_map(|t| t.parse::<f64>().ok())
        .collect();
    if vals.len() == 3 {
        Some([vals[0], vals[1], vals[2]])
    } else {
        None
    }
}

/// Element symbol: columns 77-78 when they hold a known symbol, otherwise
/// derived from the atom name in columns 13-16.
fn element_of(chars: &[char]) -> String {
    let field = cols(chars, 77, 78);
    let field = field.trim();
    if !field.is_empty() && field.chars().all(|c| c.is_ascii_alphabetic()) {
        let sym = normalize_element(field);
        if crate::model::elements::get_atomic_number(&sym) > 0 {
            return sym;
        }
    }
    element_from_name(&cols(chars, 13, 16))
}

/// Classic PDB atom-name convention: a two-character element symbol starts in
/// column 13, a one-character symbol starts in column 14. So " CA " is an
/// alpha-carbon and "CA  " is calcium. Names are also sometimes prefixed with
/// a digit ("1HB"), which is skipped.
fn element_from_name(name: &str) -> String {
    let c: Vec<char> = name.chars().collect();
    let known = |s: &str| crate::model::elements::get_atomic_number(s) > 0;

    if !c.is_empty() && c[0].is_ascii_alphabetic() {
        if c.len() > 1 && c[1].is_ascii_alphabetic() {
            let two = normalize_element(&format!("{}{}", c[0], c[1]));
            if known(&two) {
                return two;
            }
        }
        let one = normalize_element(&c[0].to_string());
        if known(&one) {
            return one;
        }
    }

    // Column 13 blank or a digit: the symbol is the first alphabetic run.
    let rest: String = name
        .chars()
        .skip_while(|ch| !ch.is_ascii_alphabetic())
        .take(2)
        .collect();
    if !rest.is_empty() {
        let one = normalize_element(&rest.chars().take(1).collect::<String>());
        if known(&one) {
            return one;
        }
        let two = normalize_element(&rest);
        if known(&two) {
            return two;
        }
        return one;
    }

    "X".to_string()
}

/// Title-case an up-to-two-letter symbol: "FE" -> "Fe", "c" -> "C".
fn normalize_element(s: &str) -> String {
    let alpha: String = s.chars().filter(|c| c.is_ascii_alphabetic()).take(2).collect();
    let mut out = String::with_capacity(alpha.len());
    for (i, ch) in alpha.chars().enumerate() {
        if i == 0 {
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Formal charge from columns 79-80. The spec writes the digit first ("2+"),
/// but sign-first ("+2") is common in the wild.
fn parse_charge(field: &str) -> Option<i32> {
    let t = field.trim();
    if t.is_empty() {
        return None;
    }
    let sign = if t.contains('-') {
        -1
    } else if t.contains('+') {
        1
    } else {
        return None;
    };
    let mag: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    let mag: i32 = mag.parse().unwrap_or(1);
    if mag == 0 { None } else { Some(sign * mag) }
}

/// CRYST1 -> lattice matrix, or None when the record carries no real cell.
fn parse_cryst1(chars: &[char]) -> Option<[[f64; 3]; 3]> {
    let a = cols(chars, 7, 15).trim().parse::<f64>().ok()?;
    let b = cols(chars, 16, 24).trim().parse::<f64>().ok()?;
    let c = cols(chars, 25, 33).trim().parse::<f64>().ok()?;
    let alpha = cols(chars, 34, 40).trim().parse::<f64>().ok()?;
    let beta = cols(chars, 41, 47).trim().parse::<f64>().ok()?;
    let gamma = cols(chars, 48, 54).trim().parse::<f64>().ok()?;

    if a <= 0.0 || b <= 0.0 || c <= 0.0 {
        return None;
    }
    for ang in [alpha, beta, gamma] {
        if ang <= 0.0 || ang >= 180.0 {
            return None;
        }
    }

    // The spec's "no cell" placeholder: a unit cube in P 1.
    let unit_edge = |v: f64| (v - 1.0).abs() < 1e-6;
    let right = |v: f64| (v - 90.0).abs() < 1e-6;
    if unit_edge(a)
        && unit_edge(b)
        && unit_edge(c)
        && right(alpha)
        && right(beta)
        && right(gamma)
    {
        return None;
    }

    let to_rad = std::f64::consts::PI / 180.0;
    let (ar, br, gr) = (alpha * to_rad, beta * to_rad, gamma * to_rad);
    let v = 1.0 - ar.cos().powi(2) - br.cos().powi(2) - gr.cos().powi(2)
        + 2.0 * ar.cos() * br.cos() * gr.cos();
    if v <= 0.0 {
        return None; // angles do not close a real cell
    }
    let v = v.sqrt();

    Some([
        [a, 0.0, 0.0],
        [b * gr.cos(), b * gr.sin(), 0.0],
        [
            c * br.cos(),
            c * (ar.cos() - br.cos() * gr.cos()) / gr.sin(),
            c * v / gr.sin(),
        ],
    ])
}

/// Build a bounding box for a molecule with no cell and shift the atoms into
/// it. CView anchors the cell at the Cartesian origin and the scene culls
/// atoms far outside it, so molecules with negative or large coordinates have
/// to be translated — a rigid shift, which changes nothing physical.
fn fit_box(atoms: &mut [Atom]) -> [[f64; 3]; 3] {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for a in atoms.iter() {
        for k in 0..3 {
            lo[k] = lo[k].min(a.position[k]);
            hi[k] = hi[k].max(a.position[k]);
        }
    }

    let mut edge = [0.0f64; 3];
    for k in 0..3 {
        edge[k] = (hi[k] - lo[k] + 2.0 * NONPERIODIC_PAD).max(MIN_BOX);
    }

    // Centre the molecule in the box.
    let mut shift = [0.0f64; 3];
    for (k, sh) in shift.iter_mut().enumerate() {
        *sh = edge[k] / 2.0 - (lo[k] + hi[k]) / 2.0;
    }
    for a in atoms.iter_mut() {
        for (k, p) in a.position.iter_mut().enumerate() {
            *p += shift[k];
        }
    }

    [
        [edge[0], 0.0, 0.0],
        [0.0, edge[1], 0.0],
        [0.0, 0.0, edge[2]],
    ]
}

// =========================================================================
// Writer
// =========================================================================

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = File::create(path)?;

    writeln!(file, "REMARK   Written by CView")?;

    // CRYST1 is expressed as a, b, c, α, β, γ, which fixes the cell in the
    // standard orientation (a along x, b in the xy-plane). A structure whose
    // lattice is rotated relative to that frame has to have its Cartesian
    // coordinates re-expressed in the standard frame, or the atoms end up
    // rotated with respect to the cell on reload.
    let positions: Vec<[f64; 3]> = if structure.is_periodic {
        let (a, b, c, alpha, beta, gamma) = cell_params(&structure.lattice);
        writeln!(
            file,
            "CRYST1{:9.3}{:9.3}{:9.3}{:7.2}{:7.2}{:7.2} {:<11}{:4}",
            a, b, c, alpha, beta, gamma, "P 1", 1
        )?;
        reorient(structure)
    } else {
        // The spec's "no cell" placeholder, so the file reloads as a molecule.
        writeln!(
            file,
            "CRYST1{:9.3}{:9.3}{:9.3}{:7.2}{:7.2}{:7.2} {:<11}{:4}",
            1.0, 1.0, 1.0, 90.0, 90.0, 90.0, "P 1", 1
        )?;
        structure.atoms.iter().map(|a| a.position).collect()
    };

    // Serial numbers have five columns. Past 99999 the field overflows and
    // shifts every column after it, so the counter wraps instead.
    if structure.atoms.len() > 99999 {
        console::log_warn(
            "PDB export: more than 99999 atoms — serial numbers wrap to keep columns aligned",
        );
    }

    let mut per_element: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for (i, atom) in structure.atoms.iter().enumerate() {
        let serial = i % 99999 + 1;
        let n = per_element.entry(atom.element.as_str()).or_insert(0);
        *n += 1;

        let pos = positions[i];
        let charge = match atom.oxidation {
            Some(q) if q != 0 => format!("{}{}", q.abs().min(9), if q > 0 { '+' } else { '-' }),
            _ => "  ".to_string(),
        };

        writeln!(
            file,
            "HETATM{:5} {:<4} MOL A{:4}    {:8.3}{:8.3}{:8.3}{:6.2}{:6.2}          {:>2}{:<2}",
            serial,
            atom_name(&atom.element, *n),
            1,
            pos[0],
            pos[1],
            pos[2],
            atom.occupancy.clamp(0.0, 1.0),
            0.0,
            atom.element.to_uppercase(),
            charge,
        )?;
    }

    writeln!(file, "END")?;
    Ok(())
}

/// Atom name field (columns 13-16). A one-letter element is indented by one
/// column per the PDB convention; a per-element counter is appended when it
/// fits in the remaining width.
fn atom_name(element: &str, n: usize) -> String {
    let sym = element.to_uppercase();
    if sym.len() >= 2 {
        let tag = format!("{}{}", sym, n);
        if tag.len() <= 4 { tag } else { sym }
    } else {
        let tag = format!("{}{}", sym, n);
        if tag.len() <= 3 {
            format!(" {}", tag)
        } else {
            format!(" {}", sym)
        }
    }
}

/// Lattice vectors -> (a, b, c, α, β, γ) in Å and degrees.
fn cell_params(lat: &[[f64; 3]; 3]) -> (f64, f64, f64, f64, f64, f64) {
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];

    let (av, bv, cv) = (lat[0], lat[1], lat[2]);
    let (a, b, c) = (norm(av), norm(bv), norm(cv));
    let to_deg = 180.0 / std::f64::consts::PI;

    let safe = |num: f64, den: f64| {
        if den.abs() < 1e-12 {
            90.0
        } else {
            (num / den).clamp(-1.0, 1.0).acos() * to_deg
        }
    };

    (
        a,
        b,
        c,
        safe(dot(bv, cv), b * c),
        safe(dot(av, cv), a * c),
        safe(dot(av, bv), a * b),
    )
}

/// Re-express Cartesian positions in the CRYST1 standard frame.
fn reorient(structure: &Structure) -> Vec<[f64; 3]> {
    let (a, b, c, alpha, beta, gamma) = cell_params(&structure.lattice);
    let to_rad = std::f64::consts::PI / 180.0;
    let (ar, br, gr) = (alpha * to_rad, beta * to_rad, gamma * to_rad);
    let v = 1.0 - ar.cos().powi(2) - br.cos().powi(2) - gr.cos().powi(2)
        + 2.0 * ar.cos() * br.cos() * gr.cos();

    if v <= 0.0 || gr.sin().abs() < 1e-12 {
        return structure.atoms.iter().map(|a| a.position).collect();
    }
    let std_lat = [
        [a, 0.0, 0.0],
        [b * gr.cos(), b * gr.sin(), 0.0],
        [
            c * br.cos(),
            c * (ar.cos() - br.cos() * gr.cos()) / gr.sin(),
            c * v.sqrt() / gr.sin(),
        ],
    ];

    structure
        .atoms
        .iter()
        .map(|atom| match cart_to_frac(atom.position, structure.lattice) {
            Some(f) => frac_to_cart(f, std_lat),
            None => atom.position,
        })
        .collect()
}

// =========================================================================
// Tests
// =========================================================================

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
            p.push(format!("cview_pdb_{}_{}.pdb", std::process::id(), n));
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
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    #[test]
    fn cryst1_cell_is_periodic_and_orthogonal() {
        let f = TmpFile::new(
            "CRYST1    4.000    4.000    4.000  90.00  90.00  90.00 P 1           1\n\
             ATOM      1 PO   MOL A   1       1.000   2.000   3.000  1.00  0.00          PO\n\
             END\n",
        );
        let s = parse(f.path()).unwrap();
        assert!(s.is_periodic);
        approx(s.lattice[0][0], 4.0);
        approx(s.lattice[1][1], 4.0);
        approx(s.lattice[2][2], 4.0);
        // Periodic structures keep their coordinates untouched.
        approx(s.atoms[0].position[0], 1.0);
        approx(s.atoms[0].position[2], 3.0);
    }

    #[test]
    fn placeholder_cryst1_loads_as_molecule() {
        let f = TmpFile::new(
            "CRYST1    1.000    1.000    1.000  90.00  90.00  90.00 P 1           1\n\
             HETATM    1  C1  UNL     1       0.000   0.000   0.000  1.00  0.00           C\n\
             HETATM    2  O1  UNL     1       1.200   0.000   0.000  1.00  0.00           O\n\
             END\n",
        );
        let s = parse(f.path()).unwrap();
        assert!(!s.is_periodic);
        assert_eq!(s.atoms.len(), 2);
        // Box is at least MIN_BOX on every edge and the bond length survives
        // the centring shift.
        assert!(s.lattice[0][0] >= MIN_BOX);
        approx(s.atoms[1].position[0] - s.atoms[0].position[0], 1.2);
    }

    #[test]
    fn zero_length_axes_fall_back_to_bounding_box() {
        let f = TmpFile::new(
            "CRYST1    0.000    0.000   12.780  90.00  90.00  90.00 P 1\n\
             ATOM      1    C MOL     1       4.919   0.000   0.000  1.00  0.00           C\n\
             ATOM      2    C MOL     1       6.149   0.000   0.710  1.00  0.00           C\n\
             ENDMDL\n",
        );
        let s = parse(f.path()).unwrap();
        assert!(!s.is_periodic);
        assert!(s.lattice[2][2] > 0.0);
    }

    #[test]
    fn no_cell_record_encloses_and_centres_the_molecule() {
        // Coordinates far from the origin must end up inside the box, or the
        // scene culls them.
        let f = TmpFile::new(
            "HETATM    1  C1  UNL     1     -80.000   0.000   0.000  1.00  0.00           C\n\
             HETATM    2  C1  UNL     1      80.000   0.000   0.000  1.00  0.00           C\n",
        );
        let s = parse(f.path()).unwrap();
        assert!(!s.is_periodic);
        for a in &s.atoms {
            for k in 0..3 {
                assert!(
                    a.position[k] >= 0.0 && a.position[k] <= s.lattice[k][k],
                    "atom outside fallback box: {:?}",
                    a.position
                );
            }
        }
        approx(s.atoms[1].position[0] - s.atoms[0].position[0], 160.0);
    }

    #[test]
    fn only_the_first_model_is_read() {
        let f = TmpFile::new(
            "MODEL        1\n\
             HETATM    1  C1  UNL     1       0.000   0.000   0.000  1.00  0.00           C\n\
             ENDMDL\n\
             MODEL        2\n\
             HETATM    1  C1  UNL     1       9.000   9.000   9.000  1.00  0.00           C\n\
             ENDMDL\n\
             END\n",
        );
        let s = parse(f.path()).unwrap();
        assert_eq!(s.atoms.len(), 1);
    }

    #[test]
    fn element_column_wins_over_atom_name() {
        // " CA " is an alpha carbon by name, but columns 77-78 say calcium.
        let f = TmpFile::new(
            "HETATM    1  CA  MOL A   1       0.000   0.000   0.000  1.00  0.00          CA\n",
        );
        let s = parse(f.path()).unwrap();
        assert_eq!(s.atoms[0].element, "Ca");
    }

    #[test]
    fn element_derived_from_name_when_column_is_missing() {
        // No columns 77-78: " CA " -> carbon (one-letter symbol in col 14),
        // "FE  " -> iron (two-letter symbol in col 13).
        let f = TmpFile::new(
            "ATOM      1  CA  ALA A   1       0.000   0.000   0.000  1.00  0.00\n\
             HETATM    2 FE   FE  A   2       1.000   0.000   0.000  1.00  0.00\n\
             ATOM      3 1HB  ALA A   1       2.000   0.000   0.000  1.00  0.00\n",
        );
        let s = parse(f.path()).unwrap();
        assert_eq!(s.atoms[0].element, "C");
        assert_eq!(s.atoms[1].element, "Fe");
        assert_eq!(s.atoms[2].element, "H");
    }

    #[test]
    fn alternate_locations_other_than_a_are_dropped() {
        let f = TmpFile::new(
            "HETATM    1  C1 AUNL     1       0.000   0.000   0.000  1.00  0.00           C\n\
             HETATM    2  C1 BUNL     1       0.100   0.000   0.000  1.00  0.00           C\n",
        );
        let s = parse(f.path()).unwrap();
        assert_eq!(s.atoms.len(), 1);
    }

    #[test]
    fn occupancy_and_charge_are_read() {
        let f = TmpFile::new(
            "CRYST1    5.000    5.000    5.000  90.00  90.00  90.00 P 1           1\n\
             HETATM    1 FE   MOL A   1       0.000   0.000   0.000  0.50  0.00          FE3+\n\
             HETATM    2  O   MOL A   1       2.000   0.000   0.000  0.00  0.00           O2-\n",
        );
        let s = parse(f.path()).unwrap();
        approx(s.atoms[0].occupancy, 0.5);
        assert_eq!(s.atoms[0].oxidation, Some(3));
        // Zero occupancy is promoted to full so the site still counts.
        approx(s.atoms[1].occupancy, 1.0);
        assert_eq!(s.atoms[1].oxidation, Some(-2));
    }

    #[test]
    fn file_without_atoms_is_an_error() {
        let f = TmpFile::new("HEADER    NOTHING HERE\nEND\n");
        assert!(parse(f.path()).is_err());
    }

    #[test]
    fn written_records_use_the_spec_columns() {
        let s = Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: vec![Atom {
                element: "Fe".into(),
                position: [1.0, 2.0, 3.0],
                original_index: 0,
                oxidation: Some(3),
                occupancy: 1.0,
            }],
            formula: String::new(),
            is_periodic: true,
        };
        let f = TmpFile::new("");
        write(f.path(), &s).unwrap();
        let text = std::fs::read_to_string(f.path()).unwrap();
        let rec = text
            .lines()
            .find(|l| l.starts_with("HETATM"))
            .expect("no HETATM record");
        assert_eq!(rec.len(), 80, "record is not 80 columns: {rec:?}");
        assert_eq!(&rec[30..38], "   1.000");
        assert_eq!(&rec[38..46], "   2.000");
        assert_eq!(&rec[46..54], "   3.000");
        assert_eq!(&rec[76..78], "FE");
        assert_eq!(&rec[78..80], "3+");
    }

    #[test]
    fn periodic_roundtrip_preserves_cell_and_positions() {
        let original = Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 6.0]],
            atoms: vec![
                Atom {
                    element: "Ba".into(),
                    position: [0.0, 0.0, 0.0],
                    original_index: 0,
                    oxidation: Some(2),
                    occupancy: 1.0,
                },
                Atom {
                    element: "O".into(),
                    position: [2.0, 2.5, 3.0],
                    original_index: 1,
                    oxidation: Some(-2),
                    occupancy: 0.75,
                },
            ],
            formula: String::new(),
            is_periodic: true,
        };
        let f = TmpFile::new("");
        write(f.path(), &original).unwrap();
        let s = parse(f.path()).unwrap();

        assert!(s.is_periodic);
        approx(s.lattice[0][0], 4.0);
        approx(s.lattice[1][1], 5.0);
        approx(s.lattice[2][2], 6.0);
        assert_eq!(s.atoms.len(), 2);
        assert_eq!(s.atoms[0].element, "Ba");
        assert_eq!(s.atoms[1].element, "O");
        approx(s.atoms[1].position[1], 2.5);
        approx(s.atoms[1].occupancy, 0.75);
        assert_eq!(s.atoms[1].oxidation, Some(-2));
    }

    #[test]
    fn rotated_cell_is_written_in_the_standard_orientation() {
        // Same cell as above, rotated 90° about z: a along +y, b along -x.
        // The CRYST1 record can only describe the standard frame, so the
        // writer must rotate the coordinates to match.
        let original = Structure {
            lattice: [[0.0, 4.0, 0.0], [-5.0, 0.0, 0.0], [0.0, 0.0, 6.0]],
            atoms: vec![Atom {
                element: "O".into(),
                // frac (0.5, 0.5, 0.5)
                position: [-2.5, 2.0, 3.0],
                original_index: 0,
                oxidation: None,
                occupancy: 1.0,
            }],
            formula: String::new(),
            is_periodic: true,
        };
        let f = TmpFile::new("");
        write(f.path(), &original).unwrap();
        let s = parse(f.path()).unwrap();

        approx(s.lattice[0][0], 4.0);
        approx(s.lattice[1][1], 5.0);
        // Body centre of the re-oriented cell.
        approx(s.atoms[0].position[0], 2.0);
        approx(s.atoms[0].position[1], 2.5);
        approx(s.atoms[0].position[2], 3.0);
    }

    #[test]
    fn molecular_roundtrip_stays_non_periodic() {
        let original = Structure {
            lattice: [[30.0, 0.0, 0.0], [0.0, 30.0, 0.0], [0.0, 0.0, 30.0]],
            atoms: vec![
                Atom {
                    element: "C".into(),
                    position: [10.0, 10.0, 10.0],
                    original_index: 0,
                    oxidation: None,
                    occupancy: 1.0,
                },
                Atom {
                    element: "F".into(),
                    position: [11.35, 10.0, 10.0],
                    original_index: 1,
                    oxidation: None,
                    occupancy: 1.0,
                },
            ],
            formula: String::new(),
            is_periodic: false,
        };
        let f = TmpFile::new("");
        write(f.path(), &original).unwrap();
        let s = parse(f.path()).unwrap();

        assert!(!s.is_periodic);
        assert_eq!(s.atoms[0].element, "C");
        assert_eq!(s.atoms[1].element, "F");
        // Bond length survives the re-centring.
        approx(s.atoms[1].position[0] - s.atoms[0].position[0], 1.35);
    }
}
