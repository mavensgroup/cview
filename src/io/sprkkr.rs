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
// POTENTIAL  - Optional: DFT potential data (not parsed/written here)
//
// Key Features Supported:
// - Chemical disorder (e.g., Fe₀.₅Co₀.₅ alloys)
// - Both Cartesian and Direct (fractional) coordinates
// - ALAT scaling (auto-detect Bohr vs Angstrom)
// - BASSCALE for anisotropic position scaling
// - Multiple occupation at single site

use crate::model::elements::get_atomic_number;
use crate::model::{Atom, Structure};
use crate::utils::linalg::{cart_to_frac, frac_to_cart};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, Write};
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
            current_section = Section::Potential;
            continue;
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
            && line.chars().nth(1).map_or(false, |c| c.is_numeric()))
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

    for site_id in sorted_ids {
        if let Some(site_pos) = data.site_positions.get(&site_id) {
            // Get occupation for this site
            let occupations = data.site_occupation.get(&site_id);

            // Find the type with highest concentration (for pure sites, there's only one)
            let type_id = if let Some(occs) = occupations {
                occs.iter()
                    .max_by(|a, b| a.concentration.partial_cmp(&b.concentration).unwrap())
                    .map(|o| o.type_id)
                    .unwrap_or(1)
            } else {
                1 // Default to type 1
            };

            // Get element symbol
            let element = data
                .type_data
                .get(&type_id)
                .map(|(el, _)| el.clone())
                .unwrap_or_else(|| String::from("X"));

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

            atoms.push(Atom {
                element,
                position,
                original_index: atoms.len(),
            });
        }
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
    })
}

// ============================================================================
// WRITER
// ============================================================================

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = File::create(path)?;

    // Prepare data
    let mut unique_elements: Vec<String> = Vec::new();
    let mut atom_to_type_id: Vec<usize> = Vec::new();

    for atom in &structure.atoms {
        if let Some(idx) = unique_elements.iter().position(|e| e == &atom.element) {
            atom_to_type_id.push(idx + 1);
        } else {
            unique_elements.push(atom.element.clone());
            atom_to_type_id.push(unique_elements.len());
        }
    }

    // ========================================================================
    // HEADER
    // ========================================================================
    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "HEADER    'Exported by CView'")?;
    writeln!(file, "SYSTEM    {}", structure.formula)?;
    writeln!(
        file,
        "*******************************************************************************"
    )?;

    // ========================================================================
    // LATTICE
    // ========================================================================
    writeln!(file, "LATTICE")?;
    writeln!(file, "SYSDIM       3D")?;
    writeln!(file, "SYSTYPE      BULK")?;

    // Use ALAT = 1.0 and put actual values in lattice vectors
    // This is cleaner and avoids unit ambiguity
    writeln!(file, "ALAT          1.00000000")?;

    for (i, vec) in structure.lattice.iter().enumerate() {
        writeln!(
            file,
            "A({})          {:18.10} {:18.10} {:18.10}",
            i + 1,
            vec[0],
            vec[1],
            vec[2]
        )?;
    }

    // ========================================================================
    // SITES
    // ========================================================================
    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "SITES")?;
    writeln!(file, "CARTESIAN T")?;
    writeln!(
        file,
        "BASSCALE      1.000000000000000    1.000000000000000    1.000000000000000"
    )?;
    writeln!(
        file,
        "   IQ       QX                   QY                   QZ"
    )?;

    for (i, atom) in structure.atoms.iter().enumerate() {
        writeln!(
            file,
            " {:4}    {:18.10} {:18.10} {:18.10}",
            i + 1,
            atom.position[0],
            atom.position[1],
            atom.position[2]
        )?;
    }

    // ========================================================================
    // OCCUPATION
    // ========================================================================
    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "OCCUPATION")?;
    writeln!(file, "   IQ     IREFQ       IMQ       NOQ  ITOQ  CONC")?;

    for (i, type_id) in atom_to_type_id.iter().enumerate() {
        writeln!(
            file,
            " {:4} {:9} {:9} {:9} {:5}   1.00000",
            i + 1,   // IQ: site number
            i + 1,   // IREFQ: reference site (self)
            i + 1,   // IMQ: magnetic site index
            1,       // NOQ: number of occupations (pure site = 1)
            type_id  // ITOQ: type ID
        )?;
    }

    // ========================================================================
    // TYPES
    // ========================================================================
    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "TYPES")?;
    writeln!(
        file,
        "   IT     TXTT            ZT      NC      LC      KC      VC"
    )?;

    for (i, element) in unique_elements.iter().enumerate() {
        let atomic_number = get_atomic_number(element);
        writeln!(
            file,
            " {:4}     {:<8}      {:4}       0       0       0       0.0",
            i + 1,         // IT: type ID
            element,       // TXTT: element symbol
            atomic_number  // ZT: atomic number
        )?;
    }

    writeln!(
        file,
        "*******************************************************************************"
    )?;

    Ok(())
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Extract first number from a line
fn extract_first_number(line: &str) -> Option<f64> {
    line.split(|c: char| !c.is_numeric() && c != '.' && c != '-' && c != 'e' && c != 'E')
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
