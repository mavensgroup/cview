// src/utils/report.rs

use crate::model::structure::Structure;
use crate::physics::bond_valence::database::has_experimental_param;
use crate::physics::bond_valence::{
  calculate_bvs_pbc, calculate_structure_quality, get_ideal_oxidation_state, BVSQuality,
};
use crate::utils::geometry;
use std::collections::{HashMap, HashSet};

/// Generates the text for the "Interactions" tab when a file is loaded
pub fn structure_summary(structure: &Structure, filename: &str) -> String {
  let mut counts: HashMap<String, usize> = HashMap::new();
  for atom in &structure.atoms {
    *counts.entry(atom.element.clone()).or_insert(0) += 1;
  }

  let mut parts: Vec<_> = counts.into_iter().collect();
  parts.sort_by(|a, b| a.0.cmp(&b.0));

  let formula_str: String = parts
    .iter()
    .map(|(el, count)| format!("{}{}", el, count))
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

/// Generates Bond Valence Sum analysis for the Interactions tab
pub fn bvs_analysis(structure: &Structure) -> String {
  let mut out = String::new();

  out.push_str("═══════════════════════════════════════════════════\n");
  out.push_str("           BOND VALENCE SUM ANALYSIS\n");
  out.push_str("═══════════════════════════════════════════════════\n\n");

  // Overall quality assessment
  let (avg_dev, max_dev, count) = calculate_structure_quality(structure);
  let quality = BVSQuality::from_deviation(avg_dev);

  // out.push_str(&format!("Structure: {}\n", structure.formula));
  // out.push_str(&format!("Total atoms: {}\n", structure.atoms.len()));
  // out.push_str(&format!("Analyzed: {} atoms\n\n", count));

  // out.push_str(&format!(
  // "Overall Quality: {} {}\n",
  // quality.symbol(),
  // quality.as_str()
  // ));
  out.push_str(&format!("Average Deviation: {:.3}\n", avg_dev));
  out.push_str(&format!("Maximum Deviation: {:.3}\n\n", max_dev));

  // out.push_str("Color Guide:\n");
  // out.push_str("  🟢 Deviation < 0.15  → Excellent\n");
  // out.push_str("  🟡 Deviation < 0.40  → Acceptable\n");
  // out.push_str("  🔴 Deviation ≥ 0.40  → Poor\n\n");

  out.push_str("─────────────────────────────────────────────────────────────\n");
  out.push_str(&format!(
    "{:<6} {:<4} {:<10} {:<10} {:<10} {:<8} {:<8}\n",
    "Index", "Elem", "BVS Calc", "Expected", "Deviation", "Status", "Params"
  ));
  out.push_str("─────────────────────────────────────────────────────────────\n");

  // Calculate BVS for each atom (show first 30)
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
      "Unknown"
    } else if deviation < 0.15 {
      "✓ Good"
    } else if deviation < 0.40 {
      "⚠ Warn"
    } else {
      "✗ Poor"
    };

    // Check if we have experimental parameters for neighbors
    let has_exp_params = check_neighbor_params(structure, i, &atom.element);
    let param_status = if has_exp_params { "Exp" } else { "ML" };

    out.push_str(&format!(
      "{:<6} {:<4} {:<10.3} {:<10.3} {:<10.3} {:<8} {:<8}\n",
      i, atom.element, bvs_calc, bvs_ideal, deviation, status, param_status
    ));
  }

  if structure.atoms.len() > 30 {
    out.push_str(&format!(
      "... and {} more atoms.\n",
      structure.atoms.len() - 30
    ));
  }

  // out.push_str("─────────────────────────────────────────────────────────────\n\n");

  // Add interpretation guide
  // out.push_str("Interpretation:\n");
  // out.push_str("• BVS Calc: Sum of bond valences (should match oxidation state)\n");
  // out.push_str("• Expected: Ideal oxidation state for the element\n");
  // out.push_str("• Deviation: |BVS - Expected|\n");
  // out.push_str("• Params: Exp=Experimental database, ML=Predicted\n\n");

  // Add recommendations if structure is poor
  if matches!(quality, BVSQuality::Poor | BVSQuality::Acceptable) {
    out.push_str("⚠ RECOMMENDATIONS:\n");

    if avg_dev > 0.5 {
      out.push_str("• Structure may have incorrect atom positions\n");
      out.push_str("• Check if this is the asymmetric unit (needs full cell)\n");
      out.push_str("• Try 'View → Show Full Unit Cell' to include periodic images\n");
    } else if avg_dev > 0.25 {
      out.push_str("• Structure is acceptable but has some distortion\n");
      out.push_str("• This is common for DFT-relaxed or experimental structures\n");
    }

    // Check if using mostly ML predictions
    let ml_count = (0..structure.atoms.len())
      .filter(|&i| !check_neighbor_params(structure, i, &structure.atoms[i].element))
      .count();

    if ml_count > structure.atoms.len() / 2 {
      out.push_str("• Many bonds use ML-predicted parameters (less accurate)\n");
      out.push_str("• Consider structures with common elements (Li, Na, O, etc.)\n");
    }
  }

  out.push_str("\n═══════════════════════════════════════════════════\n");

  out
}

/// Helper: Check if atom has experimental BV parameters with neighbors
fn check_neighbor_params(structure: &Structure, atom_idx: usize, element: &str) -> bool {
  const CUTOFF: f64 = 4.0;

  let atom = &structure.atoms[atom_idx];
  let pos_a = atom.position;

  for (i, neighbor) in structure.atoms.iter().enumerate() {
    if i == atom_idx {
      continue;
    }

    let pos_b = neighbor.position;
    let dx = pos_b[0] - pos_a[0];
    let dy = pos_b[1] - pos_a[1];
    let dz = pos_b[2] - pos_a[2];
    let dist_sq = dx * dx + dy * dy + dz * dz;

    if dist_sq < CUTOFF * CUTOFF {
      if has_experimental_param(element, &neighbor.element) {
        return true;
      }
    }
  }

  false
}

/// Generates the text for the "Interactions" tab when atoms are selected
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
      let d = geometry::calculate_distance(p1, p2);
      out.push_str(&format!("Distance: {:.5} Å", d));
    }
    3 => {
      let p1 = structure.atoms[sel[0]].position;
      let p2 = structure.atoms[sel[1]].position;
      let p3 = structure.atoms[sel[2]].position;

      let angle = geometry::calculate_angle(p1, p2, p3);
      let d1 = geometry::calculate_distance(p1, p2);
      let d2 = geometry::calculate_distance(p2, p3);

      out.push_str(&format!("Angle (A-B-C): {:.2}°\n", angle));
      out.push_str(&format!("Dist (A-B):    {:.4} Å\n", d1));
      out.push_str(&format!("Dist (B-C):    {:.4} Å", d2));
    }
    4 => {
      let p1 = structure.atoms[sel[0]].position;
      let p2 = structure.atoms[sel[1]].position;
      let p3 = structure.atoms[sel[2]].position;
      let p4 = structure.atoms[sel[3]].position;

      let dihedral = geometry::calculate_dihedral(p1, p2, p3, p4);
      let angle = geometry::calculate_angle(p1, p2, p3);

      out.push_str(&format!("Dihedral:      {:.2}°\n", dihedral));
      out.push_str(&format!("Angle (A-B-C): {:.2}°", angle));
    }
    _ => {
      out.push_str("Select 2-4 atoms for geometric calculations.");
    }
  }
  out
}

/// NEW: Geometry analysis using actual 3D Cartesian positions from rendered atoms
/// This works correctly with periodic boundary conditions and symmetry
pub fn geometry_analysis_from_positions(
  selected_atoms: &[(usize, String, [f64; 3])], // (unique_id, element, position)
) -> String {
  if selected_atoms.is_empty() {
    return "Select atoms to measure.".to_string();
  }

  let mut out = String::new();
  out.push_str("Selection:\n");

  for (i, (uid, element, _pos)) in selected_atoms.iter().enumerate() {
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
      let d = geometry::calculate_distance(p1, p2);
      out.push_str(&format!("Distance: {:.5} Å", d));
    }
    3 => {
      let p1 = selected_atoms[0].2;
      let p2 = selected_atoms[1].2;
      let p3 = selected_atoms[2].2;

      let angle = geometry::calculate_angle(p1, p2, p3);
      let d1 = geometry::calculate_distance(p1, p2);
      let d2 = geometry::calculate_distance(p2, p3);

      out.push_str(&format!("Angle (A-B-C): {:.2}°\n", angle));
      out.push_str(&format!("Dist (A-B):    {:.4} Å\n", d1));
      out.push_str(&format!("Dist (B-C):    {:.4} Å", d2));
    }
    4 => {
      let p1 = selected_atoms[0].2;
      let p2 = selected_atoms[1].2;
      let p3 = selected_atoms[2].2;
      let p4 = selected_atoms[3].2;

      let dihedral = geometry::calculate_dihedral(p1, p2, p3, p4);
      let angle = geometry::calculate_angle(p1, p2, p3);

      out.push_str(&format!("Dihedral:      {:.2}°\n", dihedral));
      out.push_str(&format!("Angle (A-B-C): {:.2}°", angle));
    }
    _ => {
      out.push_str("Select 2-4 atoms for geometric calculations.");
    }
  }
  out
}
