// src/utils/report.rs

use crate::model::bvs::get_bvs_params;
use crate::model::structure::Structure;
use crate::physics::bond_valence::{
    calculate_bvs_pbc, calculate_structure_quality, get_ideal_oxidation_state, BVSQuality,
};
use crate::utils::geometry;
use std::collections::{HashMap, HashSet};

// ─── Structure summary ───────────────────────────────────────────────────────

pub fn structure_summary(structure: &Structure, filename: &str) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for atom in &structure.atoms {
        *counts.entry(atom.element.clone()).or_insert(0) += 1;
    }

    let mut parts: Vec<_> = counts.into_iter().collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));

    let formula_str: String = parts
        .iter()
        .map(|(el, n)| format!("{}{}", el, n))
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = String::new();
    out.push_str(&format!("File: {}\n", filename));
    out.push_str(&format!("Formula: {}\n", formula_str));
    out.push_str("--------------------------------------------------\n");
    out.push_str(&format!(
        "{:<8} {:<8} {:<10} {:<10} {:<10}\n",
        "Index", "Element", "X", "Y", "Z"
    ));
    out.push_str("--------------------------------------------------\n");

    for (i, atom) in structure.atoms.iter().take(20).enumerate() {
        out.push_str(&format!(
            "{:<8} {:<8} {:<10.4} {:<10.4} {:<10.4}\n",
            i, atom.element, atom.position[0], atom.position[1], atom.position[2]
        ));
    }
    if structure.atoms.len() > 20 {
        out.push_str(&format!(
            "... and {} more atoms.\n",
            structure.atoms.len() - 20
        ));
    }
    out
}

// ─── BVS analysis ────────────────────────────────────────────────────────────

pub fn bvs_analysis(structure: &Structure) -> String {
    let mut out = String::new();

    out.push_str("═══════════════════════════════════════════════════\n");
    out.push_str("           BOND VALENCE SUM ANALYSIS\n");
    out.push_str("═══════════════════════════════════════════════════\n\n");

    let (avg_dev, max_dev, count) = calculate_structure_quality(structure);
    let quality = BVSQuality::from_deviation(avg_dev);

    out.push_str(&format!("Validated atoms:   {}\n", count));
    out.push_str(&format!("Average deviation: {:.3} v.u.\n", avg_dev));
    out.push_str(&format!("Maximum deviation: {:.3} v.u.\n", max_dev));
    out.push_str(&format!(
        "Overall quality:   {} {}\n\n",
        quality.symbol(),
        quality.as_str()
    ));

    out.push_str("─────────────────────────────────────────────────────────────\n");
    out.push_str(&format!(
        "{:<6} {:<4} {:<10} {:<10} {:<10} {:<8} {:<6}\n",
        "Index", "Elem", "BVS Calc", "Expected", "Deviation", "Status", "Params"
    ));
    out.push_str("─────────────────────────────────────────────────────────────\n");

    let show_count = structure.atoms.len().min(30);

    for i in 0..show_count {
        let atom = &structure.atoms[i];
        let bvs_calc = calculate_bvs_pbc(structure, i);
        let bvs_ideal = get_ideal_oxidation_state(&atom.element);

        let deviation = if bvs_ideal > 0.1 {
            (bvs_calc - bvs_ideal).abs()
        } else {
            0.0
        };

        let status = if bvs_ideal < 0.1 {
            "–"
        } else if deviation < 0.15 {
            "✓ Good"
        } else if deviation < 0.40 {
            "⚠ Warn"
        } else {
            "✗ Poor"
        };

        let param_status = neighbor_param_source(structure, i);

        out.push_str(&format!(
            "{:<6} {:<4} {:<10.3} {:<10.3} {:<10.3} {:<8} {:<6}\n",
            i, atom.element, bvs_calc, bvs_ideal, deviation, status, param_status
        ));
    }

    if structure.atoms.len() > 30 {
        out.push_str(&format!(
            "... and {} more atoms.\n",
            structure.atoms.len() - 30
        ));
    }

    out.push_str("\nParams key: IUCr = experimental table  B&OK = Brese-O'Keeffe fallback\n");

    if matches!(quality, BVSQuality::Poor | BVSQuality::Acceptable) {
        out.push_str("\n⚠ RECOMMENDATIONS:\n");
        if avg_dev > 0.5 {
            out.push_str("• Structure may have incorrect atom positions\n");
            out.push_str("• Check if this is the asymmetric unit (needs full cell)\n");
            out.push_str("• Try 'View → Show Full Unit Cell' to include periodic images\n");
        } else {
            out.push_str("• Mild distortion — common in DFT-relaxed or experimental structures\n");
        }
        let bok_count = (0..structure.atoms.len())
            .filter(|&i| neighbor_param_source(structure, i) == "B&OK")
            .count();
        if bok_count > structure.atoms.len() / 2 {
            out.push_str(
                "• Many bonds use Brese-O'Keeffe fallback (less accurate than IUCr table)\n",
            );
        }
    }

    out.push_str("\n═══════════════════════════════════════════════════\n");
    out
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Anion / cation classification matching calculator.rs
fn is_anion(element: &str) -> bool {
    matches!(
        element,
        "O" | "S" | "Se" | "Te" | "F" | "Cl" | "Br" | "I" | "N" | "P" | "As" | "H"
    )
}

/// Returns "IUCr", "B&OK", or "n/a" for the closest valid bond of atom i.
/// IUCr = exact match in the bvparm2020 table.
/// B&OK = Brese-O'Keeffe empirical fallback.
///
/// Detection: call get_bvs_params with val=9 for both elements (forces B&OK).
/// If the real call's R0 differs from the val=9 result, the real call hit
/// the IUCr table.
fn neighbor_param_source(structure: &Structure, atom_idx: usize) -> &'static str {
    const CUTOFF_SQ: f64 = 6.0 * 6.0;

    let atom = &structure.atoms[atom_idx];
    let pos_a = atom.position;

    let mut best_dist_sq = f64::MAX;
    let mut best_src: &'static str = "n/a";

    for (j, neighbor) in structure.atoms.iter().enumerate() {
        if j == atom_idx {
            continue;
        }

        let pos_b = neighbor.position;
        let dx = pos_b[0] - pos_a[0];
        let dy = pos_b[1] - pos_a[1];
        let dz = pos_b[2] - pos_a[2];
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq > CUTOFF_SQ || dist_sq < 0.25 {
            continue;
        }

        // Determine cation/anion
        let (cation, anion) = if !is_anion(&atom.element) && is_anion(&neighbor.element) {
            (atom.element.as_str(), neighbor.element.as_str())
        } else if is_anion(&atom.element) && !is_anion(&neighbor.element) {
            (neighbor.element.as_str(), atom.element.as_str())
        } else {
            continue;
        };

        let src = param_source(cation, anion);
        if dist_sq < best_dist_sq && src != "n/a" {
            best_dist_sq = dist_sq;
            best_src = src;
        }
    }

    best_src
}

/// Distinguish IUCr table hit from B&OK fallback for a cation-anion pair.
fn param_source(cation: &str, anion: &str) -> &'static str {
    // Common anion valences
    let anion_vals: &[i32] = match anion {
        "O" | "S" | "Se" | "Te" => &[-2],
        "F" | "Cl" | "Br" | "I" => &[-1],
        "N" | "P" | "As" => &[-3],
        "H" => &[-1],
        _ => &[9],
    };

    // Try real valence combinations
    let cation_vals: &[i32] = match cation {
        "H" | "Li" | "Na" | "K" | "Rb" | "Cs" | "Ag" => &[1],
        "Cu" => &[2, 1],
        "Be" | "Mg" | "Ca" | "Sr" | "Ba" | "Ra" | "Zn" | "Cd" | "Hg" => &[2],
        "B" | "Al" | "Ga" | "In" | "Tl" | "La" | "Sc" | "Y" => &[3],
        "Si" | "Ge" | "Ti" | "Zr" | "Hf" | "Sn" | "Pb" | "C" | "Th" => &[4],
        "Nb" | "Ta" | "P" => &[5],
        "Mo" | "W" | "Cr" => &[6, 3],
        "Mn" => &[2, 3, 4],
        "Fe" => &[3, 2],
        "Co" | "Ni" | "Cu" => &[2, 3],
        "V" => &[5, 4, 3],
        _ => &[9],
    };

    // val=9 result is what B&OK produces
    let fallback = get_bvs_params(cation, 9, anion, 9);

    for &vc in cation_vals {
        for &va in anion_vals {
            if let Some(real) = get_bvs_params(cation, vc, anion, va) {
                return match fallback {
                    Some(f) if (real.r0 - f.r0).abs() > 1e-6 => "IUCr",
                    None => "IUCr",
                    _ => "B&OK",
                };
            }
        }
    }

    // Only val=9 available
    if fallback.is_some() {
        "B&OK"
    } else {
        "n/a"
    }
}

// ─── Geometry analysis ───────────────────────────────────────────────────────

pub fn geometry_analysis(structure: &Structure, selected_indices: &HashSet<usize>) -> String {
    let mut sel: Vec<usize> = selected_indices.iter().cloned().collect();
    sel.sort();

    if sel.is_empty() {
        return "Select atoms to measure.".to_string();
    }

    let mut out = String::new();
    out.push_str("Selection:\n");
    for (i, &idx) in sel.iter().enumerate() {
        if let Some(atom) = structure.atoms.get(idx) {
            if i > 0 {
                out.push_str(" - ");
            }
            out.push_str(&format!("Atom (#{}, {})", idx, atom.element));
        }
    }
    out.push_str("\n\n");

    match sel.len() {
        2 => {
            let p1 = structure.atoms[sel[0]].position;
            let p2 = structure.atoms[sel[1]].position;
            out.push_str(&format!(
                "Distance: {:.5} Å",
                geometry::calculate_distance(p1, p2)
            ));
        }
        3 => {
            let p1 = structure.atoms[sel[0]].position;
            let p2 = structure.atoms[sel[1]].position;
            let p3 = structure.atoms[sel[2]].position;
            out.push_str(&format!(
                "Angle (A-B-C): {:.2}°\n",
                geometry::calculate_angle(p1, p2, p3)
            ));
            out.push_str(&format!(
                "Dist (A-B):    {:.4} Å\n",
                geometry::calculate_distance(p1, p2)
            ));
            out.push_str(&format!(
                "Dist (B-C):    {:.4} Å",
                geometry::calculate_distance(p2, p3)
            ));
        }
        4 => {
            let p1 = structure.atoms[sel[0]].position;
            let p2 = structure.atoms[sel[1]].position;
            let p3 = structure.atoms[sel[2]].position;
            let p4 = structure.atoms[sel[3]].position;
            out.push_str(&format!(
                "Dihedral:      {:.2}°\n",
                geometry::calculate_dihedral(p1, p2, p3, p4)
            ));
            out.push_str(&format!(
                "Angle (A-B-C): {:.2}°",
                geometry::calculate_angle(p1, p2, p3)
            ));
        }
        _ => {
            out.push_str("Select 2-4 atoms for geometric calculations.");
        }
    }
    out
}

pub fn geometry_analysis_from_positions(selected_atoms: &[(usize, String, [f64; 3])]) -> String {
    if selected_atoms.is_empty() {
        return "Select atoms to measure.".to_string();
    }

    let mut out = String::new();
    out.push_str("Selection:\n");
    for (i, (uid, element, _)) in selected_atoms.iter().enumerate() {
        if i > 0 {
            out.push_str(" - ");
        }
        out.push_str(&format!("Atom (UID#{}, {})", uid, element));
    }
    out.push_str("\n\n");

    match selected_atoms.len() {
        2 => {
            let p1 = selected_atoms[0].2;
            let p2 = selected_atoms[1].2;
            out.push_str(&format!(
                "Distance: {:.5} Å",
                geometry::calculate_distance(p1, p2)
            ));
        }
        3 => {
            let p1 = selected_atoms[0].2;
            let p2 = selected_atoms[1].2;
            let p3 = selected_atoms[2].2;
            out.push_str(&format!(
                "Angle (A-B-C): {:.2}°\n",
                geometry::calculate_angle(p1, p2, p3)
            ));
            out.push_str(&format!(
                "Dist (A-B):    {:.4} Å\n",
                geometry::calculate_distance(p1, p2)
            ));
            out.push_str(&format!(
                "Dist (B-C):    {:.4} Å",
                geometry::calculate_distance(p2, p3)
            ));
        }
        4 => {
            let p1 = selected_atoms[0].2;
            let p2 = selected_atoms[1].2;
            let p3 = selected_atoms[2].2;
            let p4 = selected_atoms[3].2;
            out.push_str(&format!(
                "Dihedral:      {:.2}°\n",
                geometry::calculate_dihedral(p1, p2, p3, p4)
            ));
            out.push_str(&format!(
                "Angle (A-B-C): {:.2}°",
                geometry::calculate_angle(p1, p2, p3)
            ));
        }
        _ => {
            out.push_str("Select 2-4 atoms for geometric calculations.");
        }
    }
    out
}
