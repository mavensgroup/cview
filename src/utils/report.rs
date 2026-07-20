// src/utils/report.rs

use crate::model::structure::Structure;
use crate::physics::bond_valence::{analyze_structure, BVSQuality};
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
  let r = analyze_structure(structure);
  // Quality banner is banded on the GII — that's what the literature bands
  // (Brown 2002: < 0.1 stable, > 0.2 strained) are defined on. Mean |Δ| is
  // still reported as a statistic below.
  let quality = BVSQuality::from_deviation(r.gii);
  let mut out = String::new();

  out.push_str("═══════════════════════════════════════════════════════════════\n");
  out.push_str("                  BOND VALENCE SUM ANALYSIS\n");
  out.push_str("═══════════════════════════════════════════════════════════════\n\n");

  out.push_str(&format!("Atoms:                {}\n", structure.atoms.len()));
  out.push_str(&format!("Validated:            {}\n", r.validated));
  out.push_str(&format!("Mean |Δ|:             {:.3} v.u.\n", r.mean_abs_dev));
  out.push_str(&format!("Max  |Δ|:             {:.3} v.u.\n", r.max_abs_dev));
  out.push_str(&format!(
    "GII (√⟨Δ²⟩):          {:.3} v.u.\n",
    r.gii
  ));
  out.push_str(&format!(
    "Overall quality:      {} {}\n\n",
    quality.symbol(),
    quality.as_str()
  ));

  // Tabulate atoms in worst-deviation-first order. Atoms with no expected
  // valence (V=0) sink to the bottom — they don't contribute to GII.
  let mut order: Vec<usize> = (0..r.atoms.len()).collect();
  order.sort_by(|&i, &j| {
    let ai = &r.atoms[i];
    let aj = &r.atoms[j];
    let key = |a: &crate::physics::bond_valence::AtomBVS| -> (u8, f64) {
      // Primary sort: known states first; secondary: |Δ| descending.
      let cls = if a.is_unknown() { 1 } else { 0 };
      (cls, -a.abs_deviation())
    };
    key(ai).partial_cmp(&key(aj)).unwrap_or(std::cmp::Ordering::Equal)
  });

  out.push_str("───────────────────────────────────────────────────────────────\n");
  out.push_str(&format!(
    "{:<5} {:<4} {:>4} {:>8} {:>8} {:>8} {:>4} {:<8} {:<6}\n",
    "Idx", "Elem", "Ox", "BVS", "Expect", "Δ", "CN", "Status", "Source"
  ));
  out.push_str("───────────────────────────────────────────────────────────────\n");

  // Cap at 50 worst entries — long tables are noise. The summary stats
  // above already capture the global picture.
  const MAX_ROWS: usize = 50;
  let shown = order.iter().copied().take(MAX_ROWS);

  for i in shown {
    let atom = &structure.atoms[i];
    let a = r.atoms[i];

    let ox_str = if a.is_unknown() {
      "?".to_string()
    } else {
      format!("{:+}", a.assumed_v)
    };

    let status = if a.is_unknown() {
      "–"
    } else {
      let d = a.abs_deviation();
      if d < 0.10 {
        "✓ Excel"
      } else if d < 0.20 {
        "✓ Good"
      } else if d < 0.40 {
        "⚠ Warn"
      } else {
        "✗ Poor"
      }
    };

    out.push_str(&format!(
      "{:<5} {:<4} {:>4} {:>8.3} {:>8.3} {:>+8.3} {:>4} {:<8} {:<6}\n",
      i,
      atom.element,
      ox_str,
      a.bvs,
      a.expected,
      a.deviation(),
      a.coordination,
      status,
      a.source.as_str()
    ));
  }

  if r.atoms.len() > MAX_ROWS {
    out.push_str(&format!(
      "… {} more atoms not shown (sorted by |Δ| descending).\n",
      r.atoms.len() - MAX_ROWS
    ));
  }

  out.push_str(
    "\nSource: IUCr = bvparm2020 exact   IUCr* = substituted valence   B&OK = O'Keeffe-Brese estimate\n",
  );
  out.push_str(
    "Δ      = signed deviation BVS − expected (positive = over-bonded)\n",
  );
  out.push_str(
    "CN     = coordination number (bonds with v_ij > 0.04 v.u.)\n",
  );
  out.push_str(
    "GII    = Global Instability Index √⟨Δ²⟩ over validated atoms\n",
  );

  // Factual notes about the calculation only — no interpretation of the
  // deviations. What a large Δ *means* is context-dependent (surface atoms
  // of a slab are legitimately under-bonded, ideal high-symmetry phases can
  // be genuinely strained, a bad refinement is simply wrong), and guessing
  // the cause helps one audience while misleading another. The numbers
  // speak; these notes only qualify how they were obtained.
  let mut notes: Vec<String> = Vec::new();
  {
    use crate::physics::bond_valence::ParamSource;
    let bok = r
      .atoms
      .iter()
      .filter(|a| matches!(a.source, ParamSource::BresOKeeffe))
      .count();
    if bok > 0 {
      notes.push(format!(
        "{bok} atom(s) use estimated (B&OK) parameters — R0 accurate to ~±0.05 Å (~15% in valence)"
      ));
    }
    let substituted = r
      .atoms
      .iter()
      .filter(|a| matches!(a.source, ParamSource::IucrSubstituted))
      .count();
    if substituted > 0 {
      notes.push(format!(
        "{substituted} atom(s) use IUCr parameters for a different valence of the same pair (IUCr*)"
      ));
    }
    let unknown = r.atoms.iter().filter(|a| a.is_unknown()).count();
    if unknown > 0 {
      notes.push(format!(
        "{unknown} atom(s) have no reference oxidation state and are excluded from GII"
      ));
    }
  }
  if !notes.is_empty() {
    out.push_str("\nNotes:\n");
    for n in &notes {
      out.push_str(&format!("• {n}\n"));
    }
  }

  out.push_str("\n═══════════════════════════════════════════════════════════════\n");
  out
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
