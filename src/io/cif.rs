use crate::model::{Atom, Structure};
use crate::utils::linalg::frac_to_cart;
use std::fs::File;
use std::io::Write;
use std::io::{self, BufRead};
use std::path::Path;

/// Parse a CIF file into a Structure.
///
/// Supports:
/// - Cell parameters: `_cell_length_{a,b,c}`, `_cell_angle_{alpha,beta,gamma}`.
/// - Symmetry operations: both legacy (`_symmetry_equiv_pos_as_xyz`) and modern
///   (`_space_group_symop_operation_xyz`) tags. Operation id columns are tolerated.
/// - Bravais centering fallback: if only the primitive-setting operations are
///   listed and the Hermann–Mauguin symbol implies a centered cell (A, B, C, I,
///   F, R), the missing centering translations are synthesized.
/// - Atom sites: fractional coordinates via `_atom_site_fract_{x,y,z}`.
///   Element symbol comes from `_atom_site_type_symbol` when present (IUCr
///   precedence rule), otherwise from component_0 of `_atom_site_label`.
///
/// Not yet supported (silently ignored):
/// - Partial occupancy (`_atom_site_occupancy`): atoms are kept regardless.
/// - Cartesian coordinates (`_atom_site_Cartn_{x,y,z}`).
/// - Hall-symbol-only CIFs (no explicit symop loop): only the identity is
///   applied, falling through to the centering fallback if an H-M symbol is
///   present.
pub fn parse(path: &str) -> io::Result<Structure> {
    let path = Path::new(path);
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut a = 0.0;
    let mut b = 0.0;
    let mut c = 0.0;
    let mut alpha = 90.0;
    let mut beta = 90.0;
    let mut gamma = 90.0;

    // Hermann–Mauguin symbol for centering fallback. First non-blank character
    // is the Bravais lattice type (P/A/B/C/I/F/R). We look at both the modern
    // and legacy tags and use whichever appears first.
    let mut hm_symbol: Option<String> = None;

    let mut symmetry_ops: Vec<String> = Vec::new();
    let mut base_atoms: Vec<Atom> = Vec::new();

    let mut in_loop = false;
    let mut current_loop_headers: Vec<String> = Vec::new();

    // Per CIF spec (IUCr, Hall/Allen/Brown 1991), a `;` at column 1 opens
    // a multi-line text field; the next line beginning with `;` at column 1
    // closes it. Everything in between is opaque string data and MUST NOT
    // be parsed as tags/loops/data — both for correctness (text values can
    // legally contain anything) and for speed (ShelXL-produced CIFs embed
    // hkl reflection dumps here, often 100k+ lines).
    let mut in_text_field = false;

    for line_res in reader.lines() {
        let line = line_res?;

        // --- Semicolon text field (highest priority, before trim) ---
        // The CIF rule is strict: the `;` must be the first character of
        // the raw line. Do NOT use the trimmed line for this check.
        if in_text_field {
            if line.starts_with(';') {
                in_text_field = false;
            }
            continue;
        }
        if line.starts_with(';') {
            in_text_field = true;
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // --- Cell parameters (scalar, not in a loop) ---
        if trimmed.starts_with("_cell_length_a") {
            a = parse_cif_val(trimmed);
            continue;
        }
        if trimmed.starts_with("_cell_length_b") {
            b = parse_cif_val(trimmed);
            continue;
        }
        if trimmed.starts_with("_cell_length_c") {
            c = parse_cif_val(trimmed);
            continue;
        }
        if trimmed.starts_with("_cell_angle_alpha") {
            alpha = parse_cif_val(trimmed);
            continue;
        }
        if trimmed.starts_with("_cell_angle_beta") {
            beta = parse_cif_val(trimmed);
            continue;
        }
        if trimmed.starts_with("_cell_angle_gamma") {
            gamma = parse_cif_val(trimmed);
            continue;
        }

        // --- Hermann–Mauguin symbol (scalar) ---
        // Modern tag takes precedence; legacy is a fallback. Once set, don't
        // overwrite.
        if hm_symbol.is_none()
            && (trimmed.starts_with("_space_group_name_H-M_alt")
                || trimmed.starts_with("_symmetry_space_group_name_H-M"))
        {
            if let Some(sym) = extract_quoted_or_tail(trimmed) {
                hm_symbol = Some(sym);
            }
            continue;
        }

        // --- Loop detection ---
        if trimmed == "loop_" || trimmed.starts_with("loop_") {
            in_loop = true;
            current_loop_headers.clear();
            continue;
        }

        // --- A new data_ block closes any active loop ---
        if trimmed.starts_with("data_") {
            in_loop = false;
            current_loop_headers.clear();
            continue;
        }

        // --- Header lines inside a loop ---
        if in_loop && trimmed.starts_with('_') {
            current_loop_headers.push(trimmed.to_string());
            continue;
        }

        // --- A non-header, non-loop tag line ends any loop we were in ---
        if !in_loop {
            continue;
        }
        if trimmed.starts_with('_') {
            // This branch is normally caught above; reachable if we somehow
            // left a header alone. Keep as defensive.
            continue;
        }

        // --- Loop data rows ---
        let is_atom_loop = current_loop_headers
            .iter()
            .any(|h| h.contains("_atom_site_fract_x"));

        let is_sym_loop = current_loop_headers.iter().any(|h| {
            h.contains("_symmetry_equiv_pos_as_xyz")
                || h.contains("_space_group_symop_operation_xyz")
        });

        if is_sym_loop {
            if let Some(op) = extract_symop_string(trimmed) {
                symmetry_ops.push(op);
            }
        } else if is_atom_loop {
            if let Some(atom) = parse_atom_row(trimmed, &current_loop_headers) {
                base_atoms.push(atom);
            }
        }
    }

    // --- Post-processing: symmetry ---
    if symmetry_ops.is_empty() {
        symmetry_ops.push("x,y,z".to_string());
    }

    // Bravais-centering fallback: if the H-M symbol starts with a centering
    // letter and the parsed ops don't already include the centering
    // translations, synthesize them.
    if let Some(hm) = &hm_symbol {
        let centering_ops = centering_translations(hm);
        if !centering_ops.is_empty() && !ops_include_centering(&symmetry_ops, &centering_ops) {
            let primitive_ops = symmetry_ops.clone();
            for (dx, dy, dz) in &centering_ops {
                for op in &primitive_ops {
                    symmetry_ops.push(translated_op(op, *dx, *dy, *dz));
                }
            }
        }
    }

    // --- Expand asymmetric unit with symmetry ---
    let mut final_atoms: Vec<Atom> = Vec::new();
    let epsilon = 1e-3;

    for atom in &base_atoms {
        for op in &symmetry_ops {
            let new_pos = apply_symmetry(atom.position, op);

            let wx = new_pos[0].rem_euclid(1.0);
            let wy = new_pos[1].rem_euclid(1.0);
            let wz = new_pos[2].rem_euclid(1.0);

            let is_duplicate = final_atoms.iter().any(|existing| {
                let dx = (existing.position[0] - wx).abs();
                let dy = (existing.position[1] - wy).abs();
                let dz = (existing.position[2] - wz).abs();
                (dx < epsilon || (1.0 - dx) < epsilon)
                    && (dy < epsilon || (1.0 - dy) < epsilon)
                    && (dz < epsilon || (1.0 - dz) < epsilon)
            });

            if !is_duplicate {
                let idx = final_atoms.len();
                final_atoms.push(Atom {
                    element: atom.element.clone(),
                    position: [wx, wy, wz],
                    original_index: idx,
                });
            }
        }
    }

    // --- Build lattice matrix and convert to Cartesian ---
    let to_rad = std::f64::consts::PI / 180.0;
    let alpha_r = alpha * to_rad;
    let beta_r = beta * to_rad;
    let gamma_r = gamma * to_rad;
    let v = (1.0 - alpha_r.cos().powi(2) - beta_r.cos().powi(2) - gamma_r.cos().powi(2)
        + 2.0 * alpha_r.cos() * beta_r.cos() * gamma_r.cos())
    .sqrt();

    let lattice = [
        [a, 0.0, 0.0],
        [b * gamma_r.cos(), b * gamma_r.sin(), 0.0],
        [
            c * beta_r.cos(),
            c * (alpha_r.cos() - beta_r.cos() * gamma_r.cos()) / gamma_r.sin(),
            c * v / gamma_r.sin(),
        ],
    ];

    for atom in &mut final_atoms {
        atom.position = frac_to_cart(atom.position, lattice);
    }

    Ok(Structure {
        lattice,
        atoms: final_atoms,
        formula: "CIF Import".to_string(),
        is_periodic: true,
    })
}

// =========================================================================
// Symmetry op extraction / evaluation
// =========================================================================

/// Given a raw data-row line from a symop loop, extract the "x,y,z"-style
/// operation string. Handles rows with a leading integer id and/or surrounding
/// single or double quotes. Returns None if no commas are found.
fn extract_symop_string(line: &str) -> Option<String> {
    let stripped = line.replace('\'', "").replace('"', "");
    let s = stripped.trim();

    // If the whole thing is surrounded by extra tokens (e.g. leading id), the
    // actual op contains at least two commas. Walk tokens and pick the first
    // one containing a comma; if that one has no commas in any single token,
    // fall back to stripping a leading numeric id.
    let comma_count = s.matches(',').count();
    if comma_count < 2 {
        return None;
    }

    // Heuristic: if the line starts with an integer followed by whitespace,
    // drop it. Otherwise use the whole line.
    let first_word_end = s.find(char::is_whitespace);
    if let Some(end) = first_word_end {
        let first = &s[..end];
        if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
            return Some(s[end..].trim().to_string());
        }
    }

    Some(s.to_string())
}

fn apply_symmetry(p: [f64; 3], op: &str) -> [f64; 3] {
    let parts: Vec<&str> = op.split(',').collect();
    if parts.len() != 3 {
        return p;
    }
    [
        evaluate_expr(parts[0], p),
        evaluate_expr(parts[1], p),
        evaluate_expr(parts[2], p),
    ]
}

fn evaluate_expr(expr: &str, p: [f64; 3]) -> f64 {
    let s = expr.replace(' ', "").to_lowercase();
    let mut val = 0.0;
    let mut current_term = String::new();

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

    let mut sign = 1.0;
    if t.starts_with('-') {
        sign = -1.0;
        t.remove(0);
    } else if t.starts_with('+') {
        t.remove(0);
    }

    if t.contains('x') {
        return sign * p[0];
    } else if t.contains('y') {
        return sign * p[1];
    } else if t.contains('z') {
        return sign * p[2];
    }

    if let Some(idx) = t.find('/') {
        let num: f64 = t[..idx].parse().unwrap_or(0.0);
        let den: f64 = t[idx + 1..].parse().unwrap_or(1.0);
        sign * (num / den)
    } else {
        sign * t.parse::<f64>().unwrap_or(0.0)
    }
}

// =========================================================================
// Bravais-centering fallback
// =========================================================================

/// Return the list of centering translations (other than (0,0,0)) implied by
/// the first character of the Hermann–Mauguin symbol.
fn centering_translations(hm: &str) -> Vec<(f64, f64, f64)> {
    let first = hm.trim().chars().next().unwrap_or(' ');
    match first {
        'A' => vec![(0.0, 0.5, 0.5)],
        'B' => vec![(0.5, 0.0, 0.5)],
        'C' => vec![(0.5, 0.5, 0.0)],
        'I' => vec![(0.5, 0.5, 0.5)],
        'F' => vec![(0.0, 0.5, 0.5), (0.5, 0.0, 0.5), (0.5, 0.5, 0.0)],
        'R' => vec![
            (2.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0),
            (1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0),
        ],
        _ => Vec::new(),
    }
}

/// Detect whether the already-parsed op list already contains the centering
/// translations. We test by applying each op to the reference point
/// (0.1, 0.2, 0.3) and checking if the centering-translated versions appear.
fn ops_include_centering(ops: &[String], centering: &[(f64, f64, f64)]) -> bool {
    if ops.is_empty() {
        return false;
    }
    let test_point = [0.1, 0.2, 0.3];
    let identity_image = apply_symmetry(test_point, &ops[0]); // usually x,y,z
    let epsilon = 1e-4;

    // For each centering vector, check if any op produces identity + centering.
    centering.iter().all(|(dx, dy, dz)| {
        let expected = [
            (identity_image[0] + dx).rem_euclid(1.0),
            (identity_image[1] + dy).rem_euclid(1.0),
            (identity_image[2] + dz).rem_euclid(1.0),
        ];
        ops.iter().any(|op| {
            let image = apply_symmetry(test_point, op);
            let wx = image[0].rem_euclid(1.0);
            let wy = image[1].rem_euclid(1.0);
            let wz = image[2].rem_euclid(1.0);
            (wx - expected[0]).abs() < epsilon
                && (wy - expected[1]).abs() < epsilon
                && (wz - expected[2]).abs() < epsilon
        })
    })
}

/// Produce a new symop string that is `op` followed by a translation
/// (dx, dy, dz). We operate on the string form by appending the translation
/// to each of the three coordinate expressions.
fn translated_op(op: &str, dx: f64, dy: f64, dz: f64) -> String {
    let parts: Vec<&str> = op.split(',').collect();
    if parts.len() != 3 {
        return op.to_string();
    }
    format!(
        "{}+{},{}+{},{}+{}",
        parts[0].trim(),
        format_frac(dx),
        parts[1].trim(),
        format_frac(dy),
        parts[2].trim(),
        format_frac(dz)
    )
}

fn format_frac(v: f64) -> String {
    // Match common CIF fractions exactly so downstream evaluation stays
    // numerically clean.
    let candidates: &[(f64, &str)] = &[
        (1.0 / 2.0, "1/2"),
        (1.0 / 3.0, "1/3"),
        (2.0 / 3.0, "2/3"),
        (1.0 / 4.0, "1/4"),
        (3.0 / 4.0, "3/4"),
        (1.0 / 6.0, "1/6"),
        (5.0 / 6.0, "5/6"),
    ];
    for (val, s) in candidates {
        if (v - val).abs() < 1e-6 {
            return (*s).to_string();
        }
    }
    format!("{:.6}", v)
}

// =========================================================================
// Atom loop row parsing
// =========================================================================

fn parse_atom_row(line: &str, headers: &[String]) -> Option<Atom> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    let mut label_val: Option<&str> = None;
    let mut type_symbol_val: Option<&str> = None;
    let mut fx = None;
    let mut fy = None;
    let mut fz = None;

    for (i, header) in headers.iter().enumerate() {
        if i >= parts.len() {
            break;
        }
        let val = parts[i];

        // Order matters: check type_symbol BEFORE the generic label match,
        // because "_atom_site_label" is a substring of "_atom_site_label_*"
        // but "_atom_site_type_symbol" is unambiguous.
        if header.contains("_atom_site_type_symbol") {
            type_symbol_val = Some(val);
        } else if header.contains("_atom_site_label") {
            label_val = Some(val);
        } else if header.contains("_atom_site_fract_x") {
            fx = Some(parse_cif_float(val));
        } else if header.contains("_atom_site_fract_y") {
            fy = Some(parse_cif_float(val));
        } else if header.contains("_atom_site_fract_z") {
            fz = Some(parse_cif_float(val));
        }
    }

    let (fx, fy, fz) = match (fx, fy, fz) {
        (Some(x), Some(y), Some(z)) => (x, y, z),
        _ => return None,
    };

    // IUCr rule: type_symbol takes precedence. Fall back to label component_0.
    let element = if let Some(ts) = type_symbol_val {
        normalize_element(ts)
    } else if let Some(lbl) = label_val {
        element_from_label(lbl)
    } else {
        "X".to_string()
    };

    Some(Atom {
        element,
        position: [fx, fy, fz],
        original_index: 0,
    })
}

/// Extract the element symbol from an `_atom_site_label` per IUCr component_0
/// rules: alphabetic prefix ending at the first digit. e.g. "SI1A" -> "Si",
/// "Fe2+" -> "Fe" (stop at digit), "O1B" -> "O".
fn element_from_label(label: &str) -> String {
    let mut prefix = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphabetic() {
            prefix.push(ch);
        } else {
            break;
        }
    }
    normalize_element(&prefix)
}

/// Normalize an element string: strip charge/oxidation annotations, cap at
/// two alphabetic characters, then use Title case (first upper, rest lower).
fn normalize_element(s: &str) -> String {
    let alpha: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if alpha.is_empty() {
        return "X".to_string();
    }
    // Cap at 2 chars — no element symbol is longer.
    let capped: String = alpha.chars().take(2).collect();
    let mut out = String::with_capacity(capped.len());
    for (i, ch) in capped.chars().enumerate() {
        if i == 0 {
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

// =========================================================================
// Low-level value extraction
// =========================================================================

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

/// Extract a (possibly quoted) value from a scalar CIF line like
/// `_space_group_name_H-M_alt 'A 1 2/n 1'`. Returns the value without the
/// surrounding quotes. If the tag is followed only by whitespace (multi-line
/// semicolon-delimited value), returns None.
fn extract_quoted_or_tail(line: &str) -> Option<String> {
    let ws = line.find(char::is_whitespace)?;
    let tail = line[ws..].trim();
    if tail.is_empty() {
        return None;
    }
    // Strip matched single or double quotes if they surround the whole value.
    let stripped = if (tail.starts_with('\'') && tail.ends_with('\'') && tail.len() >= 2)
        || (tail.starts_with('"') && tail.ends_with('"') && tail.len() >= 2)
    {
        &tail[1..tail.len() - 1]
    } else {
        tail
    };
    Some(stripped.to_string())
}

// =========================================================================
// Writer (unchanged)
// =========================================================================

pub fn write(path: &str, structure: &Structure) -> io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    writeln!(file, "data_generated_by_cview")?;
    writeln!(file, "_pd_phase_name 'Exported Structure'")?;
    writeln!(file, "_symmetry_space_group_name_H-M 'P 1'")?;
    writeln!(file, "_symmetry_Int_Tables_number 1")?;

    let a_vec = structure.lattice[0];
    let b_vec = structure.lattice[1];
    let c_vec = structure.lattice[2];

    let a = (a_vec[0].powi(2) + a_vec[1].powi(2) + a_vec[2].powi(2)).sqrt();
    let b = (b_vec[0].powi(2) + b_vec[1].powi(2) + b_vec[2].powi(2)).sqrt();
    let c = (c_vec[0].powi(2) + c_vec[1].powi(2) + c_vec[2].powi(2)).sqrt();

    let b_dot_c = b_vec[0] * c_vec[0] + b_vec[1] * c_vec[1] + b_vec[2] * c_vec[2];
    let a_dot_c = a_vec[0] * c_vec[0] + a_vec[1] * c_vec[1] + a_vec[2] * c_vec[2];
    let a_dot_b = a_vec[0] * b_vec[0] + a_vec[1] * b_vec[1] + a_vec[2] * b_vec[2];

    let to_deg = 180.0 / std::f64::consts::PI;
    let alpha = (b_dot_c / (b * c)).acos() * to_deg;
    let beta = (a_dot_c / (a * c)).acos() * to_deg;
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

    use crate::utils::linalg::cart_to_frac;

    for (i, atom) in structure.atoms.iter().enumerate() {
        let frac = cart_to_frac(atom.position, structure.lattice).unwrap_or([0.0, 0.0, 0.0]);
        let (u, v, w) = (frac[0], frac[1], frac[2]);
        writeln!(
            file,
            " {}{} {:.6} {:.6} {:.6}",
            atom.element,
            i + 1,
            u,
            v,
            w
        )?;
    }

    Ok(())
}
