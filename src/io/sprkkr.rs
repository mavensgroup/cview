use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::collections::HashMap;
use crate::structure::{Atom, Structure};

#[derive(PartialEq)]
enum Section {
    None,
    Lattice,
    Sites,
    Occupation,
    Types,
}

pub fn parse(path: &str) -> io::Result<Structure> {
    let path = Path::new(path);
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut alat = 1.0;
    let mut lattice = [[0.0; 3]; 3];
    let mut site_positions: HashMap<usize, [f64; 3]> = HashMap::new();
    let mut type_labels: HashMap<usize, String> = HashMap::new();
    let mut site_to_type: HashMap<usize, usize> = HashMap::new();

    let mut current_section = Section::None;

    println!("--- Parsing SPR-KKR File: {:?} ---", path);

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed.starts_with("******") { current_section = Section::None; continue; }
        if trimmed == "LATTICE" { current_section = Section::Lattice; continue; }
        if trimmed == "SITES" { current_section = Section::Sites; continue; }
        if trimmed == "OCCUPATION" { current_section = Section::Occupation; continue; }
        if trimmed == "TYPES" { current_section = Section::Types; continue; }

        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

        match current_section {
            Section::Lattice => {
                if trimmed.starts_with("ALAT") {
                    if let Some(val) = parse_value_after_key(trimmed, "ALAT") { alat = val; }
                } else if trimmed.starts_with("A(1)") {
                    if let Some(v) = parse_vector_after_key(trimmed, "A(1)") { lattice[0] = v; }
                } else if trimmed.starts_with("A(2)") {
                    if let Some(v) = parse_vector_after_key(trimmed, "A(2)") { lattice[1] = v; }
                } else if trimmed.starts_with("A(3)") {
                    if let Some(v) = parse_vector_after_key(trimmed, "A(3)") { lattice[2] = v; }
                }
            }
            Section::Sites => {
                if trimmed.starts_with("IQ") || trimmed.starts_with("CARTESIAN") || trimmed.starts_with("BASSCALE") { continue; }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 4 {
                    if let Ok(iq) = parts[0].parse::<usize>() {
                        let x = parts[1].parse::<f64>().unwrap_or(0.0);
                        let y = parts[2].parse::<f64>().unwrap_or(0.0);
                        let z = parts[3].parse::<f64>().unwrap_or(0.0);
                        site_positions.insert(iq, [x, y, z]);
                    }
                }
            }
            Section::Occupation => {
                if trimmed.starts_with("IQ") { continue; }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 5 {
                    if let Ok(iq) = parts[0].parse::<usize>() {
                        if let Ok(itoq) = parts[4].parse::<usize>() {
                            site_to_type.insert(iq, itoq);
                        }
                    }
                }
            }
            Section::Types => {
                if trimmed.starts_with("IT") { continue; }
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(it) = parts[0].parse::<usize>() {
                        type_labels.insert(it, parts[1].to_string());
                    }
                }
            }
            Section::None => {}
        }
    }

    let mut atoms = Vec::new();
    let mut iqs: Vec<&usize> = site_positions.keys().collect();
    iqs.sort();

    for iq in iqs {
        let raw_pos = site_positions[iq];
        let mut label = "X".to_string();
        if let Some(it) = site_to_type.get(iq) {
            if let Some(l) = type_labels.get(it) {
                label = l.chars().take_while(|c| c.is_alphabetic()).collect();
            }
        }
        let pos = [
            raw_pos[0] * alat,
            raw_pos[1] * alat,
            raw_pos[2] * alat
        ];
        atoms.push(Atom { element: label, position: pos });
    }

    // Apply ALAT Scaling to Lattice
    for i in 0..3 { for j in 0..3 { lattice[i][j] *= alat; } }

    Ok(Structure { lattice, atoms })
}

// --- UPDATED WRITER ---

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    writeln!(file, "*******************************************************************************")?;
    writeln!(file, "HEADER    'Exported by CView'")?;
    writeln!(file, "*******************************************************************************")?;

    // 1. LATTICE
    writeln!(file, "LATTICE")?;
    writeln!(file, "ALAT= 1.0000")?;
    for (i, vec) in structure.lattice.iter().enumerate() {
        writeln!(file, "A({})   {:14.8} {:14.8} {:14.8}", i+1, vec[0], vec[1], vec[2])?;
    }

    // 2. SITES
    writeln!(file, "*******************************************************************************")?;
    writeln!(file, "SITES")?;
    writeln!(file, "   IQ       QX             QY             QZ")?;

    for (i, atom) in structure.atoms.iter().enumerate() {
        writeln!(file, " {:4}  {:14.8} {:14.8} {:14.8}",
            i+1, atom.position[0], atom.position[1], atom.position[2]
        )?;
    }

    // 3. OCCUPATION & TYPES LOGIC
    // We must identify unique elements to create the TYPES section
    // and map atoms to these types for the OCCUPATION section.

    let mut unique_elements: Vec<String> = Vec::new();
    let mut atom_to_type_id: Vec<usize> = Vec::new();

    for atom in &structure.atoms {
        if let Some(idx) = unique_elements.iter().position(|e| e == &atom.element) {
            atom_to_type_id.push(idx + 1); // 1-based index
        } else {
            unique_elements.push(atom.element.clone());
            atom_to_type_id.push(unique_elements.len());
        }
    }

    writeln!(file, "*******************************************************************************")?;
    writeln!(file, "OCCUPATION")?;
    writeln!(file, "   IQ     IREFQ       IMQ       NOQ  ITOQ  CONC")?;

    for (i, type_id) in atom_to_type_id.iter().enumerate() {
        // IQ=i+1, ITOQ=type_id, CONC=1.0 (assuming ordered structure)
        writeln!(file, " {:4} {:9} {:9} {:9} {:5}  1.0000", i+1, i+1, i+1, 1, type_id)?;
    }

    writeln!(file, "*******************************************************************************")?;
    writeln!(file, "TYPES")?;
    writeln!(file, "   IT     TXT             ZT      NC      LC      KC      VC")?;

    for (i, el) in unique_elements.iter().enumerate() {
        let z = get_atomic_number(el);
        // IT=i+1, TXT=Element, ZT=AtomicNum
        writeln!(file, " {:4}     {:<8}      {:4}       0       0       0       0.0",
            i+1, el, z
        )?;
    }

    Ok(())
}

// --- Helpers ---

fn parse_value_after_key(line: &str, key: &str) -> Option<f64> {
    if let Some(idx) = line.find(key) {
        let rest = &line[idx + key.len()..];
        let clean = rest.replace('D', "E").replace('d', "e");
        let clean = clean.trim_start_matches(|c| c == '=' || c == ' ');
        return clean.split_whitespace().next()?.parse().ok();
    }
    None
}

fn parse_vector_after_key(line: &str, key: &str) -> Option<[f64; 3]> {
    if let Some(idx) = line.find(key) {
        let rest = &line[idx + key.len()..];
        let clean = rest.replace('D', "E").replace('d', "e");
        let clean = clean.trim_start_matches(|c| c == '=' || c == ' ');

        let parts: Vec<f64> = clean.split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if parts.len() >= 3 {
            return Some([parts[0], parts[1], parts[2]]);
        }
    }
    None
}

// Basic Atomic Number Lookup
fn get_atomic_number(symbol: &str) -> usize {
    match symbol.trim() {
        "H" => 1, "He" => 2, "Li" => 3, "Be" => 4, "B" => 5, "C" => 6, "N" => 7, "O" => 8,
        "F" => 9, "Ne" => 10, "Na" => 11, "Mg" => 12, "Al" => 13, "Si" => 14, "P" => 15,
        "S" => 16, "Cl" => 17, "Ar" => 18, "K" => 19, "Ca" => 20, "Sc" => 21, "Ti" => 22,
        "V" => 23, "Cr" => 24, "Mn" => 25, "Fe" => 26, "Co" => 27, "Ni" => 28, "Cu" => 29,
        "Zn" => 30, "Ga" => 31, "Ge" => 32, "As" => 33, "Se" => 34, "Br" => 35, "Kr" => 36,
        "Rb" => 37, "Sr" => 38, "Y" => 39, "Zr" => 40, "Nb" => 41, "Mo" => 42, "Tc" => 43,
        "Ru" => 44, "Rh" => 45, "Pd" => 46, "Ag" => 47, "Cd" => 48, "In" => 49, "Sn" => 50,
        "Sb" => 51, "Te" => 52, "I" => 53, "Xe" => 54, "Cs" => 55, "Ba" => 56, "La" => 57,
        "Ce" => 58, "Pr" => 59, "Nd" => 60, "Pm" => 61, "Sm" => 62, "Eu" => 63, "Gd" => 64,
        "Tb" => 65, "Dy" => 66, "Ho" => 67, "Er" => 68, "Tm" => 69, "Yb" => 70, "Lu" => 71,
        "Hf" => 72, "Ta" => 73, "W" => 74, "Re" => 75, "Os" => 76, "Ir" => 77, "Pt" => 78,
        "Au" => 79, "Hg" => 80, "Tl" => 81, "Pb" => 82, "Bi" => 83, "Po" => 84, "At" => 85,
        "Rn" => 86, "Fr" => 87, "Ra" => 88, "Ac" => 89, "Th" => 90, "Pa" => 91, "U" => 92,
        _ => 0, // Unknown
    }
}
