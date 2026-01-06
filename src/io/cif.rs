use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use crate::structure::{Atom, Structure};
use std::io::Write;

pub fn parse(path: &str) -> io::Result<Structure> {
    let path = Path::new(path);
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut a = 0.0; let mut b = 0.0; let mut c = 0.0;
    let mut alpha = 90.0; let mut beta = 90.0; let mut gamma = 90.0;

    let mut symmetry_ops = Vec::new();
    let mut base_atoms = Vec::new();

    let mut in_loop = false;
    let mut current_loop_headers = Vec::new();

    let mut lines = reader.lines().peekable();

    while let Some(line_res) = lines.next() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

        // 1. Cell Parameters
        if trimmed.starts_with("_cell_length_a") { a = parse_cif_val(trimmed); }
        if trimmed.starts_with("_cell_length_b") { b = parse_cif_val(trimmed); }
        if trimmed.starts_with("_cell_length_c") { c = parse_cif_val(trimmed); }
        if trimmed.starts_with("_cell_angle_alpha") { alpha = parse_cif_val(trimmed); }
        if trimmed.starts_with("_cell_angle_beta") { beta = parse_cif_val(trimmed); }
        if trimmed.starts_with("_cell_angle_gamma") { gamma = parse_cif_val(trimmed); }

        // 2. Loop Detection
        if trimmed.starts_with("loop_") {
            in_loop = true;
            current_loop_headers.clear();
            continue;
        }

        // 3. Header Parsing
        if in_loop && trimmed.starts_with("_") {
            current_loop_headers.push(trimmed.to_string());
            continue;
        }

        // 4. Data Parsing
        if in_loop {
            if trimmed.starts_with("data_") || trimmed.starts_with("loop_") {
                in_loop = false;
            } else {
                let is_atom_loop = current_loop_headers.iter().any(|h| h.contains("_atom_site_fract_x"));
                let is_sym_loop = current_loop_headers.iter().any(|h| h.contains("_symmetry_equiv_pos_as_xyz"));

                if is_sym_loop {
                    // Robust extraction of symmetry string
                    let full_line = trimmed.replace("'", "").replace("\"", "");
                    // Find where the operation likely starts (contains x, y, z or commas)
                    if let Some(op_start) = full_line.find(|c: char| c.is_alphabetic() && "xyz".contains(c)) {
                         // Check if preceded by an index number
                         if let Some(idx) = full_line[..op_start].rfind(|c: char| c.is_numeric()) {
                             // likely "96  x, y, z" -> slice from op_start
                             // But we need to be careful not to slice off a minus sign like " -x"
                             // Safer strategy: find the first alphabetic char or minus sign after the index
                             let op = full_line[idx+1..].trim();
                             symmetry_ops.push(op.to_string());
                         } else {
                             // No index found, take whole line or heuristic
                             symmetry_ops.push(full_line.trim().to_string());
                         }
                    } else {
                        // Fallback: splitting by whitespace might fail on "x, y, z" vs "1 x,y,z"
                        // Try finding the comma
                        if full_line.contains(',') {
                             // heuristic: if it has commas, it's likely the op, maybe strip leading number
                             let op = full_line.trim_start_matches(|c: char| c.is_numeric() || c.is_whitespace());
                             symmetry_ops.push(op.to_string());
                        }
                    }
                } else if is_atom_loop {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let mut label = "X".to_string();
                        let mut fx = 0.0; let mut fy = 0.0; let mut fz = 0.0;

                        for (i, header) in current_loop_headers.iter().enumerate() {
                            if i >= parts.len() { break; }
                            let val = parts[i];
                            if header.contains("_atom_site_label") || header.contains("_atom_site_type_symbol") {
                                label = val.chars().filter(|c| c.is_alphabetic()).collect();
                            } else if header.contains("_atom_site_fract_x") { fx = parse_cif_float(val); }
                            else if header.contains("_atom_site_fract_y") { fy = parse_cif_float(val); }
                            else if header.contains("_atom_site_fract_z") { fz = parse_cif_float(val); }
                        }
                        base_atoms.push(Atom { element: label, position: [fx, fy, fz] });
                    }
                }
            }
        }
    }

    if symmetry_ops.is_empty() {
        symmetry_ops.push("x,y,z".to_string());
    }

    // 5. Expand Symmetry
    let mut final_atoms = Vec::new();
    let epsilon = 0.001; // Tighter tolerance

    for atom in base_atoms {
        for op in &symmetry_ops {
            let new_pos = apply_symmetry(atom.position, op);

            // Wrap to [0,1)
            let wx = new_pos[0].rem_euclid(1.0);
            let wy = new_pos[1].rem_euclid(1.0);
            let wz = new_pos[2].rem_euclid(1.0);

            // Check for duplicates
            let is_duplicate = final_atoms.iter().any(|existing: &Atom| {
                let dx = (existing.position[0] - wx).abs();
                let dy = (existing.position[1] - wy).abs();
                let dz = (existing.position[2] - wz).abs();
                // Check direct distance OR wrapped distance
                (dx < epsilon || (1.0 - dx) < epsilon) &&
                (dy < epsilon || (1.0 - dy) < epsilon) &&
                (dz < epsilon || (1.0 - dz) < epsilon)
            });

            if !is_duplicate {
                final_atoms.push(Atom {
                    element: atom.element.clone(),
                    position: [wx, wy, wz]
                });
            }
        }
    }

    // 6. Lattice Construction
    let to_rad = std::f64::consts::PI / 180.0;
    let alpha_r = alpha * to_rad;
    let beta_r = beta * to_rad;
    let gamma_r = gamma * to_rad;
    let v = (1.0 - alpha_r.cos().powi(2) - beta_r.cos().powi(2) - gamma_r.cos().powi(2)
             + 2.0 * alpha_r.cos() * beta_r.cos() * gamma_r.cos()).sqrt();

    let lattice = [
        [a, 0.0, 0.0],
        [b * gamma_r.cos(), b * gamma_r.sin(), 0.0],
        [c * beta_r.cos(), c * (alpha_r.cos() - beta_r.cos() * gamma_r.cos()) / gamma_r.sin(), c * v / gamma_r.sin()]
    ];

    for atom in &mut final_atoms {
        let f = atom.position;
        let x = f[0]*lattice[0][0] + f[1]*lattice[1][0] + f[2]*lattice[2][0];
        let y = f[0]*lattice[0][1] + f[1]*lattice[1][1] + f[2]*lattice[2][1];
        let z = f[0]*lattice[0][2] + f[1]*lattice[1][2] + f[2]*lattice[2][2];
        atom.position = [x, y, z];
    }

    Ok(Structure { lattice, atoms: final_atoms })
}

fn apply_symmetry(p: [f64; 3], op: &str) -> [f64; 3] {
    let parts: Vec<&str> = op.split(',').collect();
    if parts.len() != 3 { return p; }

    [
        evaluate_expr(parts[0], p),
        evaluate_expr(parts[1], p),
        evaluate_expr(parts[2], p),
    ]
}

// FIX: New robust tokenizer-based evaluator
fn evaluate_expr(expr: &str, p: [f64; 3]) -> f64 {
    let s = expr.replace(" ", "").to_lowercase();
    let mut val = 0.0;

    let mut current_term = String::new();

    // Split by + or - but keep the delimiter
    for c in s.chars() {
        if (c == '+' || c == '-') && !current_term.is_empty() {
             val += evaluate_term(&current_term, p);
             current_term.clear();
        }
        current_term.push(c);
    }
    if !current_term.is_empty() {
        val += evaluate_term(&current_term, p);
    }

    val
}

fn evaluate_term(term: &str, p: [f64; 3]) -> f64 {
    let mut t = term.to_string();

    // 1. Extract Sign
    let mut sign = 1.0;
    if t.starts_with('-') {
        sign = -1.0;
        t.remove(0);
    } else if t.starts_with('+') {
        t.remove(0);
    }

    // 2. Identify Variable
    if t.contains('x') {
        return sign * p[0];
    } else if t.contains('y') {
        return sign * p[1];
    } else if t.contains('z') {
        return sign * p[2];
    }

    // 3. Identify Fraction or Number
    if let Some(idx) = t.find('/') {
        let num: f64 = t[..idx].parse().unwrap_or(0.0);
        let den: f64 = t[idx+1..].parse().unwrap_or(1.0);
        return sign * (num / den);
    } else {
        return sign * t.parse::<f64>().unwrap_or(0.0);
    }
}

fn parse_cif_val(line: &str) -> f64 {
    if let Some(idx) = line.find(char::is_whitespace) {
         return parse_cif_float(line[idx..].trim());
    }
    0.0
}

fn parse_cif_float(s: &str) -> f64 {
    let clean: String = s.chars().take_while(|c| *c != '(').collect();
    clean.parse().unwrap_or(0.0)
}


pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    writeln!(file, "data_generated_by_cview")?;
    writeln!(file, "_pd_phase_name 'Exported Structure'")?;
    writeln!(file, "_symmetry_space_group_name_H-M 'P 1'")?;
    writeln!(file, "_symmetry_Int_Tables_number 1")?;

    // Calculate Cell Parameters (a, b, c, alpha, beta, gamma)
    let a_vec = structure.lattice[0];
    let b_vec = structure.lattice[1];
    let c_vec = structure.lattice[2];

    let a = (a_vec[0].powi(2) + a_vec[1].powi(2) + a_vec[2].powi(2)).sqrt();
    let b = (b_vec[0].powi(2) + b_vec[1].powi(2) + b_vec[2].powi(2)).sqrt();
    let c = (c_vec[0].powi(2) + c_vec[1].powi(2) + c_vec[2].powi(2)).sqrt();

    // Dot products for angles
    // alpha = angle between b and c
    let b_dot_c = b_vec[0]*c_vec[0] + b_vec[1]*c_vec[1] + b_vec[2]*c_vec[2];
    let a_dot_c = a_vec[0]*c_vec[0] + a_vec[1]*c_vec[1] + a_vec[2]*c_vec[2];
    let a_dot_b = a_vec[0]*b_vec[0] + a_vec[1]*b_vec[1] + a_vec[2]*b_vec[2];

    let to_deg = 180.0 / std::f64::consts::PI;
    let alpha = (b_dot_c / (b * c)).acos() * to_deg;
    let beta  = (a_dot_c / (a * c)).acos() * to_deg;
    let gamma = (a_dot_b / (a * b)).acos() * to_deg;

    writeln!(file, "_cell_length_a    {:.6}", a)?;
    writeln!(file, "_cell_length_b    {:.6}", b)?;
    writeln!(file, "_cell_length_c    {:.6}", c)?;
    writeln!(file, "_cell_angle_alpha {:.6}", alpha)?;
    writeln!(file, "_cell_angle_beta  {:.6}", beta)?;
    writeln!(file, "_cell_angle_gamma {:.6}", gamma)?;

    writeln!(file, "loop_")?;
    writeln!(file, " _atom_site_label")?;
    writeln!(file, " _atom_site_fract_x")?;
    writeln!(file, " _atom_site_fract_y")?;
    writeln!(file, " _atom_site_fract_z")?;

    // Need fractional coordinates
    let inv = crate::io::poscar::inverse_matrix(structure.lattice);

    for (i, atom) in structure.atoms.iter().enumerate() {
        let p = atom.position;
        let u = p[0]*inv[0][0] + p[1]*inv[1][0] + p[2]*inv[2][0];
        let v = p[0]*inv[0][1] + p[1]*inv[1][1] + p[2]*inv[2][1];
        let w = p[0]*inv[0][2] + p[1]*inv[1][2] + p[2]*inv[2][2];

        // Ensure unique label e.g., Fe1, Fe2
        writeln!(file, " {}{} {:.6} {:.6} {:.6}", atom.element, i+1, u, v, w)?;
    }

    Ok(())
}
