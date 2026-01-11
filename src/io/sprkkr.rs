use std::fs::File;
use std::io::{self, BufRead, Write}; // Added Write trait
use std::path::Path;
use std::collections::HashMap;
use crate::model::{Atom, Structure};
// Import the shared atomic number lookup
use crate::model::elements::get_atomic_number;

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

        match current_section {
            Section::Lattice => {
                if trimmed.starts_with("ALAT=") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if let Some(val_str) = parts.get(1) {
                        if let Ok(val) = val_str.parse::<f64>() { alat = val * 0.529177; } // Bohr -> Angstrom
                    }
                } else if trimmed.starts_with("A1=") {
                    if let Some(v) = parse_vec(trimmed) { lattice[0] = v; }
                } else if trimmed.starts_with("A2=") {
                    if let Some(v) = parse_vec(trimmed) { lattice[1] = v; }
                } else if trimmed.starts_with("A3=") {
                    if let Some(v) = parse_vec(trimmed) { lattice[2] = v; }
                }
            },
            Section::Sites => {
                if let Some(idx_end) = trimmed.find(' ') {
                    if let Ok(id) = trimmed[..idx_end].parse::<usize>() {
                        if let Some(pos) = parse_vec(&trimmed[idx_end..]) {
                            site_positions.insert(id, pos);
                        }
                    }
                }
            },
            Section::Occupation => {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 6 {
                    if let (Ok(site_id), Ok(type_id)) = (parts[0].parse::<usize>(), parts[4].parse::<usize>()) {
                        site_to_type.insert(site_id, type_id);
                    }
                }
            },
            Section::Types => {
                if let (Some(_iqt_pos), Some(txt_pos)) = (trimmed.find("IQ=").or(trimmed.find(|c: char| c.is_numeric())), trimmed.find("TXT=")) {
                     let parts: Vec<&str> = trimmed.split_whitespace().collect();
                     if let Ok(id) = parts[0].parse::<usize>() {
                         let element_part = &trimmed[txt_pos+4..];
                         let element = element_part.split_whitespace().next().unwrap_or("X");
                         type_labels.insert(id, element.to_string());
                     }
                }
            },
            _ => {}
        }
    }

    // Scale Lattice
    for i in 0..3 {
        for j in 0..3 {
            lattice[i][j] *= alat;
        }
    }

    let mut atoms = Vec::new();
    let mut index = 0;

    for (site_id, pos_frac) in site_positions {
        if let Some(type_id) = site_to_type.get(&site_id) {
            if let Some(el) = type_labels.get(type_id) {
                // Convert Frac -> Cart
                let x = pos_frac[0]*lattice[0][0] + pos_frac[1]*lattice[1][0] + pos_frac[2]*lattice[2][0];
                let y = pos_frac[0]*lattice[0][1] + pos_frac[1]*lattice[1][1] + pos_frac[2]*lattice[2][1];
                let z = pos_frac[0]*lattice[0][2] + pos_frac[1]*lattice[1][2] + pos_frac[2]*lattice[2][2];

                atoms.push(Atom {
                    element: el.clone(),
                    position: [x, y, z],
                    original_index: index,
                });
                index += 1;
            }
        }
    }

    Ok(Structure { lattice, atoms, formula: "SPR-KKR Import".to_string() })
}

fn parse_vec(line: &str) -> Option<[f64; 3]> {
    let parts: Vec<f64> = line.replace("=", " ").split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() >= 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

// --- YOUR IMPLEMENTATION (Restored) ---
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
        // CALLING SHARED LOGIC HERE:
        let z = get_atomic_number(el);

        writeln!(file, " {:4}     {:<8}      {:4}       0       0       0       0.0",
            i+1, el, z
        )?;
    }

    Ok(())
}
