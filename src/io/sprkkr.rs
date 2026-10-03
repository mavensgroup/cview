// src/io/sprkkr.rs
// State-of-the-art SPR-KKR .pot/.sys parser and writer
// Based on ase2sprkkr specification: https://github.com/ase2sprkkr/ase2sprkkr
//
// SPR-KKR Format Overview:
// ========================
// SPR-KKR (Spin-Polarized Relativistic Korringa-Kohn-Rostoker) is a DFT code
// for electronic structure calculations. It uses structured text format with sections:
//
// HEADER     - System description and metadata
// LATTICE    - Crystal structure (ALAT, lattice vectors, Bravais type)
// SITES      - Atomic positions (Cartesian or Direct coordinates)
// OCCUPATION - Chemical composition at each site (supports disorder/alloys)
// TYPES      - Element definitions with atomic numbers and parameters
// POTENTIAL  - Optional: DFT potential data (not parsed; the writer omits it, SPR-KKR makes it)
//
// Key Features Supported:
// - Chemical disorder (e.g., Fe₀.₅Co₀.₅ alloys)
// - Both Cartesian and Direct (fractional) coordinates
// - ALAT scaling (auto-detect Bohr vs Angstrom)
// - BASSCALE for anisotropic position scaling
// - Multiple occupation at single site

use crate::model::elements::get_atomic_number;
use crate::model::{Atom, Structure};
use crate::utils::linalg::frac_to_cart;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

const BOHR_TO_ANG: f64 = 0.52917721092; // CODATA 2018

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    None,
    Header,
    Lattice,
    Sites,
    Occupation,
    Types,
    #[allow(dead_code)]
    Potential,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CoordSystem {
    Cartesian, // Positions in Angstroms
    Direct,    // Fractional/Crystal coordinates
}

/// Occupation at a site (supports chemical disorder)
#[derive(Debug, Clone)]
struct SiteOccupation {
    type_id: usize,
    concentration: f64,
}

/// Complete SPR-KKR structure data
#[derive(Debug)]
struct SprkkrData {
    // Header
    header: String,
    system_name: String,

    // Lattice
    alat: f64,                      // Lattice constant (converted to Angstrom)
    lattice_vectors: [[f64; 3]; 3], // In units of ALAT
    bravais_type: String,           // e.g., "cubic m3m", "fcc", "bcc"
    sysdim: String,                 // "3D", "2D", "1D"
    systype: String,                // "BULK", "SLAB", etc.

    // Sites
    coord_system: CoordSystem,
    basscale: [f64; 3], // Anisotropic scaling
    site_positions: HashMap<usize, [f64; 3]>,

    // Occupation (chemical disorder support)
    site_occupation: HashMap<usize, Vec<SiteOccupation>>,

    // Types
    type_data: HashMap<usize, (String, usize)>, // type_id -> (element, atomic_number)
}

impl Default for SprkkrData {
    fn default() -> Self {
        Self {
            header: String::from("Exported by CView"),
            system_name: String::from("Structure"),
            alat: 1.0,
            lattice_vectors: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            bravais_type: String::new(),
            sysdim: String::from("3D"),
            systype: String::from("BULK"),
            coord_system: CoordSystem::Cartesian,
            basscale: [1.0, 1.0, 1.0],
            site_positions: HashMap::new(),
            site_occupation: HashMap::new(),
            type_data: HashMap::new(),
        }
    }
}

// ============================================================================
// PARSER
// ============================================================================

pub fn parse(path: &str) -> io::Result<Structure> {
    let path = Path::new(path);
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut data = SprkkrData::default();
    let mut current_section = Section::None;
    let mut found_lattice_vectors = [false; 3];

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Section delimiters (asterisk lines)
        if trimmed.starts_with("*****") {
            current_section = Section::None;
            continue;
        }

        // Section headers
        if trimmed.starts_with("HEADER") {
            current_section = Section::Header;
            // Extract header text if quoted
            if let Some(start) = trimmed.find('\'') {
                if let Some(end) = trimmed[start + 1..].find('\'') {
                    data.header = trimmed[start + 1..start + 1 + end].to_string();
                }
            }
            continue;
        }
        if trimmed == "LATTICE" {
            current_section = Section::Lattice;
            continue;
        }
        if trimmed == "SITES" {
            current_section = Section::Sites;
            continue;
        }
        if trimmed == "OCCUPATION" {
            current_section = Section::Occupation;
            continue;
        }
        if trimmed == "TYPES" {
            current_section = Section::Types;
            continue;
        }
        if trimmed == "POTENTIAL" {
            // We don't parse the POTENTIAL block; all structural data
            // (lattice, sites, occupation, types) has been collected by
            // the time we reach it. Bail out so we don't walk through
            // what can be tens of thousands of numeric table lines.
            break;
        }

        // Parse section content
        match current_section {
            Section::Header => {
                parse_header_line(trimmed, &mut data);
            }

            Section::Lattice => {
                parse_lattice_line(trimmed, &mut data, &mut found_lattice_vectors)?;
            }

            Section::Sites => {
                parse_sites_line(trimmed, &mut data)?;
            }

            Section::Occupation => {
                parse_occupation_line(trimmed, &mut data)?;
            }

            Section::Types => {
                parse_types_line(trimmed, &mut data)?;
            }

            _ => {}
        }
    }

    // Validate lattice
    if !found_lattice_vectors[0] || !found_lattice_vectors[1] || !found_lattice_vectors[2] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Missing lattice vectors: {:?}", found_lattice_vectors),
        ));
    }

    // Build final Structure
    build_structure(data)
}

fn parse_header_line(line: &str, data: &mut SprkkrData) {
    if line.starts_with("SYSTEM") {
        data.system_name = line
            .split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
            .join(" ");
    }
}

fn parse_lattice_line(line: &str, data: &mut SprkkrData, found: &mut [bool; 3]) -> io::Result<()> {
    // SYSDIM: 3D, 2D, 1D
    if line.starts_with("SYSDIM") {
        if let Some(val) = line.split_whitespace().nth(1) {
            data.sysdim = val.to_string();
        }
        return Ok(());
    }

    // SYSTYPE: BULK, SLAB, WIRE, etc.
    if line.starts_with("SYSTYPE") {
        if let Some(val) = line.split_whitespace().nth(1) {
            data.systype = val.to_string();
        }
        return Ok(());
    }

    // BRAVAIS: lattice type and space group info
    if line.starts_with("BRAVAIS") {
        let parts: Vec<&str> = line.split_whitespace().collect();
        data.bravais_type = parts
            .iter()
            .skip(1)
            .filter(|s| !s.chars().all(char::is_numeric))
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(());
    }

    // ALAT: lattice constant
    if line.starts_with("ALAT") {
        if let Some(val) = extract_first_number(line) {
            // Heuristic: ALAT > 2.0 is likely Bohr, otherwise Angstrom
            // SPR-KKR typically uses a.u. (Bohr) for lattice parameters
            data.alat = if val > 2.0 { val * BOHR_TO_ANG } else { val };
        }
        return Ok(());
    }

    // Lattice vectors: A(1), A(2), A(3) or A1, A2, A3
    if line.starts_with("A(")
        || (line.starts_with('A')
            && line.len() > 1
            && line.chars().nth(1).is_some_and(|c| c.is_numeric()))
    {
        let idx = if line.starts_with("A(1)") || line.starts_with("A1") {
            0
        } else if line.starts_with("A(2)") || line.starts_with("A2") {
            1
        } else if line.starts_with("A(3)") || line.starts_with("A3") {
            2
        } else {
            return Ok(());
        };

        if let Some(vec) = parse_vec3_flexible(line) {
            data.lattice_vectors[idx] = vec;
            found[idx] = true;
        }
    }

    Ok(())
}

fn parse_sites_line(line: &str, data: &mut SprkkrData) -> io::Result<()> {
    // CARTESIAN T/F - coordinate system
    if line.starts_with("CARTESIAN") {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            data.coord_system = if parts[1] == "T" || parts[1].to_uppercase() == "TRUE" {
                CoordSystem::Cartesian
            } else {
                CoordSystem::Direct
            };
        }
        return Ok(());
    }

    // BASSCALE - scaling factors
    if line.starts_with("BASSCALE") {
        if let Some(vec) = parse_vec3_flexible(line) {
            data.basscale = vec;
        }
        return Ok(());
    }

    // Skip header lines
    if line.starts_with("IQ") || line.starts_with("CART") {
        return Ok(());
    }

    // Parse site positions
    // Format: "  1    0.000000000  0.500000000  0.500000000"
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 4 {
        if let Ok(site_id) = parts[0].parse::<usize>() {
            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[1].parse::<f64>(),
                parts[2].parse::<f64>(),
                parts[3].parse::<f64>(),
            ) {
                // Apply BASSCALE
                let pos = [
                    x * data.basscale[0],
                    y * data.basscale[1],
                    z * data.basscale[2],
                ];
                data.site_positions.insert(site_id, pos);
            }
        }
    }

    Ok(())
}

fn parse_occupation_line(line: &str, data: &mut SprkkrData) -> io::Result<()> {
    // Skip header
    if line.starts_with("IQ") {
        return Ok(());
    }

    // Format: "  1       1       1       2     1   0.50000     2   0.50000"
    //         IQ   IREFQ    IMQ    NOQ  ITOQ1 CONC1  ITOQ2 CONC2 ...
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() >= 4 {
        if let (Ok(site_id), Ok(noq)) = (parts[0].parse::<usize>(), parts[3].parse::<usize>()) {
            let mut occupations = Vec::new();
            let mut cursor = 4;

            // Parse NOQ occupation pairs (type_id, concentration)
            for _ in 0..noq {
                if cursor + 1 < parts.len() {
                    if let (Ok(type_id), Ok(concentration)) = (
                        parts[cursor].parse::<usize>(),
                        parts[cursor + 1].parse::<f64>(),
                    ) {
                        occupations.push(SiteOccupation {
                            type_id,
                            concentration,
                        });
                    }
                    cursor += 2;
                }
            }

            if !occupations.is_empty() {
                data.site_occupation.insert(site_id, occupations);
            }
        }
    }

    Ok(())
}

fn parse_types_line(line: &str, data: &mut SprkkrData) -> io::Result<()> {
    // Skip header
    if line.starts_with("IT") {
        return Ok(());
    }

    // Format: "  1     Fe              26       0       0       0       0.0"
    //         IT   TXTT            ZT      NC      LC      KC      VC
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() >= 3 {
        if let (Ok(type_id), Ok(atomic_number)) =
            (parts[0].parse::<usize>(), parts[2].parse::<usize>())
        {
            // Clean element symbol (remove non-alphabetic characters)
            let element = parts[1]
                .chars()
                .filter(|c| c.is_alphabetic())
                .collect::<String>();

            if !element.is_empty() {
                data.type_data.insert(type_id, (element, atomic_number));
            }
        }
    }

    Ok(())
}

fn build_structure(data: SprkkrData) -> io::Result<Structure> {
    // Scale lattice vectors by ALAT
    let mut lattice = data.lattice_vectors;
    for i in 0..3 {
        for j in 0..3 {
            lattice[i][j] *= data.alat;
        }
    }

    // Build atoms
    let mut atoms = Vec::new();
    let mut sorted_ids: Vec<usize> = data.site_positions.keys().cloned().collect();
    sorted_ids.sort();

    let mut cpa_sites = 0usize;
    for site_id in sorted_ids {
        if let Some(site_pos) = data.site_positions.get(&site_id) {
            // Convert position to Cartesian if needed
            let position = if data.coord_system == CoordSystem::Direct {
                // Fractional to Cartesian
                frac_to_cart([site_pos[0], site_pos[1], site_pos[2]], lattice)
            } else {
                // Already Cartesian, just scale by ALAT
                [
                    site_pos[0] * data.alat,
                    site_pos[1] * data.alat,
                    site_pos[2] * data.alat,
                ]
            };

            // ALL occupants of the site, in the file's ITOQ order. A CPA
            // disorder site (NOQ > 1, e.g. Fe 0.7 / Cr 0.3) becomes coincident
            // atoms with occupancy = concentration — XRD and BVS weight
            // them per the virtual-crystal approximation. The file order is
            // kept so a file written back lists its types as it was read; the
            // majority species is picked where it matters (PartialSites).
            let occs: Vec<(usize, f64)> = data
                .site_occupation
                .get(&site_id)
                .map(|v| v.iter().map(|o| (o.type_id, o.concentration)).collect())
                .unwrap_or_else(|| vec![(1, 1.0)]);
            if occs.len() > 1 {
                cpa_sites += 1;
            }

            for (type_id, concentration) in occs {
                let element = data
                    .type_data
                    .get(&type_id)
                    .map(|(el, _)| el.clone())
                    .unwrap_or_else(|| String::from("X"));

                atoms.push(Atom {
                    element,
                    position,
                    original_index: atoms.len(),
                    oxidation: None,
                    occupancy: concentration.clamp(0.0, 1.0),
                });
            }
        }
    }

    if cpa_sites > 0 {
        crate::utils::console::log_info(&format!(
            "SPR-KKR: {cpa_sites} CPA disorder site(s) imported as coincident atoms with occupancy = concentration"
        ));
    }

    // Generate formula
    let formula = if !data.system_name.is_empty() {
        if !data.bravais_type.is_empty() {
            format!("{} ({})", data.system_name, data.bravais_type)
        } else {
            data.system_name
        }
    } else {
        String::from("SPR-KKR Import")
    };

    Ok(Structure {
        lattice,
        atoms,
        formula,
        is_periodic: true,
    })
}

// ============================================================================
// WRITER
// ============================================================================
//
// Built-in writer, used when ase2sprkkr is not available (see `write`). Writes an
// SPR-KKR potential file for a fresh calculation (FORMAT 7, the
// layout ase2sprkkr produces and `kkrscf` starts from), without the POTENTIAL
// block, which SPR-KKR generates itself.
//
// How SPR-KKR holds a disordered alloy (CPA), and so how it is written here:
//   * a SITE (IQ) is a crystallographic position, listed once;
//   * OCCUPATION gives each site NOQ occupants, each a TYPE (ITOQ) with a
//     concentration (CONC); the concentrations on a site sum to 1;
//   * every (site, element) pair is its own TYPE, with its own potential, so
//     Fe on site 1 and Fe on site 2 are different types (NT = sum of NOQ).
// CView holds a mixed site as several atoms at one position, each with an
// occupancy; those atoms become one site with NOQ occupants.

/// Atoms sharing one position: a single entry for an ordinary atom, several for
/// a mixed (CPA) site. Only partially occupied atoms are merged; two fully
/// occupied atoms at one position are an overlap, not a mixture.
fn group_sites(structure: &Structure) -> Vec<Vec<usize>> {
    // Same tolerance PartialSites uses, so both agree on what one site is.
    const SITE_TOLERANCE: f64 = 0.02; // Å
    const FULL: f64 = 0.999;
    let atoms = &structure.atoms;

    let separation = |i: usize, j: usize| -> f64 {
        let d = [
            atoms[j].position[0] - atoms[i].position[0],
            atoms[j].position[1] - atoms[i].position[1],
            atoms[j].position[2] - atoms[i].position[2],
        ];
        let v = if structure.is_periodic {
            crate::utils::geometry::minimum_image(d, &structure.lattice).map_or(d, |(v, _)| v)
        } else {
            d
        };
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    };

    let mut taken = vec![false; atoms.len()];
    let mut sites: Vec<Vec<usize>> = Vec::new();
    for i in 0..atoms.len() {
        if taken[i] {
            continue;
        }
        taken[i] = true;
        let mut members = vec![i];
        if atoms[i].occupancy < FULL {
            for j in (i + 1)..atoms.len() {
                if !taken[j] && atoms[j].occupancy < FULL && separation(i, j) < SITE_TOLERANCE {
                    taken[j] = true;
                    members.push(j);
                }
            }
        }
        sites.push(members);
    }
    sites
}

/// The cell in the standard setting SPR-KKR files use: a along x, b in the xy
/// plane. A rigid rotation (a reflection if the input cell is left-handed), so
/// it changes no distance or angle.
fn standard_lattice(l: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (a, b, c) = (len(l[0]), len(l[1]), len(l[2]));
    let cos_a = dot(l[1], l[2]) / (b * c);
    let cos_b = dot(l[0], l[2]) / (a * c);
    let cos_g = dot(l[0], l[1]) / (a * b);
    let sin_g = (1.0 - cos_g * cos_g).max(0.0).sqrt();

    let cx = c * cos_b;
    let cy = c * (cos_a - cos_b * cos_g) / sin_g;
    let cz = (c * c - cx * cx - cy * cy).max(0.0).sqrt();
    let det = l[0][0] * (l[1][1] * l[2][2] - l[1][2] * l[2][1])
        - l[0][1] * (l[1][0] * l[2][2] - l[1][2] * l[2][0])
        + l[0][2] * (l[1][0] * l[2][1] - l[1][1] * l[2][0]);
    let cz = if det < 0.0 { -cz } else { cz };
    [
        [a, 0.0, 0.0],
        [b * cos_g, b * sin_g, 0.0],
        [cx, cy, cz],
    ]
}

/// SPR-KKR's BRAVAIS entry for a lattice, from the lattice alone (the atoms are
/// ignored: a single atom per cell is analysed, so only the translation
/// lattice of the cell as written counts).
fn bravais_entry(lattice: [[f64; 3]; 3]) -> String {
    use crate::physics::analysis::bravais::lattice_family;
    let probe = Structure {
        lattice,
        atoms: vec![Atom {
            element: "H".to_string(),
            position: [0.0; 3],
            original_index: 0,
            oxidation: None,
            occupancy: 1.0,
        }],
        formula: String::new(),
        is_periodic: true,
    };
    let sg = crate::physics::analysis::symmetry::analyze(&probe)
        .map(|s| s.number)
        .unwrap_or(1);
    let (system, centre) = lattice_family(sg);
    // SPR-KKR's list of 14, spelled as in xband's geometry.f (the code that
    // writes these files), including "orthorombic" and "base centered".
    const TABLE: [&str; 14] = [
        "triclinic   primitive      -1     C_i",
        "monoclinic  primitive      2/m    C_2h",
        "monoclinic  base centered  2/m    C_2h",
        "orthorombic primitive      mmm    D_2h",
        "orthorombic base-centered  mmm    D_2h",
        "orthorombic body-centered  mmm    D_2h",
        "orthorombic face-centered  mmm    D_2h",
        "tetragonal  primitive      4/mmm  D_4h",
        "tetragonal  body-centered  4/mmm  D_4h",
        "trigonal    primitive      -3m    D_3d",
        "hexagonal   primitive      6/mmm  D_6h",
        "cubic       primitive      m3m    O_h",
        "cubic       face-centered  m3m    O_h",
        "cubic       body-centered  m3m    O_h",
    ];
    let idx = match (system, centre) {
        ("triclinic", _) => 1,
        ("monoclinic", 'P') => 2,
        ("monoclinic", _) => 3,
        ("orthorhombic", 'P') => 4,
        ("orthorhombic", 'C') => 5,
        ("orthorhombic", 'I') => 6,
        ("orthorhombic", _) => 7,
        ("tetragonal", 'P') => 8,
        ("tetragonal", _) => 9,
        ("trigonal", 'R') => 10,
        ("trigonal", _) | ("hexagonal", _) => 11,
        ("cubic", 'F') => 13,
        ("cubic", 'I') => 14,
        _ => 12,
    };
    format!("{:>3} {}", idx, TABLE[idx - 1])
}

/// Core and valence electron counts for a type, from the noble-gas core below
/// it: Cr 18/6, Fe 18/8, Al 10/3. SPR-KKR treats the rest as frozen core. This
/// is a convention, not a calculation: for elements with shallow semicore
/// states (Ga 3d, Sn 4d, rare-earth 4f) check TYPES against your intent.
fn core_valence(z: i32) -> (i32, i32) {
    const NOBLE: [i32; 6] = [2, 10, 18, 36, 54, 86];
    if z <= 0 {
        return (0, 0);
    }
    let core = NOBLE.iter().rev().copied().find(|&n| n < z).unwrap_or(0);
    (core, z - core)
}

/// A concentration as short as it can be written exactly enough: 0.3, 0.333333.
fn conc(c: f64) -> String {
    let t = format!("{:.6}", c);
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

const RULE: &str =
    "*******************************************************************************";

/// One occupant of a site, with its concentration normalised to the site.
struct Occupant {
    element: String,
    z: i32,
    conc: f64,
}

/// One site: where it is (Cartesian, Å, in the standard setting) and who is on it.
struct SiteData {
    position: [f64; 3],
    occupants: Vec<Occupant>,
}

/// The structure as SPR-KKR sees it: the cell in the standard setting, and the
/// sites with normalised occupants. Shared by the built-in writer and the
/// ase2sprkkr hand-off, so both write the same sites.
struct SiteModel {
    lattice: [[f64; 3]; 3],
    sites: Vec<SiteData>,
    warnings: Vec<String>,
}

fn build_model(structure: &Structure) -> SiteModel {
    let mut warnings: Vec<String> = Vec::new();
    let groups = group_sites(structure);
    let lattice = standard_lattice(structure.lattice);

    // Positions rotated into the standard setting: fractional, then back
    // through the standard cell.
    let position = |i: usize| -> [f64; 3] {
        let frac = crate::utils::linalg::cart_to_frac(structure.atoms[i].position, structure.lattice)
            .unwrap_or([0.0; 3]);
        [
            frac[0] * lattice[0][0] + frac[1] * lattice[1][0] + frac[2] * lattice[2][0],
            frac[0] * lattice[0][1] + frac[1] * lattice[1][1] + frac[2] * lattice[2][1],
            frac[0] * lattice[0][2] + frac[1] * lattice[1][2] + frac[2] * lattice[2][2],
        ]
    };

    let mut sites = Vec::new();
    for (q, members) in groups.iter().enumerate() {
        let total: f64 = members
            .iter()
            .map(|&i| structure.atoms[i].occupancy.clamp(0.0, 1.0))
            .sum();
        if (total - 1.0).abs() > 1e-3 {
            warnings.push(format!(
                "site {} has total occupancy {:.3}; SPR-KKR concentrations must sum to 1, so they were \
                 normalised (vacancies are not written)",
                q + 1,
                total
            ));
        }
        let denom = if total > 1e-9 { total } else { 1.0 };
        let occupants = members
            .iter()
            .map(|&i| {
                let a = &structure.atoms[i];
                Occupant {
                    element: a.element.clone(),
                    z: get_atomic_number(&a.element),
                    conc: a.occupancy.clamp(0.0, 1.0) / denom,
                }
            })
            .collect();
        sites.push(SiteData {
            position: position(members[0]),
            occupants,
        });
    }
    SiteModel { lattice, sites, warnings }
}

/// The potential file as text, and any warnings about what could not be
/// represented. Separate from `write` so it can be tested without a file.
fn render(structure: &Structure) -> (String, Vec<String>) {
    let model = build_model(structure);
    let warnings = model.warnings.clone();
    let n_sites = model.sites.len();

    // --- lattice in the standard setting, in units of ALAT ---
    let lat = model.lattice;
    let alat_ang = (lat[0][0] * lat[0][0] + lat[0][1] * lat[0][1] + lat[0][2] * lat[0][2]).sqrt();
    let alat_bohr = alat_ang / BOHR_TO_ANG;
    let scaled = |v: [f64; 3]| [v[0] / alat_ang, v[1] / alat_ang, v[2] / alat_ang];

    // --- types: one per (site, species), numbered in site order ---
    let mut type_ids: Vec<Vec<usize>> = Vec::new();
    let mut next_type = 1usize;
    for site in &model.sites {
        let ids: Vec<usize> = site.occupants.iter().map(|_| { let t = next_type; next_type += 1; t }).collect();
        type_ids.push(ids);
    }
    let n_types = next_type - 1;

    let mut o = String::new();
    let mut line = |s: String| {
        o.push_str(&s);
        o.push('\n');
    };
    // `KEY` padded to 12 columns, a tab, then the value: the layout of the
    // files ase2sprkkr writes and SPR-KKR reads.
    let kv = |k: &str, v: String| format!("{:<12}\t{}", k, v);

    line(RULE.into());
    line(kv("HEADER", "SPR-KKR potential file, created by CView".into()));
    line(RULE.into());
    line(kv("TITLE", "Created by CView".into()));
    line(kv("SYSTEM", format!("System: {}", crate::physics::operations::conversion::build_formula(&structure.atoms))));
    line(kv("PACKAGE", "SPR-KKR".into()));
    line(kv("FORMAT", " 7 (21.05.2007)".into()));
    line(RULE.into());
    line("GLOBAL SYSTEM PARAMETER".into());
    line(kv("NQ", n_sites.to_string()));
    line(kv("NT", n_types.to_string()));
    line(kv("NM", n_sites.to_string()));
    line(kv("IREL", "3".into()));
    line(RULE.into());
    line("SCF-INFO".into());
    for (k, v) in [
        ("INFO", "NONE"),
        ("SCFSTATUS", "START"),
        ("FULLPOT", "F"),
        ("BREITINT", "F"),
        ("NONMAG", "F"),
        ("ORBPOL", "NONE"),
        ("EXTFIELD", "F"),
        ("BLCOUPL", "F"),
        ("BEXT", "0.0"),
        ("SEMICORE", "F"),
        ("LLOYD", "F"),
        ("SCF-ITER", "0"),
        ("SCF-MIX", "0.2"),
        ("SCF-TOL", "1e-05"),
        ("RMSAVV", "999999.0"),
        ("RMSAVB", "999999.0"),
        ("EF", "999999.0"),
        ("VMTZ", "0.7"),
    ] {
        line(kv(k, v.into()));
    }
    line(RULE.into());
    line("LATTICE".into());
    line(kv("SYSDIM", "3D".into()));
    line(kv("SYSTYPE", "BULK".into()));
    line(kv("BRAVAIS", bravais_entry(structure.lattice)));
    line(kv("ALAT", format!("{}", alat_bohr)));
    for (i, v) in lat.iter().enumerate() {
        let u = scaled(*v);
        line(format!("A({}){:>29.14}{:>23.14}{:>23.14}", i + 1, u[0], u[1], u[2]));
    }
    line(RULE.into());
    line("SITES".into());
    line(kv("CARTESIAN", "T".into()));
    line(kv("BASSCALE", "1.0 1.0 1.0".into()));
    line("   IQ                QBAS(X)                QBAS(Y)                QBAS(Z)".into());
    for (q, site) in model.sites.iter().enumerate() {
        let u = scaled(site.position);
        line(format!("{:>5}{:>23.14}{:>23.14}{:>23.14}", q + 1, u[0], u[1], u[2]));
    }
    line(RULE.into());
    line("OCCUPATION".into());
    line("IQ              IREFQ              IMQ              NOQ        ITOQ CONC".into());
    for (q, site) in model.sites.iter().enumerate() {
        let list: String = site
            .occupants
            .iter()
            .zip(&type_ids[q])
            .map(|(oc, id)| format!("  {} {}", id, conc(oc.conc)))
            .collect();
        line(format!(
            "{:<4}{:>16}{:>17}{:>17}{}",
            q + 1,
            q + 1,
            q + 1,
            site.occupants.len(),
            list
        ));
    }
    line(RULE.into());
    line("REFERENCE SYSTEM".into());
    line(kv("NREF", n_sites.to_string()));
    line("IREF                   VREF                 RMTREF".into());
    for q in 1..=n_sites {
        line(format!("{:<4}{:>20}{:>24}", q, "4.0", "0.0"));
    }
    line(RULE.into());
    line("MAGNETISATION DIRECTION".into());
    line(kv("KMROT", "0".into()));
    line(kv("QMVEC", "0.0 0.0 0.0".into()));
    line("IQ                   MTET_Q                 MPHI_Q".into());
    for q in 1..=n_sites {
        line(format!("{:<4}{:>20}{:>24}", q, "0.0", "0.0"));
    }
    line(RULE.into());
    line("MESH INFORMATION".into());
    line(kv("MESH-TYPE", "EXPONENTIAL".into()));
    line("   IM             R(1)               DX             JRMT              RMT             JRWS              RWS".into());
    for m in 1..=n_sites {
        line(format!(
            "{:>5}{:>17}{:>17}{:>17}{:>17}{:>17}{:>17}",
            m, "1e-06", "0.02", "0", "0.0", "721", "0.0"
        ));
    }
    line(RULE.into());
    line("TYPES".into());
    line("IT                TXT               ZT            NCORT            NVALT      NSEMCORSHLT".into());
    for (site, ids) in model.sites.iter().zip(&type_ids) {
        for (oc, id) in site.occupants.iter().zip(ids) {
            let (core, val) = core_valence(oc.z);
            line(format!(
                "{:<4}{:>20}{:>17}{:>17}{:>17}{:>17}",
                id, oc.element, oc.z, core, val, 0
            ));
        }
    }
    drop(line);
    (o, warnings)
}

/// The helper that asks ase2sprkkr to write the file (see the script for the
/// request format).
const ASE2SPRKKR_HELPER: &str = include_str!("sprkkr_ase2sprkkr.py");

/// Which writer produced a file.
#[derive(Debug, PartialEq)]
enum Writer {
    /// ase2sprkkr, with its version.
    Ase2Sprkkr(String),
    /// CView's own writer; `why` says why ase2sprkkr was not used.
    BuiltIn { why: String },
}

/// Pythons to try, best first. `CVIEW_PYTHON` names the interpreter that has
/// ase2sprkkr (a virtualenv or conda environment); without it, `python3`.
fn python_candidates() -> Vec<String> {
    match std::env::var("CVIEW_PYTHON") {
        Ok(p) if !p.trim().is_empty() => vec![p],
        _ => vec!["python3".to_string(), "python".to_string()],
    }
}

/// The request the helper reads: the model's cell and sites.
fn helper_request(path: &str, model: &SiteModel) -> serde_json::Value {
    serde_json::json!({
        "path": path,
        "lattice": model.lattice,
        "sites": model.sites.iter().map(|s| serde_json::json!({
            "position": s.position,
            "occupants": s.occupants.iter().map(|o| serde_json::json!({
                "element": o.element,
                "conc": o.conc,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Try ase2sprkkr through each interpreter. `Ok(Some(version))` when it wrote
/// the file; `Ok(None)` when it is not installed for any of them; `Err` when it
/// is installed but failed (its message).
fn try_ase2sprkkr(
    path: &str,
    model: &SiteModel,
    pythons: &[String],
) -> Result<Option<String>, String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    // An absolute output path: the helper's working directory is not CView's.
    let abs = std::path::absolute(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string());
    let request = helper_request(&abs, model).to_string();

    for py in pythons {
        let child = Command::new(py)
            .args(["-c", ASE2SPRKKR_HELPER])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => continue, // no such interpreter
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(request.as_bytes());
        }
        let out = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => return Err(format!("could not run {py}: {e}")),
        };
        match out.status.code() {
            Some(0) => {
                let v = serde_json::from_slice::<serde_json::Value>(&out.stdout)
                    .ok()
                    .and_then(|j| j["version"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "unknown".to_string());
                return Ok(Some(v));
            }
            Some(3) => continue, // not importable with this interpreter
            _ => {
                let err = String::from_utf8_lossy(&out.stderr);
                let tail: Vec<&str> = err.lines().rev().take(3).collect();
                return Err(tail.into_iter().rev().collect::<Vec<_>>().join(" | "));
            }
        }
    }
    Ok(None)
}

/// Write the potential: with ase2sprkkr when an interpreter that has it can be
/// found (it is the reference writer, and tracks the SPR-KKR file format), else
/// with CView's own. Returns which one was used. `pythons` is explicit so tests
/// do not depend on the environment.
fn write_with(path: &str, structure: &Structure, pythons: &[String]) -> io::Result<Writer> {
    let model = build_model(structure);
    for w in &model.warnings {
        crate::utils::console::log_warn(&format!("SPR-KKR export: {w}"));
    }

    let why = match try_ase2sprkkr(path, &model, pythons) {
        Ok(Some(version)) => return Ok(Writer::Ase2Sprkkr(version)),
        Ok(None) => "ase2sprkkr was not found".to_string(),
        Err(e) => format!("ase2sprkkr failed: {e}"),
    };

    let (text, _) = render(structure);
    std::fs::write(path, text)?;
    Ok(Writer::BuiltIn { why })
}

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mixed = group_sites(structure).iter().filter(|s| s.len() > 1).count();
    match write_with(path, structure, &python_candidates())? {
        Writer::Ase2Sprkkr(v) => crate::utils::console::log_info(&format!(
            "SPR-KKR export: written by ase2sprkkr {v}{}",
            if mixed > 0 { format!(", {mixed} mixed site(s) as CPA occupations") } else { String::new() }
        )),
        Writer::BuiltIn { why } => crate::utils::console::log_warn(&format!(
            "SPR-KKR export: {why}; wrote CView's built-in potential (FORMAT 7, checked against an \
             ase2sprkkr file but not run in SPR-KKR). Install ase2sprkkr (pip install ase2sprkkr), or \
             set CVIEW_PYTHON to the Python that has it, to use the reference writer."
        )),
    }
    Ok(())
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Extract first number from a line
fn extract_first_number(line: &str) -> Option<f64> {
    // '+' belongs to a number too: SPR-KKR writes `9.85370239180532E+00`. Left
    // out, the token splits at the sign, "9.85...E" fails to parse, and the
    // next token ("00") reads as 0, giving a zero lattice for every potential
    // written by an SCF run.
    line.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .filter_map(|s| s.parse::<f64>().ok())
        .next()
}

/// Parse a 3D vector from a line (flexible format)
/// Handles: "A(1) = 1.0 2.0 3.0", "BASSCALE 1.0 2.0 3.0", etc.
fn parse_vec3_flexible(line: &str) -> Option<[f64; 3]> {
    let parts: Vec<f64> = line
        .replace("=", " ")
        .replace("(", " ")
        .replace(")", " ")
        .split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    if parts.len() >= 3 {
        // Take last 3 numbers (handles "A(1) = 1.0 2.0 3.0" format)
        let n = parts.len();
        Some([parts[n - 3], parts[n - 2], parts[n - 1]])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vec3() {
        assert_eq!(
            parse_vec3_flexible("A(1) = 5.0 0.0 0.0"),
            Some([5.0, 0.0, 0.0])
        );
        assert_eq!(
            parse_vec3_flexible("BASSCALE 1.0 1.0 1.0"),
            Some([1.0, 1.0, 1.0])
        );
    }

    #[test]
    fn test_extract_number() {
        assert_eq!(extract_first_number("ALAT = 5.42"), Some(5.42));
        assert_eq!(extract_first_number("ALAT 9.44"), Some(9.44));
    }
}


#[cfg(test)]
mod cpa_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// An SPR-KKR potential produced by ase2sprkkr for a Cr/Fe/Al alloy on an
    /// fcc lattice with four sites: the reference for how a disordered system
    /// is stored (OCCUPATION lists NOQ occupants per site, each its own TYPE).
    const FEALFE2_POT: &str = r#"*******************************************************************************
HEADER      	SPR-KKR potential file, created at 2025-09-15 22:16:43.058979
*******************************************************************************
TITLE       	Created by ASE-SPR-KKR wrapper
SYSTEM      	System: FeAlFe2
PACKAGE     	SPR-KKR
FORMAT      	 7 (21.05.2007)
*******************************************************************************
GLOBAL SYSTEM PARAMETER
NQ          	4
NT          	12
NM          	4
IREL        	3
*******************************************************************************
SCF-INFO
INFO        	NONE
SCFSTATUS   	START
FULLPOT     	F
BREITINT    	F
NONMAG      	F
ORBPOL      	NONE
EXTFIELD    	F
BLCOUPL     	F
BEXT        	0.0
SEMICORE    	F
LLOYD       	F
SCF-ITER    	0
SCF-MIX     	0.2
SCF-TOL     	1e-05
RMSAVV      	999999.0
RMSAVB      	999999.0
EF          	999999.0
VMTZ        	0.7
*******************************************************************************
LATTICE
SYSDIM      	3D
SYSTYPE     	BULK
BRAVAIS     	 13 cubic face-centered m3m O_h
ALAT        	9.85370239180532
A(1)             0.70710678118655       0.00000000000000       0.00000000000000
A(2)             0.35355343334493       0.61237241101311       0.00000000000000
A(3)             0.35355343334493       0.20412415345949       0.57735024010080
*******************************************************************************
SITES
CARTESIAN   	T
BASSCALE    	1.0 1.0 1.0
   IQ                QBAS(X)                QBAS(Y)                QBAS(Z)
    1       0.00000000000000       0.00000000000000       0.00000000000000
    2       0.70710682393821       0.40824828223630       0.28867512005040
    3       0.35355341196910       0.20412414111815       0.14433756002520
    4       1.06066023590731       0.61237242335445       0.43301268007560
*******************************************************************************
OCCUPATION
IQ              IREFQ              IMQ              NOQ        ITOQ CONC
1                   1                1                3  1 0.3  2 0.4  3 0.3
2                   2                2                3  4 0.3  5 0.3  6 0.4
3                   3                3                3  7 0.2  8 0.6  9 0.2
4                   4                4                3  10 0.2  11 0.6  12 0.2
*******************************************************************************
REFERENCE SYSTEM
NREF        	4
IREF                   VREF                 RMTREF
1                       4.0                    0.0
2                       4.0                    0.0
3                       4.0                    0.0
4                       4.0                    0.0
*******************************************************************************
MAGNETISATION DIRECTION
KMROT       	0
QMVEC       	0.0 0.0 0.0
IQ                   MTET_Q                 MPHI_Q
1                       0.0                    0.0
2                       0.0                    0.0
3                       0.0                    0.0
4                       0.0                    0.0
*******************************************************************************
MESH INFORMATION
MESH-TYPE   	EXPONENTIAL
   IM             R(1)               DX             JRMT              RMT             JRWS              RWS
    1            1e-06             0.02                0              0.0              721              0.0
    2            1e-06             0.02                0              0.0              721              0.0
    3            1e-06             0.02                0              0.0              721              0.0
    4            1e-06             0.02                0              0.0              721              0.0
*******************************************************************************
TYPES
IT                TXT               ZT            NCORT            NVALT      NSEMCORSHLT
1                  Cr               24               18                6                0
2                  Fe               26               18                8                0
3                  Al               13               10                3                0
4                  Cr               24               18                6                0
5                  Fe               26               18                8                0
6                  Al               13               10                3                0
7                  Cr               24               18                6                0
8                  Fe               26               18                8                0
9                  Al               13               10                3                0
10                 Cr               24               18                6                0
11                 Fe               26               18                8                0
12                 Al               13               10                3                0
"#;

    /// An SCF-result header (FORMAT 9): numbers in E+00 notation.
    const SCF_RESULT_HEAD: &str = "\
HEADER    'SPR-KKR dataset created by KKRSCF    '
LATTICE   
SYSDIM    '3D        '
BRAVAIS           13     cubic       face-centered  m3m    O_h 
ALAT        9.85370239180532E+00
A(1)        7.07106781186550E-01  0.00000000000000E+00  0.00000000000000E+00
A(2)        3.53553433344930E-01  6.12372411013110E-01  0.00000000000000E+00
A(3)        3.53553433344930E-01  2.04124153459490E-01  5.77350240100800E-01
*******************************************************************************
SITES     
CARTESIAN T
BASSCALE    1.00000000000000E+00  1.00000000000000E+00  1.00000000000000E+00
        IQ        QBAS(X)               QBAS(Y)               QBAS(Z)
         1  0.00000000000000E+00  0.00000000000000E+00  0.00000000000000E+00
         2  3.53553390593280E-01  2.04124128776810E-01 -2.88675120050400E-01
*******************************************************************************
OCCUPATION
        IQ     IREFQ       IMQ       NOQ  ITOQ  CONC
         1         1         1         2     1 0.50000     2 0.50000
         2         2         2         1     3 1.00000
*******************************************************************************
TYPES
   IT     TXT_T            ZT     NCORT     NVALT    NSEMCORSHLT
    1     Fe_1            26        18         8            0
    2     Cr_1            24        18         6            0
    3     Al_2            13        10         3            0
*******************************************************************************
POTENTIAL
";

    fn tmp(name: &str, text: &str) -> String {
        // Unique per call: tests run in parallel, and several write the same
        // fixture name (one truncating it while another reads it failed
        // intermittently).
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("cview_sprkkr_{}_{n}_{name}", std::process::id()));
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    }

    /// site index -> sorted (element, concentration*1000) from a parsed file.
    fn sites_of(s: &Structure) -> BTreeMap<usize, Vec<(String, i64)>> {
        let mut out: BTreeMap<usize, Vec<(String, i64)>> = BTreeMap::new();
        for (q, members) in group_sites(s).iter().enumerate() {
            let mut v: Vec<(String, i64)> = members
                .iter()
                .map(|&i| (s.atoms[i].element.clone(), (s.atoms[i].occupancy * 1000.0).round() as i64))
                .collect();
            v.sort();
            out.insert(q, v);
        }
        out
    }

    /// Lengths and angles of the cell: unchanged by the rotation to the standard setting.
    fn metric(l: [[f64; 3]; 3]) -> [f64; 6] {
        let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let n = |u: [f64; 3]| dot(u, u).sqrt();
        [
            n(l[0]),
            n(l[1]),
            n(l[2]),
            dot(l[1], l[2]) / (n(l[1]) * n(l[2])),
            dot(l[0], l[2]) / (n(l[0]) * n(l[2])),
            dot(l[0], l[1]) / (n(l[0]) * n(l[1])),
        ]
    }

    #[test]
    fn exponent_notation_in_alat_is_read() {
        assert_eq!(extract_first_number("ALAT        9.85370239180532E+00"), Some(9.85370239180532));
        assert_eq!(extract_first_number("ALAT 1.5E-01"), Some(0.15));
    }

    #[test]
    fn an_scf_result_potential_reads_with_its_real_lattice() {
        // Before the fix ALAT read as 0 and the lattice and every position were zero.
        let s = parse(&tmp("scf.pot_new", SCF_RESULT_HEAD)).unwrap();
        let a = 9.85370239180532 * BOHR_TO_ANG;
        assert!((s.lattice[0][0] - 0.70710678118655 * a).abs() < 1e-9, "{:?}", s.lattice);
        assert_eq!(s.atoms.len(), 3, "site 1 mixed (2) + site 2 pure (1)");
        // Concentrations survive and types such as "Fe_1" read as Fe.
        assert_eq!(
            sites_of(&s),
            BTreeMap::from([
                (0, vec![("Cr".to_string(), 500), ("Fe".to_string(), 500)]),
                (1, vec![("Al".to_string(), 1000)]),
            ])
        );
    }

    #[test]
    fn noble_gas_core_counts_match_the_reference_types() {
        assert_eq!(core_valence(24), (18, 6), "Cr");
        assert_eq!(core_valence(26), (18, 8), "Fe");
        assert_eq!(core_valence(13), (10, 3), "Al");
        assert_eq!(core_valence(0), (0, 0));
    }

    #[test]
    fn writing_the_reference_alloy_reproduces_its_occupation_and_types() {
        let s = parse(&tmp("ref.pot", FEALFE2_POT)).unwrap();
        let (text, warnings) = render(&s);
        assert!(warnings.is_empty(), "{warnings:?}");

        // Normalise whitespace so only the content is compared.
        let norm = |t: &str| -> Vec<String> {
            t.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect()
        };
        let (got, want) = (norm(&text), norm(FEALFE2_POT));

        // Scalar sections the reference sets and we must match exactly.
        for key in ["NQ 4", "NT 12", "NM 4", "IREL 3", "NREF 4", "KMROT 0", "MESH-TYPE EXPONENTIAL"] {
            assert!(got.iter().any(|l| l == key), "missing {key:?}");
            assert!(want.iter().any(|l| l == key), "reference lacks {key:?}");
        }
        // BRAVAIS: the reference line, with the lattice detected from the cell.
        assert!(got.iter().any(|l| l == "BRAVAIS 13 cubic face-centered m3m O_h"), "{:?}",
            got.iter().find(|l| l.starts_with("BRAVAIS")));
        // Mesh rows and reference-system rows are identical per site.
        for l in want.iter().filter(|l| l.starts_with("1 1e-06 0.02 0 0.0 721 0.0")
            || l.starts_with("4 4.0 0.0") || l.starts_with("4 0.0 0.0")) {
            assert!(got.contains(l), "missing {l:?}");
        }
        // TYPES: same (element, Z, NCORT, NVALT) multiset, 12 types.
        let types = |v: &[String]| -> Vec<String> {
            let i = v.iter().position(|l| l == "TYPES").unwrap();
            let mut t: Vec<String> = v[i + 2..]
                .iter()
                .filter(|l| !l.starts_with('*') && !l.is_empty())
                .map(|l| l.split(' ').skip(1).collect::<Vec<_>>().join(" "))
                .collect();
            t.sort();
            t
        };
        assert_eq!(types(&got), types(&want));
        // OCCUPATION: three occupants per site, concentrations as a multiset per site.
        let occ = |v: &[String]| -> Vec<(String, usize, Vec<String>)> {
            let i = v.iter().position(|l| l == "OCCUPATION").unwrap();
            v[i + 2..i + 6]
                .iter()
                .map(|l| {
                    let p: Vec<&str> = l.split(' ').collect();
                    let mut c: Vec<String> = p[4..].chunks(2).map(|x| x[1].to_string()).collect();
                    c.sort();
                    (p[0].to_string(), p[3].parse().unwrap(), c)
                })
                .collect()
        };
        assert_eq!(occ(&got), occ(&want));
    }

    #[test]
    fn reference_sections_match_line_for_line() {
        // Everything that does not depend on the author's choice of ALAT, the
        // header text, or the numerical noise in the positions.
        let s = parse(&tmp("ref3.pot", FEALFE2_POT)).unwrap();
        let (text, _) = render(&s);
        let norm = |t: &str| -> Vec<String> {
            t.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect()
        };
        let (got, want) = (norm(&text), norm(FEALFE2_POT));
        let section = |v: &[String], name: &str| -> Vec<String> {
            let i = v.iter().position(|l| l == name).unwrap_or_else(|| panic!("no {name}"));
            v[i..].iter().skip(1).take_while(|l| !l.starts_with('*')).cloned().collect()
        };
        for name in [
            "GLOBAL SYSTEM PARAMETER",
            "SCF-INFO",
            "OCCUPATION",
            "REFERENCE SYSTEM",
            "MAGNETISATION DIRECTION",
            "MESH INFORMATION",
            "TYPES",
        ] {
            assert_eq!(section(&got, name), section(&want, name), "section {name}");
        }
        let line = |v: &[String], key: &str| v.iter().find(|l| l.starts_with(key)).cloned();
        assert_eq!(line(&got, "BRAVAIS"), line(&want, "BRAVAIS"));
        assert_eq!(line(&got, "SYSDIM"), line(&want, "SYSDIM"));
    }

    #[test]
    fn a_written_alloy_reads_back_as_the_same_structure() {
        let s = parse(&tmp("ref2.pot", FEALFE2_POT)).unwrap();
        let (text, _) = render(&s);
        let back = parse(&tmp("round.pot", &text)).unwrap();

        // Same sites, species and concentrations.
        assert_eq!(sites_of(&back), sites_of(&s));
        // Same cell (up to the rotation), and the same site positions in it.
        for (a, b) in metric(back.lattice).iter().zip(metric(s.lattice)) {
            assert!((a - b).abs() < 1e-7, "{:?} vs {:?}", metric(back.lattice), metric(s.lattice));
        }
        let frac = |st: &Structure| -> Vec<[f64; 3]> {
            group_sites(st)
                .iter()
                .map(|m| {
                    let f = crate::utils::linalg::cart_to_frac(st.atoms[m[0]].position, st.lattice).unwrap();
                    [f[0].rem_euclid(1.0), f[1].rem_euclid(1.0), f[2].rem_euclid(1.0)]
                })
                .collect()
        };
        for (a, b) in frac(&back).iter().zip(frac(&s)) {
            for k in 0..3 {
                let d = (a[k] - b[k]).abs();
                assert!(d.min(1.0 - d) < 1e-6, "{:?} vs {:?}", a, b);
            }
        }
    }

    fn atom(el: &str, pos: [f64; 3], occ: f64) -> Atom {
        Atom {
            element: el.to_string(),
            position: pos,
            original_index: 0,
            oxidation: None,
            occupancy: occ,
        }
    }

    fn cubic(atoms: Vec<Atom>) -> Structure {
        Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms,
            formula: String::new(),
            is_periodic: true,
        }
    }

    #[test]
    fn a_mixed_site_becomes_one_site_with_two_occupants() {
        // O 0.67 / N 0.33 at one position, plus an ordered Ba: 2 sites, 3 types.
        let s = cubic(vec![
            atom("Ba", [0.0; 3], 1.0),
            atom("O", [2.0, 0.0, 0.0], 0.67),
            atom("N", [2.0, 0.0, 0.0], 0.33),
        ]);
        let (text, warnings) = render(&s);
        assert!(warnings.is_empty(), "{warnings:?}");
        let flat: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        assert!(flat.contains(&"NQ 2".to_string()));
        assert!(flat.contains(&"NT 3".to_string()), "one type per (site, species)");
        assert!(flat.contains(&"1 1 1 1 1 1".to_string()), "pure Ba site: NOQ 1, type 1, conc 1");
        assert!(flat.contains(&"2 2 2 2 2 0.67 3 0.33".to_string()), "{:?}",
            flat.iter().find(|l| l.starts_with("2 2 2")));
    }

    #[test]
    fn two_full_atoms_at_one_position_stay_two_sites() {
        // An overlap, not a mixture: only partial occupancies merge.
        let s = cubic(vec![atom("Fe", [1.0; 3], 1.0), atom("Cr", [1.0; 3], 1.0)]);
        assert_eq!(group_sites(&s).len(), 2);
    }

    #[test]
    fn concentrations_not_summing_to_one_are_normalised_with_a_warning() {
        let s = cubic(vec![atom("Fe", [1.0; 3], 0.5), atom("Cr", [1.0; 3], 0.3)]);
        let (text, warnings) = render(&s);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("0.800") && warnings[0].contains("normalised"), "{warnings:?}");
        let line = text.lines().find(|l| l.starts_with("1 ")).unwrap();
        let conc: Vec<f64> = line.split_whitespace().skip(5).step_by(2).map(|c| c.parse().unwrap()).collect();
        assert!((conc.iter().sum::<f64>() - 1.0).abs() < 1e-5, "{line}");
    }

    #[test]
    fn bravais_entry_follows_the_cell() {
        let sc = [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]];
        let a = 4.0 / 2.0;
        let fcc = [[0.0, a, a], [a, 0.0, a], [a, a, 0.0]];
        let hex = [[3.0, 0.0, 0.0], [-1.5, 2.598076211, 0.0], [0.0, 0.0, 5.0]];
        assert!(bravais_entry(sc).contains("12 cubic       primitive"), "{}", bravais_entry(sc));
        assert!(bravais_entry(fcc).contains("13 cubic       face-centered"), "{}", bravais_entry(fcc));
        assert!(bravais_entry(hex).contains("11 hexagonal"), "{}", bravais_entry(hex));
    }

    #[test]
    fn standard_lattice_keeps_every_length_and_angle() {
        let tilted = [[0.0, 2.0, 2.0], [2.0, 0.0, 2.0], [2.0, 2.0, 0.0]];
        let std = standard_lattice(tilted);
        assert!(std[0][1].abs() < 1e-12 && std[0][2].abs() < 1e-12 && std[1][2].abs() < 1e-12);
        for (a, b) in metric(std).iter().zip(metric(tilted)) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    // ---- delegation to ase2sprkkr ----

    fn alloy_from_reference() -> Structure {
        parse(&tmp("deleg_ref.pot", FEALFE2_POT)).unwrap()
    }

    #[test]
    fn the_helper_request_carries_sites_and_occupants_in_order() {
        let s = alloy_from_reference();
        let model = build_model(&s);
        let req = helper_request("/x/out.pot", &model);
        assert_eq!(req["path"], "/x/out.pot");
        let sites = req["sites"].as_array().unwrap();
        assert_eq!(sites.len(), 4);
        let occ = sites[0]["occupants"].as_array().unwrap();
        assert_eq!(occ.len(), 3);
        // Occupants keep the file's order (Cr, Fe, Al), concentrations sum to 1.
        let el: Vec<&str> = occ.iter().map(|o| o["element"].as_str().unwrap()).collect();
        assert_eq!(el, ["Cr", "Fe", "Al"]);
        let sum: f64 = occ.iter().map(|o| o["conc"].as_f64().unwrap()).sum();
        assert!((sum - 1.0).abs() < 1e-9);
        assert_eq!(req["lattice"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn without_ase2sprkkr_the_built_in_writer_is_used_and_says_why() {
        let s = alloy_from_reference();
        let out = tmp("fallback.pot", "");
        let w = write_with(&out, &s, &["/nonexistent/python-for-cview-test".to_string()]).unwrap();
        assert!(matches!(&w, Writer::BuiltIn { why } if why.contains("not found")), "{w:?}");
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("FORMAT") && text.contains(" 7 (21.05.2007)"), "built-in is FORMAT 7");
        let back = parse(&out).unwrap();
        assert_eq!(sites_of(&back), sites_of(&s));
    }

    /// Needs a Python with ase2sprkkr (`python3` here, or CVIEW_PYTHON); skipped
    /// where there is none, so the suite does not depend on it.
    #[test]
    fn ase2sprkkr_writes_the_alloy_when_it_is_installed() {
        let s = alloy_from_reference();
        let out = tmp("ase2sprkkr.pot", "");
        let pythons = python_candidates();
        let w = write_with(&out, &s, &pythons).unwrap();
        let version = match w {
            Writer::Ase2Sprkkr(v) => v,
            Writer::BuiltIn { why } => {
                eprintln!("skipped: {why}");
                return;
            }
        };
        let text = std::fs::read_to_string(&out).unwrap();
        let flat: Vec<String> =
            text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        // Its own layout, not ours: the version it reports wrote this file.
        assert!(flat.iter().any(|l| l.starts_with("PACKAGE SPR-KKR")), "version {version}");
        // One site per CView site, every (site, element) its own type, CPA rows.
        for key in ["NQ 4", "NT 12", "NM 4"] {
            assert!(flat.contains(&key.to_string()), "missing {key} in\n{text}");
        }
        assert!(flat.iter().any(|l| l.starts_with("1 1 1 3 1 0.3 2 0.4 3 0.3")), "{text}");
        // And CView reads it back as the same alloy.
        let back = parse(&out).unwrap();
        assert_eq!(sites_of(&back), sites_of(&s));
    }
}
