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
/// - Partial occupancy (`_atom_site_occupancy`): parsed onto
///   `Atom.occupancy` (default 1.0). Split sites — coincident positions
///   with different elements — are kept as separate atoms; XRD and BVS
///   weight contributions by occupancy (virtual-crystal approximation).
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
    // magCIF (mCIF): magnetic operations and centring translations, each
    // "x,y,z,+1" with a time-reversal flag; site moments by atom label, in
    // μB along unit vectors parallel to a, b, c (`crystalaxis`).
    let mut magn_ops: Vec<String> = Vec::new();
    let mut magn_centering: Vec<String> = Vec::new();
    let mut base_labels: Vec<String> = Vec::new();
    let mut site_moments: std::collections::HashMap<String, [f64; 3]> =
        std::collections::HashMap::new();
    // Optional `_atom_type_*` loop side-table:
    //   key   = `_atom_type_symbol` value as it appears in the CIF
    //           (may include charge, e.g. "Fe3+", or be bare "Fe")
    //   value = `_atom_type_oxidation_number` (integer, when present)
    // Used as a *fallback* for atoms whose `_atom_site_type_symbol` carried
    // no inline charge. Inline charge always wins.
    let mut atom_type_ox: std::collections::HashMap<String, i32> =
        std::collections::HashMap::new();

    let mut in_loop = false;
    let mut current_loop_headers: Vec<String> = Vec::new();

    // Sites with `_atom_site_occupancy` < 1: the Atom model has no occupancy
    // field, so downstream analyses (BVS, XRD) assume full occupancy. We at
    // least tell the user at import time.
    let mut partial_occupancy_sites = 0usize;

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
        // `_space_group_symop_magn_operation.xyz` (also the older
        // `_space_group_symop.magn_operation_xyz`), and `..._centering...`.
        let is_magn_op_loop = current_loop_headers
            .iter()
            .any(|h| h.contains("magn_operation") && h.contains("xyz"));
        let is_magn_centering_loop = current_loop_headers
            .iter()
            .any(|h| h.contains("magn_centering") && h.contains("xyz"));
        let is_moment_loop = current_loop_headers
            .iter()
            .any(|h| h.contains("_atom_site_moment") && h.contains("crystalaxis_x"));

        // _atom_type_* dictionary loop. Disambiguated from _atom_site_* by the
        // absence of any _atom_site_ headers — _atom_type_ loops describe
        // species, not coordinates.
        let is_atom_type_loop = !is_atom_loop
            && current_loop_headers
                .iter()
                .any(|h| h.contains("_atom_type_symbol"))
            && current_loop_headers
                .iter()
                .any(|h| h.contains("_atom_type_oxidation_number"));

        if is_magn_op_loop || is_magn_centering_loop {
            if let Some(op) = extract_symop_string(trimmed) {
                if is_magn_op_loop {
                    magn_ops.push(op);
                } else {
                    magn_centering.push(op);
                }
            }
        } else if is_moment_loop {
            if let Some((label, m)) = parse_moment_row(trimmed, &current_loop_headers) {
                site_moments.insert(label, m);
            }
        } else if is_sym_loop {
            if let Some(op) = extract_symop_string(trimmed) {
                symmetry_ops.push(op);
            }
        } else if is_atom_loop {
            if let Some(atom) = parse_atom_row(trimmed, &current_loop_headers) {
                if atom.occupancy < 0.99 {
                    partial_occupancy_sites += 1;
                }
                base_labels.push(row_label(trimmed, &current_loop_headers).unwrap_or_default());
                base_atoms.push(atom);
            }
        } else if is_atom_type_loop {
            if let Some((sym, ox)) =
                parse_atom_type_row(trimmed, &current_loop_headers)
            {
                atom_type_ox.insert(sym, ox);
            }
        }
    }

    // Apply _atom_type_oxidation_number fallback to atoms with no inline charge.
    if !atom_type_ox.is_empty() {
        for atom in base_atoms.iter_mut() {
            if atom.oxidation.is_some() {
                continue;
            }
            // Try a direct match against the bare element first; CIFs that use
            // this loop typically key it on the bare symbol ("Fe" → 3) and
            // leave _atom_site_type_symbol bare too.
            if let Some(&ox) = atom_type_ox.get(&atom.element) {
                atom.oxidation = Some(ox);
            }
        }
    }

    // --- Post-processing: symmetry ---
    // A magnetic file lists its operations (with time reversal) and its
    // centring translations separately; the full set is every centring
    // after every operation. They replace any non-magnetic operations.
    let magnetic = !magn_ops.is_empty();
    let ops: Vec<SymOp> = if magnetic {
        if magn_centering.is_empty() {
            magn_centering.push("x,y,z,+1".to_string());
        }
        let ops: Vec<SymOp> = magn_ops.iter().filter_map(|o| SymOp::parse(o)).collect();
        let cen: Vec<SymOp> = magn_centering.iter().filter_map(|o| SymOp::parse(o)).collect();
        cen.iter().flat_map(|c| ops.iter().map(move |o| c.after(o))).collect()
    } else {
        Vec::new()
    };
    if symmetry_ops.is_empty() {
        symmetry_ops.push("x,y,z".to_string());
    }

    // Bravais-centering fallback: if the H-M symbol starts with a centering
    // letter and the parsed ops don't already include the centering
    // translations, synthesize them. (Not for a magnetic file: its
    // centring loop is explicit, and may carry time reversal.)
    if let Some(hm) = hm_symbol.as_ref().filter(|_| !magnetic) {
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
    // Coincident sites carrying DIFFERENT elements (split-site disorder,
    // e.g. Fe/Cr sharing a position) are KEPT — with per-atom occupancy
    // now on the model, XRD/BVS weight them correctly. Dedup only removes
    // same-element duplicates (symmetry images of a special position).
    let mut split_site_pairs = 0usize;

    let ops: Vec<SymOp> = if magnetic {
        ops
    } else {
        symmetry_ops.iter().filter_map(|o| SymOp::parse(o)).collect()
    };
    let cell_lengths = [a, b, c];
    let mut sites_with_moment = 0usize;

    for (k, atom) in base_atoms.iter().enumerate() {
        // Moment in lattice-vector components: crystalaxis_i / |a_i|.
        let m_frac = base_labels
            .get(k)
            .and_then(|l| site_moments.get(l))
            .map(|m| [0, 1, 2].map(|i| if cell_lengths[i] > 0.0 { m[i] / cell_lengths[i] } else { 0.0 }));
        if m_frac.is_some() {
            sites_with_moment += 1;
        }
        for op in &ops {
            let new_pos = op.apply(atom.position);

            let wx = new_pos[0].rem_euclid(1.0);
            let wy = new_pos[1].rem_euclid(1.0);
            let wz = new_pos[2].rem_euclid(1.0);

            let mut is_duplicate = false;
            let mut coincident_other_element = false;
            for existing in &final_atoms {
                let dx = (existing.position[0] - wx).abs();
                let dy = (existing.position[1] - wy).abs();
                let dz = (existing.position[2] - wz).abs();
                let coincident = (dx < epsilon || (1.0 - dx) < epsilon)
                    && (dy < epsilon || (1.0 - dy) < epsilon)
                    && (dz < epsilon || (1.0 - dz) < epsilon);
                if coincident {
                    if existing.element == atom.element {
                        is_duplicate = true;
                        break;
                    }
                    coincident_other_element = true;
                }
            }

            if !is_duplicate {
                if coincident_other_element {
                    split_site_pairs += 1;
                }
                let idx = final_atoms.len();
                final_atoms.push(Atom {
                    element: atom.element.clone(),
                    position: [wx, wy, wz],
                    original_index: idx,
                    oxidation: atom.oxidation,
                    occupancy: atom.occupancy,
                    // Still in lattice-vector components; made Cartesian
                    // with the positions below.
                    magmom: m_frac.map(|m| op.apply_axial(m)),
                });
            }
        }
    }

    // --- Build lattice matrix and convert to Cartesian ---
    let lattice = crate::physics::operations::spacegroup_build::lattice_from_params([
        a, b, c, alpha, beta, gamma,
    ])
    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("CIF cell parameters: {e}")))?;

    for atom in &mut final_atoms {
        atom.position = frac_to_cart(atom.position, lattice);
        if let Some(m) = atom.magmom {
            atom.magmom = Some(frac_to_cart(m, lattice));
        }
    }

    if magnetic {
        crate::utils::console::log_info(&format!(
            "mCIF: {} magnetic operation(s) × {} centring(s); moments on {} of {} site(s) in the asymmetric unit",
            magn_ops.len(),
            magn_centering.len(),
            sites_with_moment,
            base_atoms.len()
        ));
    } else if !site_moments.is_empty() {
        crate::utils::console::log_warn(
            "CIF has _atom_site_moment but no magnetic symmetry operations; moments expanded with the ordinary operations (no time reversal)",
        );
    }

    if partial_occupancy_sites > 0 {
        crate::utils::console::log_info(&format!(
            "CIF: {partial_occupancy_sites} site(s) have occupancy < 1 — XRD form factors and BVS bond valences are occupancy-weighted (virtual-crystal approximation)"
        ));
    }
    if split_site_pairs > 0 {
        crate::utils::console::log_info(&format!(
            "CIF: {split_site_pairs} split-site position(s) kept with multiple species — contributions are occupancy-weighted"
        ));
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
    let stripped = line.replace(['\'', '"'], "");
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

/// A symmetry operation x' = W·x + t on fractional coordinates, with a
/// time-reversal flag (+1 or −1; +1 for non-magnetic operations).
#[derive(Clone, Copy, Debug)]
struct SymOp {
    w: [[f64; 3]; 3],
    t: [f64; 3],
    theta: f64,
}

impl SymOp {
    /// From "x,y,z" or the magnetic "x,y,z,+1" / "-x,y+1/2,z,-1".
    fn parse(op: &str) -> Option<Self> {
        let parts: Vec<&str> = op.split(',').map(str::trim).collect();
        let theta = match parts.len() {
            3 => 1.0,
            4 => match parts[3].trim_start_matches('+') {
                "1" => 1.0,
                "-1" => -1.0,
                _ => return None,
            },
            _ => return None,
        };
        let mut w = [[0.0; 3]; 3];
        let mut t = [0.0; 3];
        for i in 0..3 {
            t[i] = evaluate_expr(parts[i], [0.0; 3]);
            for j in 0..3 {
                let mut e = [0.0; 3];
                e[j] = 1.0;
                w[i][j] = evaluate_expr(parts[i], e) - t[i];
            }
        }
        Some(Self { w, t, theta })
    }

    fn rotate(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| (0..3).map(|j| self.w[i][j] * v[j]).sum())
    }

    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let r = self.rotate(p);
        [r[0] + self.t[0], r[1] + self.t[1], r[2] + self.t[2]]
    }

    /// An axial vector (magnetic moment) in lattice-vector components:
    /// m' = θ · det(W) · W · m. Translations do not act on it.
    fn apply_axial(&self, m: [f64; 3]) -> [f64; 3] {
        let w = &self.w;
        let det = w[0][0] * (w[1][1] * w[2][2] - w[1][2] * w[2][1])
            - w[0][1] * (w[1][0] * w[2][2] - w[1][2] * w[2][0])
            + w[0][2] * (w[1][0] * w[2][1] - w[1][1] * w[2][0]);
        self.rotate(m).map(|x| x * det * self.theta)
    }

    /// `self` applied after `first`.
    fn after(&self, first: &SymOp) -> SymOp {
        let mut w = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                w[i][j] = (0..3).map(|k| self.w[i][k] * first.w[k][j]).sum();
            }
        }
        SymOp { w, t: self.apply(first.t), theta: self.theta * first.theta }
    }
}

/// `_atom_site_label` of an atom-site row, to match moment rows against.
fn row_label(line: &str, headers: &[String]) -> Option<String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    headers
        .iter()
        .position(|h| h.trim() == "_atom_site_label")
        .and_then(|i| parts.get(i))
        .map(|s| s.to_string())
}

/// One `_atom_site_moment` row: (label, crystalaxis x/y/z in μB). Accepts
/// the DDLm (`_atom_site_moment.crystalaxis_x`) and older underscore tags.
fn parse_moment_row(line: &str, headers: &[String]) -> Option<(String, [f64; 3])> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let col = |suffix: &str| headers.iter().position(|h| h.trim().ends_with(suffix));
    let label = parts.get(col("label")?)?.to_string();
    let get = |s: &str| col(s).and_then(|i| parts.get(i)).map(|v| parse_cif_float(v));
    Some((label, [get("crystalaxis_x")?, get("crystalaxis_y")?, get("crystalaxis_z")?]))
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
    let mut occupancy: Option<f64> = None;

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
        } else if header.contains("_atom_site_occupancy") {
            // "0.5(2)" parses as 0.5; "." / "?" stay None (full occupancy).
            let cleaned: String = val.chars().take_while(|c| *c != '(').collect();
            occupancy = cleaned.parse::<f64>().ok();
        }
    }

    let (fx, fy, fz) = match (fx, fy, fz) {
        (Some(x), Some(y), Some(z)) => (x, y, z),
        _ => return None,
    };

    // IUCr rule: type_symbol takes precedence. Fall back to label component_0.
    // type_symbol carries the inline charge (e.g. "Fe3+"); label rarely does.
    let (element, oxidation) = if let Some(ts) = type_symbol_val {
        (normalize_element(ts), parse_oxidation_from_species(ts))
    } else if let Some(lbl) = label_val {
        (element_from_label(lbl), None)
    } else {
        ("X".to_string(), None)
    };

    Some(Atom {
        element,
        position: [fx, fy, fz],
        original_index: 0,
        oxidation,
        occupancy: occupancy.unwrap_or(1.0).clamp(0.0, 1.0),
        magmom: None,
    })
}

/// Parse one row of an `_atom_type_*` loop. Returns the (`_atom_type_symbol`,
/// `_atom_type_oxidation_number`) pair when both columns are present and the
/// oxidation column is an integer.
fn parse_atom_type_row(line: &str, headers: &[String]) -> Option<(String, i32)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let mut symbol: Option<&str> = None;
    let mut ox: Option<i32> = None;

    for (i, header) in headers.iter().enumerate() {
        if i >= parts.len() {
            break;
        }
        let val = parts[i];
        if header.contains("_atom_type_oxidation_number") {
            // Integer or integral-valued float (CIFs often write "2.0").
            let cleaned: String = val.chars().take_while(|c| *c != '(').collect();
            ox = cleaned.parse::<i32>().ok().or_else(|| {
                let f: f64 = cleaned.parse().ok()?;
                if (f - f.round()).abs() < 1e-6 {
                    Some(f.round() as i32)
                } else {
                    None
                }
            });
        } else if header.contains("_atom_type_symbol") {
            symbol = Some(val);
        }
    }

    match (symbol, ox) {
        (Some(s), Some(o)) => Some((s.to_string(), o)),
        _ => None,
    }
}

/// Extract the formal oxidation state from a CIF species string.
///
/// Accepts the canonical IUCr core-CIF form `Aa[n][s]` where `s ∈ {+, -}`
/// (e.g. `"Fe3+"`, `"O2-"`), the inverted form `Aa[s][n]` (`"Fe+3"`,
/// `"O-2"`), and parenthesized variants (`"Fe(3+)"`, `"Fe(+3)"`). A bare
/// sign with no digit is treated as ±1 (`"Cl-"` → −1).
///
/// Returns `None` for:
/// - no charge annotation,
/// - non-integer oxidation states (`"Co3.5+"` mixed-valence shorthand),
/// - Roman-numeral notation (`"Fe(III)"` — non-standard, locale-dependent),
/// - any string that doesn't match the patterns above.
fn parse_oxidation_from_species(s: &str) -> Option<i32> {
    // Strip whitespace and one optional pair of surrounding parentheses around
    // the charge segment. We handle "Fe(3+)" by removing parens around the
    // trailing token only.
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Drop the leading element letters (1–2 alphabetic chars).
    let mut chars = trimmed.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphabetic() {
            chars.next();
        } else {
            break;
        }
    }
    let tail: String = chars.collect();
    if tail.is_empty() {
        return None;
    }

    // Strip a single pair of surrounding parentheses if present: "(3+)" → "3+".
    let core = tail
        .strip_prefix('(')
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or(&tail);

    let bytes = core.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    // Locate the sign character.
    let (sign_idx, sign): (usize, i32) = match (
        bytes.iter().position(|&b| b == b'+'),
        bytes.iter().position(|&b| b == b'-'),
    ) {
        (Some(i), None) => (i, 1),
        (None, Some(i)) => (i, -1),
        // Both present, or neither: no parseable charge.
        _ => return None,
    };

    // Magnitude is whatever digits surround the sign. Try after-sign first
    // ("+3"), then before-sign ("3+"). Reject non-integer (decimal point).
    let after = &core[sign_idx + 1..];
    let before = &core[..sign_idx];

    let parse_uint = |t: &str| -> Option<i32> {
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        if t.contains('.') {
            return None; // mixed-valence shorthand like "3.5"
        }
        t.parse::<i32>().ok().filter(|n| *n >= 0)
    };

    let magnitude: i32 = match (parse_uint(after), parse_uint(before)) {
        (Some(n), _) => n,
        (None, Some(n)) => n,
        // Bare sign with no digit ("Cl-", "Na+") → ±1.
        (None, None) if before.is_empty() && after.is_empty() => 1,
        _ => return None,
    };

    Some(sign * magnitude)
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
    // `_atom_site_type_symbol` carries the formal charge (e.g. "Fe3+"). Without
    // it the oxidation state read from the source file is silently dropped on
    // export, and BVS falls back to a guessed valence on re-import.
    writeln!(file, " _atom_site_type_symbol")?;
    writeln!(file, " _atom_site_fract_x")?;
    writeln!(file, " _atom_site_fract_y")?;
    writeln!(file, " _atom_site_fract_z")?;
    writeln!(file, " _atom_site_occupancy")?;

    use crate::utils::linalg::cart_to_frac;

    for (i, atom) in structure.atoms.iter().enumerate() {
        let frac = cart_to_frac(atom.position, structure.lattice).unwrap_or([0.0, 0.0, 0.0]);
        let (u, v, w) = (frac[0], frac[1], frac[2]);
        // IUCr canonical form: symbol, magnitude, sign — "Fe3+", "O2-".
        let type_symbol = match atom.oxidation {
            Some(n) if n > 0 => format!("{}{}+", atom.element, n),
            Some(n) if n < 0 => format!("{}{}-", atom.element, -n),
            // An explicit neutral state (`_atom_type_oxidation_number 0`) is
            // written in the "0+" convention used by ICSD-derived CIFs. A bare
            // symbol would re-read as "unknown" rather than "known to be zero".
            Some(_) => format!("{}0+", atom.element),
            None => atom.element.clone(),
        };
        writeln!(
            file,
            " {}{} {} {:.6} {:.6} {:.6} {:.4}",
            atom.element,
            i + 1,
            type_symbol,
            u,
            v,
            w,
            atom.occupancy
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_oxidation_inline_charge_n_then_sign() {
        // Canonical IUCr form: element + magnitude + sign.
        assert_eq!(parse_oxidation_from_species("Fe3+"), Some(3));
        assert_eq!(parse_oxidation_from_species("O2-"), Some(-2));
        assert_eq!(parse_oxidation_from_species("Ti4+"), Some(4));
        assert_eq!(parse_oxidation_from_species("Ba2+"), Some(2));
    }

    #[test]
    fn parse_oxidation_inline_charge_sign_then_n() {
        // Inverted form some refinement programs emit.
        assert_eq!(parse_oxidation_from_species("Fe+3"), Some(3));
        assert_eq!(parse_oxidation_from_species("O-2"), Some(-2));
    }

    #[test]
    fn parse_oxidation_bare_sign_means_one() {
        assert_eq!(parse_oxidation_from_species("Cl-"), Some(-1));
        assert_eq!(parse_oxidation_from_species("Na+"), Some(1));
    }

    #[test]
    fn parse_oxidation_parens_tolerated() {
        assert_eq!(parse_oxidation_from_species("Fe(3+)"), Some(3));
        assert_eq!(parse_oxidation_from_species("O(2-)"), Some(-2));
    }

    #[test]
    fn parse_oxidation_none_for_bare_or_fractional() {
        assert_eq!(parse_oxidation_from_species("Fe"), None);
        assert_eq!(parse_oxidation_from_species("O"), None);
        // Mixed-valence shorthand isn't a single integer state.
        assert_eq!(parse_oxidation_from_species("Co3.5+"), None);
        // Roman numerals — non-standard, intentionally rejected.
        assert_eq!(parse_oxidation_from_species("Fe(III)"), None);
    }

    #[test]
    fn parses_batio3_with_inline_oxidation() {
        // The repo ships BaTiO3.cif with inline charges in
        // _atom_site_type_symbol. Verify the parser captures them.
        let s = parse("../BaTiO3.cif").or_else(|_| parse("BaTiO3.cif"));
        let s = match s {
            Ok(s) => s,
            Err(_) => return, // CIF not available in this run; test is opportunistic
        };
        // Every Ba/Ti/O in BaTiO3 should have an explicit oxidation state
        // because the file uses Ba2+ / Ti4+ / O2- in _atom_site_type_symbol.
        let mut seen_ba = false;
        let mut seen_ti = false;
        let mut seen_o = false;
        for atom in &s.atoms {
            match atom.element.as_str() {
                "Ba" => {
                    seen_ba = true;
                    assert_eq!(atom.oxidation, Some(2), "Ba expected +2");
                }
                "Ti" => {
                    seen_ti = true;
                    assert_eq!(atom.oxidation, Some(4), "Ti expected +4");
                }
                "O" => {
                    seen_o = true;
                    assert_eq!(atom.oxidation, Some(-2), "O expected -2");
                }
                _ => {}
            }
        }
        assert!(seen_ba && seen_ti && seen_o, "expected Ba/Ti/O atoms");
    }

    #[test]
    fn write_then_parse_preserves_oxidation_and_occupancy() {
        // Regression: the writer used to emit only `_atom_site_label`, so
        // oxidation state was silently dropped on export and BVS fell back to
        // a guessed valence after a CIF -> CIF round trip.
        use crate::model::{Atom, Structure};

        let mk = |el: &str, ox: Option<i32>, occ: f64, pos: [f64; 3]| Atom {
            element: el.to_string(),
            position: pos,
            original_index: 0,
            oxidation: ox,
            occupancy: occ,
            magmom: None,
        };
        let original = Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: vec![
                mk("Fe", Some(3), 1.0, [0.0, 0.0, 0.0]),
                mk("O", Some(-2), 0.5, [2.0, 2.0, 2.0]),
                mk("Na", Some(1), 1.0, [2.0, 0.0, 0.0]),
                mk("Si", None, 1.0, [0.0, 2.0, 0.0]),
                // Explicit neutral must stay distinguishable from unknown.
                mk("Co", Some(0), 1.0, [0.0, 0.0, 2.0]),
            ],
            formula: String::new(),
            is_periodic: true,
        };

        let mut path = std::env::temp_dir();
        path.push(format!("cview_cif_roundtrip_{}.cif", std::process::id()));
        let path_str = path.to_string_lossy().to_string();

        write(&path_str, &original).expect("write failed");
        let back = parse(&path_str).expect("re-parse failed");
        std::fs::remove_file(&path).ok();

        assert_eq!(back.atoms.len(), original.atoms.len());
        for (a, b) in original.atoms.iter().zip(back.atoms.iter()) {
            assert_eq!(a.element, b.element, "element changed");
            assert_eq!(a.oxidation, b.oxidation, "oxidation lost for {}", a.element);
            assert!(
                (a.occupancy - b.occupancy).abs() < 1e-4,
                "occupancy changed for {}",
                a.element
            );
        }
    }

    fn parse_text(name: &str, text: &str) -> Structure {
        let p = std::env::temp_dir().join(format!("cview_mcif_{}_{}.mcif", std::process::id(), name));
        std::fs::write(&p, text).unwrap();
        let s = parse(p.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&p);
        s
    }

    const MNF2: &str = r#"# MnF2, rutile, antiferromagnetic with moments along c.
# Magnetic space group P4_2'/mnm' (BNS 136.499). Mn at 2a; the operations
# that move (0,0,0) to (1/2,1/2,1/2) reverse the moment.
data_MnF2
_cell_length_a 4.8734
_cell_length_b 4.8734
_cell_length_c 3.3099
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
_space_group_magn.name_BNS "P4_2'/mnm'"
loop_
_space_group_symop_magn_operation.id
_space_group_symop_magn_operation.xyz
1 x,y,z,+1
2 -x,-y,z,+1
3 -y+1/2,x+1/2,z+1/2,-1
4 y+1/2,-x+1/2,z+1/2,-1
5 -x+1/2,y+1/2,-z+1/2,+1
6 x+1/2,-y+1/2,-z+1/2,+1
7 y,x,-z,-1
8 -y,-x,-z,-1
9 -x,-y,-z,+1
10 x,y,-z,+1
11 y+1/2,-x+1/2,-z+1/2,-1
12 -y+1/2,x+1/2,-z+1/2,-1
13 x+1/2,-y+1/2,z+1/2,+1
14 -x+1/2,y+1/2,z+1/2,+1
15 -y,-x,z,-1
16 y,x,z,-1
loop_
_space_group_symop_magn_centering.id
_space_group_symop_magn_centering.xyz
1 x,y,z,+1
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Mn1 Mn 0.0 0.0 0.0
F1 F 0.3050 0.3050 0.0
loop_
_atom_site_moment.label
_atom_site_moment.crystalaxis_x
_atom_site_moment.crystalaxis_y
_atom_site_moment.crystalaxis_z
Mn1 0.0 0.0 4.6
"#;

    #[test]
    fn mcif_antiferromagnet_time_reversal_flips_the_body_centre() {
        let s = parse_text("mnf2", MNF2);
        let mn: Vec<_> = s.atoms.iter().filter(|a| a.element == "Mn").collect();
        assert_eq!(mn.len(), 2);
        assert_eq!(s.atoms.iter().filter(|a| a.element == "F").count(), 4);
        let m: Vec<[f64; 3]> = mn.iter().map(|a| a.magmom.expect("Mn has a moment")).collect();
        assert!((m[0][2] - 4.6).abs() < 1e-9 && (m[1][2] + 4.6).abs() < 1e-9, "{:?}", m);
        assert!(s.atoms.iter().filter(|a| a.element == "F").all(|a| a.magmom.is_none()));
    }

    #[test]
    fn mcif_anti_translation_centring_and_crystalaxis_units() {
        // Cell doubled along a; the centring x+1/2 carries time reversal, so
        // the second Fe is reversed. The in-plane moment is given along b
        // (a unit vector), and must come out as 2 muB whatever |b| is.
        let text = "data_t
_cell_length_a 6.0
_cell_length_b 3.0
_cell_length_c 3.0
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_space_group_symop_magn_operation.id
_space_group_symop_magn_operation.xyz
1 x,y,z,+1
loop_
_space_group_symop_magn_centering.id
_space_group_symop_magn_centering.xyz
1 x,y,z,+1
2 x+1/2,y,z,-1
loop_
_atom_site_label
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
Fe1 0 0 0
loop_
_atom_site_moment.label
_atom_site_moment.crystalaxis_x
_atom_site_moment.crystalaxis_y
_atom_site_moment.crystalaxis_z
Fe1 0 2.0 0
";
        let s = parse_text("anti", text);
        assert_eq!(s.atoms.len(), 2);
        let close = |m: Option<[f64; 3]>, want: [f64; 3]| {
            let m = m.expect("moment");
            assert!((0..3).all(|k| (m[k] - want[k]).abs() < 1e-9), "{:?} vs {:?}", m, want);
        };
        close(s.atoms[0].magmom, [0.0, 2.0, 0.0]);
        close(s.atoms[1].magmom, [0.0, -2.0, 0.0]);
        assert!((s.atoms[1].position[0] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn mirror_reverses_an_axial_moment_along_its_normal_unless_primed() {
        // Axial vector under a mirror x -> -x: m_x kept, m_y and m_z reversed.
        let m = SymOp::parse("-x,y,z,+1").unwrap().apply_axial([1.0, 2.0, 3.0]);
        assert_eq!(m, [1.0, -2.0, -3.0]);
        let m = SymOp::parse("-x,y,z,-1").unwrap().apply_axial([1.0, 2.0, 3.0]);
        assert_eq!(m, [-1.0, 2.0, 3.0]);
        // Inversion leaves an axial vector alone.
        assert_eq!(SymOp::parse("-x,-y,-z").unwrap().apply_axial([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }
}
