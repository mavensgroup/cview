// src/utils/report.rs

use crate::model::structure::Structure;
use crate::physics::bond_valence::{analyze_structure, BVSQuality};
use crate::rendering::polyhedra::{self, Polyhedron};
use crate::rendering::scene::RenderAtom;
use crate::state::{SelectedAtom, TabState};
use crate::utils::geometry;
use std::collections::HashMap;

// ─── Structure summary ───────────────────────────────────────────────────────

/// Summary shown when a file is loaded: `File: <name>`, formula, first atoms.
pub fn structure_summary(structure: &Structure, filename: &str) -> String {
  summary_with_header(structure, &format!("File: {}", filename))
}

/// Same summary after an operation replaced the structure (supercell, slab,
/// cell conversion...): the header names the operation instead of a file.
pub fn structure_summary_after(structure: &Structure, operation: &str) -> String {
  summary_with_header(structure, &format!("After: {}", operation))
}

fn summary_with_header(structure: &Structure, header: &str) -> String {
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
  out.push_str(&format!("{}\n", header));
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


// ─── Measurements ────────────────────────────────────────────────────────────

const PICK_LABELS: [&str; 4] = ["A", "B", "C", "D"];

/// Report for atoms picked on the canvas, read in pick order.
///
/// - 2 atoms: the distance, plus the shortest periodic distance when another
///   image of B is closer to A than the copy that was clicked.
/// - 3 atoms: the angle at B and all three distances.
/// - 4 atoms: the dihedral A-B-C-D, the angles at B and C, and the three
///   distances along the chain.
pub fn measurement_report(picked: &[&SelectedAtom], structure: Option<&Structure>) -> String {
  if picked.is_empty() {
    return "Selection cleared.".to_string();
  }

  let mut out = String::new();
  out.push_str("Selection (in pick order):\n");
  for (i, atom) in picked.iter().enumerate() {
    let label = PICK_LABELS.get(i).copied().unwrap_or("·");
    // A ghost copy sits one lattice vector away from the stored atom.
    let is_image = structure
      .and_then(|s| s.atoms.get(atom.original_index))
      .map(|a| geometry::calculate_distance(a.position, atom.cart_pos) > 1e-6)
      .unwrap_or(false);
    out.push_str(&format!(
      "  {label}  {:<3} #{}{}\n",
      atom.element,
      atom.original_index,
      if is_image { "  (periodic image)" } else { "" }
    ));
  }
  out.push('\n');

  let p: Vec<[f64; 3]> = picked.iter().map(|a| a.cart_pos).collect();
  let dist = |i: usize, j: usize| geometry::calculate_distance(p[i], p[j]);
  let angle = |i: usize, j: usize, k: usize| geometry::calculate_angle(p[i], p[j], p[k]);

  match p.len() {
    1 => out.push_str("Pick more atoms: 2 for a distance, 3 for an angle, 4 for a dihedral."),
    2 => {
      out.push_str(&format!("Distance A–B:     {:.4} Å", dist(0, 1)));
      let lattice = structure.filter(|s| s.is_periodic).map(|s| &s.lattice);
      if let Some((image, shift)) =
        lattice.and_then(|lat| geometry::closer_periodic_image(p[0], p[1], lat))
      {
        out.push_str(&format!(
          "\nShortest periodic: {:.4} Å  (B translated by [{} {} {}] cells)",
          geometry::calculate_distance(p[0], image),
          shift[0],
          shift[1],
          shift[2]
        ));
      }
    }
    3 => {
      out.push_str(&format!("Angle A-B-C:  {:.2}°\n", angle(0, 1, 2)));
      out.push_str(&format!("Dist  A–B:    {:.4} Å\n", dist(0, 1)));
      out.push_str(&format!("Dist  B–C:    {:.4} Å\n", dist(1, 2)));
      out.push_str(&format!("Dist  A–C:    {:.4} Å", dist(0, 2)));
    }
    4 => {
      out.push_str(&format!(
        "Dihedral A-B-C-D: {:.2}°\n",
        geometry::calculate_dihedral(p[0], p[1], p[2], p[3])
      ));
      out.push_str(&format!("Angle A-B-C:      {:.2}°\n", angle(0, 1, 2)));
      out.push_str(&format!("Angle B-C-D:      {:.2}°\n", angle(1, 2, 3)));
      out.push_str(&format!("Dist  A–B:        {:.4} Å\n", dist(0, 1)));
      out.push_str(&format!("Dist  B–C:        {:.4} Å\n", dist(1, 2)));
      out.push_str(&format!("Dist  C–D:        {:.4} Å", dist(2, 3)));
    }
    n => out.push_str(&format!(
      "{n} atoms selected — measurements use 2 to 4 atoms."
    )),
  }
  out
}


// ─── Polyhedra ───────────────────────────────────────────────────────────────

/// Composition of a polyhedron as a formula, e.g. "TiO6" (centre first).
fn polyhedron_formula(poly: &Polyhedron, atoms: &[RenderAtom]) -> String {
  let mut counts: Vec<(String, usize)> = Vec::new();
  for &i in &poly.neighbor_indices {
    let el = &atoms[i].element;
    match counts.iter_mut().find(|(e, _)| e == el) {
      Some((_, n)) => *n += 1,
      None => counts.push((el.clone(), 1)),
    }
  }
  counts.sort();
  let tail: String = counts
    .iter()
    .map(|(e, n)| if *n == 1 { e.clone() } else { format!("{e}{n}") })
    .collect();
  format!("{}{}", atoms[poly.center_idx].element, tail)
}

/// Metrics block for one polyhedron (shown when its centre atom is picked).
pub fn polyhedron_report(poly: &Polyhedron, atoms: &[RenderAtom]) -> String {
  let m = poly.metrics(atoms);
  let centre = &atoms[poly.center_idx];
  let ligand = {
    let mut els: Vec<&str> = poly
      .neighbor_indices
      .iter()
      .map(|&i| atoms[i].element.as_str())
      .collect();
    els.sort();
    els.dedup();
    if els.len() == 1 { els[0].to_string() } else { "X".to_string() }
  };

  let mut out = String::new();
  out.push_str(&format!(
    "Polyhedron {}  (centre {} #{})\n",
    polyhedron_formula(poly, atoms),
    centre.element,
    centre.original_index
  ));
  out.push_str(&format!("  Coordination number    {}\n", poly.coordination_number));
  out.push_str(&format!(
    "  Mean {}–{} distance    {:.3} Å  (range {:.3}–{:.3})\n",
    centre.element, ligand, m.mean_bond_length, m.bond_length_range.0, m.bond_length_range.1
  ));
  out.push_str(&format!("  Baur distortion Δ      {:.4}\n", m.baur_distortion));
  match m.quadratic_elongation {
    Some(l) => out.push_str(&format!("  Quadratic elongation   {:.4}\n", l)),
    None => out.push_str("  Quadratic elongation   n/a (no reference polyhedron for this CN)\n"),
  }
  match m.bond_angle_variance {
    Some(v) => out.push_str(&format!("  Bond-angle variance    {:.2} deg²\n", v)),
    None => out.push_str("  Bond-angle variance    n/a (no reference polyhedron for this CN)\n"),
  }
  out.push_str(&format!("  Volume                 {:.3} Å³", m.volume));
  out
}

/// Summary of every polyhedron in the cell: per formula, how many, their mean
/// metrics, and how they link to each other. None if polyhedra are not shown.
pub fn polyhedra_summary(tab: &TabState, atoms: &[RenderAtom]) -> Option<String> {
  let settings = tab.style.polyhedra_settings.as_ref().filter(|s| s.show_polyhedra)?;
  let polys = polyhedra::build_for_tab(atoms, tab)?;
  // One representative per physical polyhedron: the ones centred in the cell,
  // not their periodic images.
  let base: Vec<&Polyhedron> = polys
    .iter()
    .filter(|p| !atoms[p.center_idx].is_ghost && !atoms[p.center_idx].is_coord_only)
    .collect();
  if base.is_empty() {
    return Some("No polyhedra found with the current elements and bond range.".to_string());
  }

  // Group by formula.
  let mut groups: Vec<(String, Vec<&Polyhedron>)> = Vec::new();
  for p in &base {
    let f = polyhedron_formula(p, atoms);
    match groups.iter_mut().find(|(g, _)| *g == f) {
      Some((_, v)) => v.push(p),
      None => groups.push((f, vec![p])),
    }
  }
  groups.sort_by(|a, b| a.0.cmp(&b.0));

  let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
  let mut out = String::from("Polyhedra summary\n");
  out.push_str(&format!(
    "{:<10} {:>3} {:>4} {:>9} {:>8} {:>8} {:>9} {:>9}\n",
    "Polyhedron", "N", "CN", "⟨d⟩ (Å)", "Δ", "⟨λ⟩", "σ² (deg²)", "V (Å³)"
  ));
  let na = "—".to_string();
  for (f, ps) in &groups {
    let ms: Vec<_> = ps.iter().map(|p| p.metrics(atoms)).collect();
    let opt_mean = |sel: &dyn Fn(&polyhedra::PolyhedronMetrics) -> Option<f64>| {
      let v: Vec<f64> = ms.iter().filter_map(sel).collect();
      if v.is_empty() { na.clone() } else { format!("{:.3}", mean(&v)) }
    };
    out.push_str(&format!(
      "{:<10} {:>3} {:>4} {:>9.3} {:>8.4} {:>8} {:>9} {:>9.3}\n",
      f,
      ps.len(),
      ps[0].coordination_number,
      mean(&ms.iter().map(|m| m.mean_bond_length).collect::<Vec<_>>()),
      mean(&ms.iter().map(|m| m.baur_distortion).collect::<Vec<_>>()),
      opt_mean(&|m| m.quadratic_elongation),
      opt_mean(&|m| m.bond_angle_variance),
      mean(&ms.iter().map(|m| m.volume).collect::<Vec<_>>()),
    ));
  }

  out.push_str("\nLinkage (other polyhedra sharing 1 / 2 / 3+ vertices; mean per polyhedron)\n");
  // Asked of the structure's periodic images, not the scene: the scene only
  // holds images near the cell, so polyhedra on a cell face would miss neighbours.
  let images = tab
    .structure
    .as_ref()
    .map(polyhedra::periodic_atoms)
    .unwrap_or_default();
  let ctx = polyhedra::ConnectivityContext::new(
    &images,
    tab.view.bond_cutoff,
    settings.min_coordination,
    settings.max_bond_dist,
  );
  for (f, ps) in &groups {
    let conns: Vec<polyhedra::Connectivity> = ps
      .iter()
      .map(|p| {
        let verts: Vec<[f64; 3]> = p.neighbor_indices.iter().map(|&i| atoms[i].cart_pos).collect();
        ctx.of(atoms[p.center_idx].cart_pos, &verts, &settings.enabled_elements)
      })
      .collect();
    let avg = |sel: &dyn Fn(&polyhedra::Connectivity) -> usize| {
      conns.iter().map(|c| sel(c) as f64).sum::<f64>() / conns.len().max(1) as f64
    };
    out.push_str(&format!(
      "  {:<10} corner {:.1}   edge {:.1}   face {:.1}\n",
      f,
      avg(&|c| c.corner),
      avg(&|c| c.edge),
      avg(&|c| c.face)
    ));
  }
  out.push_str(
    "\nΔ: Baur (1974).  ⟨λ⟩, σ²: Robinson et al. (1971); — where no reference polyhedron exists.",
  );
  Some(out)
}
