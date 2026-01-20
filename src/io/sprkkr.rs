use crate::model::elements::get_atomic_number;
use crate::model::{Atom, Structure};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::Path;

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

    // --- Data Stores ---
    let mut alat = 1.0;
    let mut lattice = [[0.0; 3]; 3];
    let mut site_positions: HashMap<usize, [f64; 3]> = HashMap::new();
    let mut type_labels: HashMap<usize, String> = HashMap::new();
    let mut site_occupation: HashMap<usize, usize> = HashMap::new();

    // --- Metadata Stores ---
    let mut symmetry_info = String::new();
    let mut formula_hint = String::new();

    let mut basscale = [1.0; 3];
    let mut current_section = Section::None;
    let mut found_lattice_vectors = [false; 3];

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // --- 1. Global Metadata Parsing ---
        if trimmed.starts_with("SYSTEM") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() > 1 {
                formula_hint = parts[1..].join(" ");
            }
        }

        if trimmed.starts_with("space group number") {
            // Sometimes appears in .pot header too? Just in case.
            // We can parse next line if needed, but BRAVAIS usually handles it.
        }

        // --- 2. Section Control ---
        if trimmed.starts_with("******") {
            current_section = Section::None;
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

        // --- 3. Section Content ---
        match current_section {
            Section::Lattice => {
                // BRAVAIS Parsing
                if trimmed.starts_with("BRAVAIS") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    // Skip "BRAVAIS" (0) and usually "12" (1). Capture words like "cubic", "m3m".
                    let info: Vec<&str> = parts
                        .iter()
                        .skip(1)
                        .filter(|s| !s.chars().all(char::is_numeric))
                        .cloned()
                        .collect();
                    if !info.is_empty() {
                        symmetry_info = info.join(" ");
                    }
                }

                // ALAT handling
                if trimmed.starts_with("ALAT") {
                    let parts: Vec<&str> = trimmed
                        .split(|c| c == '=' || c == ' ' || c == '\t')
                        .filter(|s| !s.is_empty())
                        .collect();
                    for p in parts {
                        if let Ok(val) = p.parse::<f64>() {
                            // If ALAT > 2.0 (e.g. 5.0, 9.44), it is definitely Bohr.
                            // If ALAT ~ 1.0, it is arbitrary scaling (Angstroms usually).
                            // The xband format file uses Bohr for lattice parameters.
                            if val > 2.0 {
                                alat = val * 0.52917721; // Convert to Angstroms
                            } else {
                                alat = val;
                            }
                            break;
                        }
                    }
                }
                // Vector handling: Strict Prefix Checking
                else if trimmed.starts_with("A") {
                    let idx = if trimmed.starts_with("A(1)") || trimmed.starts_with("A1") {
                        0
                    } else if trimmed.starts_with("A(2)") || trimmed.starts_with("A2") {
                        1
                    } else if trimmed.starts_with("A(3)") || trimmed.starts_with("A3") {
                        2
                    } else {
                        -1
                    };

                    if idx >= 0 {
                        if let Some(v) = parse_vec_flexible(trimmed) {
                            lattice[idx as usize] = v;
                            found_lattice_vectors[idx as usize] = true;
                        }
                    }
                }
            }
            Section::Sites => {
                if trimmed.starts_with("IQ") || trimmed.starts_with("CART") {
                    continue;
                }

                if trimmed.starts_with("BASSCALE") {
                    if let Some(v) = parse_vec_flexible(trimmed) {
                        basscale = v;
                    }
                    continue;
                }

                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 4 {
                    if let Ok(id) = parts[0].parse::<usize>() {
                        if let (Ok(x), Ok(y), Ok(z)) = (
                            parts[1].parse::<f64>(),
                            parts[2].parse::<f64>(),
                            parts[3].parse::<f64>(),
                        ) {
                            site_positions
                                .insert(id, [x * basscale[0], y * basscale[1], z * basscale[2]]);
                        }
                    }
                }
            }
            Section::Occupation => {
                if trimmed.starts_with("IQ") {
                    continue;
                }

                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 4 {
                    if let (Ok(site_id), Ok(noq)) =
                        (parts[0].parse::<usize>(), parts[3].parse::<usize>())
                    {
                        let mut best_type = 0;
                        let mut max_conc = -1.0;
                        let mut cursor = 4;
                        for _ in 0..noq {
                            if cursor + 1 < parts.len() {
                                if let (Ok(tid), Ok(conc)) = (
                                    parts[cursor].parse::<usize>(),
                                    parts[cursor + 1].parse::<f64>(),
                                ) {
                                    if conc > max_conc {
                                        max_conc = conc;
                                        best_type = tid;
                                    }
                                }
                            }
                            cursor += 2;
                        }
                        if best_type > 0 {
                            site_occupation.insert(site_id, best_type);
                        }
                    }
                }
            }
            Section::Types => {
                if trimmed.starts_with("IT") {
                    continue;
                }

                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(id) = parts[0].parse::<usize>() {
                        let label = parts[1].to_string();
                        let clean_label: String =
                            label.chars().filter(|c| c.is_alphabetic()).collect();
                        if !clean_label.is_empty() {
                            type_labels.insert(id, clean_label);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // --- CONSTRUCTION ---

    // Final check for lattice integrity
    if !found_lattice_vectors[0] || !found_lattice_vectors[1] || !found_lattice_vectors[2] {
        println!(
            "Warning: Missing lattice vectors. Read: {:?}",
            found_lattice_vectors
        );
        // Fallback to cubic identity if missing (prevents singular error, allows debugging)
        if !found_lattice_vectors[0] {
            lattice[0] = [1.0, 0.0, 0.0];
        }
        if !found_lattice_vectors[1] {
            lattice[1] = [0.0, 1.0, 0.0];
        }
        if !found_lattice_vectors[2] {
            lattice[2] = [0.0, 0.0, 1.0];
        }
    }

    // Apply ALAT Scaling
    for i in 0..3 {
        for j in 0..3 {
            lattice[i][j] *= alat;
        }
    }

    let mut atoms = Vec::new();
    let mut sorted_ids: Vec<usize> = site_positions.keys().cloned().collect();
    sorted_ids.sort();

    for id in sorted_ids {
        if let Some(raw_pos) = site_positions.get(&id) {
            let type_id = site_occupation.get(&id).cloned().unwrap_or(1);
            let element = type_labels
                .get(&type_id)
                .cloned()
                .unwrap_or_else(|| "X".to_string());

            let pos = [raw_pos[0] * alat, raw_pos[1] * alat, raw_pos[2] * alat];

            atoms.push(Atom {
                element,
                position: pos,
                original_index: atoms.len(),
            });
        }
    }

    let mut formula = if !formula_hint.is_empty() {
        formula_hint
    } else {
        "SPR-KKR Import".to_string()
    };

    if !symmetry_info.is_empty() {
        formula = format!("{} ({})", formula, symmetry_info);
    }

    Ok(Structure {
        lattice,
        atoms,
        formula,
    })
}

fn parse_vec_flexible(line: &str) -> Option<[f64; 3]> {
    let parts: Vec<f64> = line
        .replace("=", " ")
        .replace("(", " ")
        .replace(")", " ")
        .split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    if parts.len() >= 3 {
        let n = parts.len();
        Some([parts[n - 3], parts[n - 2], parts[n - 1]])
    } else {
        None
    }
}

// --- WRITE FUNCTION (Unchanged) ---
pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "HEADER    'Exported by CView'")?;
    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "LATTICE")?;
    writeln!(file, "SYSDIM       3D")?;
    writeln!(file, "SYSTYPE      BULK")?;
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
            i + 1,
            i + 1,
            i + 1,
            1,
            type_id
        )?;
    }
    writeln!(
        file,
        "*******************************************************************************"
    )?;
    writeln!(file, "TYPES")?;
    writeln!(
        file,
        "   IT     TXTT            ZT      NC      LC      KC      VC"
    )?;
    for (i, el) in unique_elements.iter().enumerate() {
        let z = get_atomic_number(el);
        writeln!(
            file,
            " {:4}     {:<8}      {:4}       0       0       0       0.0",
            i + 1,
            el,
            z
        )?;
    }
    Ok(())
}
