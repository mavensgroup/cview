// src/physics/bond_valence/calculator.rs
//
// Bond Valence Sum (BVS) implementation.
//
// Formula:  BVS_i = Σ_{j,images} exp((R₀ - R_ij) / B)
//
// Parameters R₀ and B:
//   1. IUCr bvparm2020.cif table  (1000+ oxidation-state-specific pairs)
//   2. Brese & O'Keeffe (1991) empirical fallback for untabulated pairs
//   Both live in model::bvs — no duplicate database here.
//
// PBC: loop over ALL periodic images within CUTOFF, not just minimum image.
// The minimum-image trick (round()) only finds one image per neighbor and
// is wrong for high-coordination sites like Ba in perovskite (CN=12).
//
// Valences: NOT hardcoded here. We try a sequence of plausible valences
// from the IUCr table (most-specific first, falling back to the val=9
// "average" sentinel already present in the table for 100+ element pairs).
//
// Known limitation: Mixed-valence systems (e.g. Fe₃O₄ with Fe²⁺/Fe³⁺)
// will use the same parameters for all sites of a given element because
// the first matching valence in the priority list wins. Resolving this
// requires either user-supplied oxidation states or iterative self-consistent
// assignment, which is outside the current scope.
//
// References:
//   Brown & Altermatt (1985) Acta Cryst. B41, 244-247
//   Brese & O'Keeffe (1991) Acta Cryst. B47, 192-197

use crate::model::bvs::get_bvs_params;
use crate::model::structure::Structure;
use nalgebra::{Matrix3, Vector3};

/// Bond cutoff (Å). 6 Å covers all common bonds including heavy-element
/// and high-CN sites. Contributions beyond 5 Å are < 0.001 v.u.
const CUTOFF: f64 = 6.0;

/// Guard against overlapping / erroneous atoms.
const MIN_DIST: f64 = 0.5;

/// Minimum lattice volume (ų) to consider a lattice valid for PBC.
/// Below this, the lattice is degenerate (e.g. all-zeros from an XYZ file).
const MIN_LATTICE_VOLUME: f64 = 0.01;

// ─── Lattice helpers ─────────────────────────────────────────────────────────

/// Build Matrix3 matching the convention in utils_linalg:
///   rows = lattice vectors  →  cart = mat.transpose() * frac
fn lattice_matrix(lat: [[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    )
}

/// Compute the signed volume of the unit cell (determinant of lattice matrix).
fn lattice_volume(lat_mat: &Matrix3<f64>) -> f64 {
    lat_mat.determinant().abs()
}

/// Image search range: smallest integer R such that a sphere of radius CUTOFF
/// is fully contained in the R-shell of periodic images.
/// ceil(CUTOFF / shortest_cell_length) + 1 safety margin for oblique cells.
fn image_range(lat_mat: &Matrix3<f64>) -> i32 {
    let len_a = lat_mat.row(0).norm();
    let len_b = lat_mat.row(1).norm();
    let len_c = lat_mat.row(2).norm();
    let shortest = len_a.min(len_b).min(len_c).max(0.1); // guard against zero
    (CUTOFF / shortest).ceil() as i32 + 1
}

// ─── Valence priority lists ───────────────────────────────────────────────────
//
// We do NOT store valences on atoms. Instead we try valences in priority order
// until the IUCr table (or B&OK fallback) returns a hit.
//
// val=9 is the IUCr "average/unspecified" sentinel — the table has 100+
// entries with val=9 covering common bonding situations. It always reaches
// the Brese-O'Keeffe fallback path in model::bvs for untabulated pairs.

fn anion_valences(element: &str) -> &'static [i32] {
    match element {
        "O" | "S" | "Se" | "Te" => &[-2, 9],
        "F" | "Cl" | "Br" | "I" => &[-1, 9],
        "N" | "P" | "As" => &[-3, 9],
        "H" => &[-1, 9],
        _ => &[9],
    }
}

fn cation_valences(element: &str) -> &'static [i32] {
    match element {
        "H" => &[1, 9],
        "Li" | "Na" | "K" | "Rb" | "Cs" => &[1, 9],
        "Ag" => &[1, 9],
        "Cu" => &[2, 1, 9],
        "Be" | "Mg" | "Ca" | "Sr" | "Ba" | "Ra" => &[2, 9],
        "Zn" | "Cd" | "Hg" => &[2, 9],
        "B" | "Al" | "Ga" | "In" => &[3, 9],
        "Tl" => &[3, 1, 9],
        "Si" | "Ge" => &[4, 9],
        "Sn" => &[4, 2, 9],
        "Pb" => &[4, 2, 9],
        "C" => &[4, 9],
        "Sb" | "Bi" => &[3, 5, 9],
        "As" => &[3, 5, 9],
        "P" => &[5, 3, 9],
        "N" => &[5, 3, 9],
        "La" | "Pr" | "Nd" | "Pm" | "Sm" | "Gd" | "Tb" | "Dy" | "Ho" | "Er" | "Tm" | "Lu"
        | "Sc" | "Y" => &[3, 9],
        "Ce" => &[4, 3, 9],
        "Eu" => &[3, 2, 9],
        "Yb" => &[3, 2, 9],
        "Th" => &[4, 9],
        "U" => &[4, 6, 5, 3, 9],
        "Pa" => &[5, 4, 9],
        "Np" | "Pu" | "Am" => &[4, 3, 9],
        "Ti" | "Zr" | "Hf" => &[4, 3, 9],
        "Nb" | "Ta" => &[5, 4, 9],
        "Mo" | "W" => &[6, 5, 4, 9],
        "Re" => &[7, 6, 4, 9],
        "Mn" => &[2, 3, 4, 7, 9],
        "Fe" => &[3, 2, 9],
        "Co" | "Ni" => &[2, 3, 9],
        "Cr" => &[3, 6, 9],
        "V" => &[5, 4, 3, 2, 9],
        "Ru" | "Os" => &[4, 3, 9],
        "Rh" | "Ir" => &[3, 4, 9],
        "Pd" | "Pt" => &[2, 4, 9],
        "Au" => &[3, 1, 9],
        _ => &[9],
    }
}

/// Is this element primarily an anion in ionic crystals?
fn is_anion(element: &str) -> bool {
    anion_valences(element)[0] < 0
}

/// Find the best BVS parameters for a bond between two elements.
/// Tries valence combinations in priority order; first hit wins.
///
/// BVS is defined for cation-anion bonds only. Anion-anion pairs (O-O, F-Cl, etc.)
/// are never counted — they are not ionic bonds and would massively inflate BVS
/// for anions (e.g., O in a perovskite would show BVS ~10 instead of ~2).
/// Cation-cation pairs use the val=9 "average" sentinel for intermetallic bonds.
fn best_params(elem_a: &str, elem_b: &str) -> Option<crate::model::bvs::BvsParams> {
    let a_anion = is_anion(elem_a);
    let b_anion = is_anion(elem_b);

    let (cation, anion) = match (a_anion, b_anion) {
        (false, true) => (elem_a, elem_b),
        (true, false) => (elem_b, elem_a),
        // Anion-anion: never a BVS bond
        (true, true) => return None,
        // Cation-cation: try val=9 both ways (intermetallics)
        (false, false) => {
            return get_bvs_params(elem_a, 9, elem_b, 9)
                .or_else(|| get_bvs_params(elem_b, 9, elem_a, 9));
        }
    };

    for &val_c in cation_valences(cation) {
        for &val_a in anion_valences(anion) {
            if let Some(p) = get_bvs_params(cation, val_c, anion, val_a) {
                return Some(p);
            }
        }
    }
    None
}

// ─── Core BVS calculation ─────────────────────────────────────────────────────

/// Calculate BVS for atom `atom_idx` with full periodic boundary conditions.
///
/// Iterates over ALL periodic images of each neighbor within CUTOFF.
/// This is the physically correct approach and gives consistent results
/// between a unit cell and any supercell of the same structure.
pub fn calculate_bvs_pbc(structure: &Structure, atom_idx: usize) -> f64 {
    let lat_mat = lattice_matrix(structure.lattice);

    // Guard: degenerate lattice → fall back to non-PBC
    if lattice_volume(&lat_mat) < MIN_LATTICE_VOLUME {
        return calculate_bvs(structure, atom_idx);
    }

    let inv_lat_t = match lat_mat.transpose().try_inverse() {
        Some(m) => m,
        None => return calculate_bvs(structure, atom_idx), // singular lattice → non-PBC fallback
    };

    let rng = image_range(&lat_mat);
    let atom = &structure.atoms[atom_idx];
    let pos_i = Vector3::from(atom.position);
    let frac_i = inv_lat_t * pos_i;

    let mut bvs = 0.0;

    for (j, neighbor) in structure.atoms.iter().enumerate() {
        let params = match best_params(&atom.element, &neighbor.element) {
            Some(p) => p,
            None => continue,
        };

        let frac_j = inv_lat_t * Vector3::from(neighbor.position);

        for nx in -rng..=rng {
            for ny in -rng..=rng {
                for nz in -rng..=rng {
                    if j == atom_idx && nx == 0 && ny == 0 && nz == 0 {
                        continue;
                    }
                    let img_frac = frac_j + Vector3::new(nx as f64, ny as f64, nz as f64);
                    let dist = (lat_mat.transpose() * (img_frac - frac_i)).norm();

                    if dist >= MIN_DIST && dist <= CUTOFF {
                        bvs += ((params.r0 - dist) / params.b).exp();
                    }
                }
            }
        }
    }

    bvs
}

/// Non-PBC BVS for isolated molecules / no meaningful lattice.
pub fn calculate_bvs(structure: &Structure, atom_idx: usize) -> f64 {
    let atom = &structure.atoms[atom_idx];
    let pos_i = Vector3::from(atom.position);
    let mut bvs = 0.0;

    for (j, neighbor) in structure.atoms.iter().enumerate() {
        if j == atom_idx {
            continue;
        }

        let params = match best_params(&atom.element, &neighbor.element) {
            Some(p) => p,
            None => continue,
        };

        let dist = (pos_i - Vector3::from(neighbor.position)).norm();
        if dist >= MIN_DIST && dist <= CUTOFF {
            bvs += ((params.r0 - dist) / params.b).exp();
        }
    }
    bvs
}

/// Automatic dispatch: uses PBC for periodic structures, direct sum for molecules.
/// This is the recommended entry point for all callers.
pub fn calculate_bvs_auto(structure: &Structure, atom_idx: usize) -> f64 {
    if structure.is_periodic {
        calculate_bvs_pbc(structure, atom_idx)
    } else {
        calculate_bvs(structure, atom_idx)
    }
}

// ─── Batch helpers ────────────────────────────────────────────────────────────

pub fn calculate_bvs_all(structure: &Structure) -> Vec<f64> {
    (0..structure.atoms.len())
        .map(|i| calculate_bvs(structure, i))
        .collect()
}

pub fn calculate_bvs_all_pbc(structure: &Structure) -> Vec<f64> {
    (0..structure.atoms.len())
        .map(|i| calculate_bvs_pbc(structure, i))
        .collect()
}

/// Automatic dispatch batch version — recommended entry point.
pub fn calculate_bvs_all_auto(structure: &Structure) -> Vec<f64> {
    (0..structure.atoms.len())
        .map(|i| calculate_bvs_auto(structure, i))
        .collect()
}

#[cfg(feature = "parallel")]
pub fn calculate_bvs_all_parallel(structure: &Structure) -> Vec<f64> {
    use rayon::prelude::*;
    (0..structure.atoms.len())
        .into_par_iter()
        .map(|i| calculate_bvs_auto(structure, i))
        .collect()
}

// ─── Quality metrics ──────────────────────────────────────────────────────────

/// Ideal oxidation state magnitude for coloring / quality checks.
/// Uses the first (most common) valence from the priority lists.
/// Returns 0.0 for elements where the state is genuinely ambiguous
/// (those with only val=9) — they don't distort the quality metric.
pub fn get_ideal_oxidation_state(element: &str) -> f64 {
    // Anion?
    let av = anion_valences(element)[0];
    if av < 0 {
        return av.unsigned_abs() as f64;
    }
    // Cation?
    let cv = cation_valences(element)[0];
    if cv > 0 && cv != 9 {
        return cv as f64;
    }
    0.0
}

/// BUG FIX: Was calling calculate_bvs (non-PBC) — now uses calculate_bvs_auto.
pub fn calculate_bvs_deviation(structure: &Structure, atom_idx: usize) -> f64 {
    let ideal = get_ideal_oxidation_state(&structure.atoms[atom_idx].element);
    if ideal < 0.1 {
        return 0.0;
    }
    (calculate_bvs_auto(structure, atom_idx) - ideal).abs()
}

/// (mean |ΔBVS|, max |ΔBVS|, n_atoms_with_known_state)
pub fn calculate_structure_quality(structure: &Structure) -> (f64, f64, usize) {
    let mut sum = 0.0_f64;
    let mut max = 0.0_f64;
    let mut count = 0_usize;

    for (i, atom) in structure.atoms.iter().enumerate() {
        let ideal = get_ideal_oxidation_state(&atom.element);
        if ideal < 0.1 {
            continue;
        }

        let dev = (calculate_bvs_auto(structure, i) - ideal).abs();
        sum += dev;
        if dev > max {
            max = dev;
        }
        count += 1;
    }

    let avg = if count > 0 { sum / count as f64 } else { 0.0 };
    (avg, max, count)
}

pub fn assess_structure_quality(structure: &Structure) -> BVSQuality {
    let (avg, _, _) = calculate_structure_quality(structure);
    BVSQuality::from_deviation(avg)
}

// ─── Quality enum ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BVSQuality {
    Excellent,  // avg deviation < 0.15
    Good,       // < 0.25
    Acceptable, // < 0.40
    Poor,       // ≥ 0.40
}

impl BVSQuality {
    pub fn from_deviation(d: f64) -> Self {
        if d < 0.15 {
            Self::Excellent
        } else if d < 0.25 {
            Self::Good
        } else if d < 0.40 {
            Self::Acceptable
        } else {
            Self::Poor
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Excellent => "Excellent",
            Self::Good => "Good",
            Self::Acceptable => "Acceptable",
            Self::Poor => "Poor",
        }
    }
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Excellent | Self::Good => "✓",
            Self::Acceptable => "⚠",
            Self::Poor => "✗",
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    fn atom(element: &str, pos: [f64; 3]) -> Atom {
        Atom {
            element: element.into(),
            position: pos,
            original_index: 0,
        }
    }

    /// BaTiO₃ — canonical BVS test. Ba has CN=12 requiring 4 images of each
    /// of the 3 O atoms. If minimum-image were used, Ba BVS would be ~0.69
    /// instead of ~2.0.
    #[test]
    fn test_batio3_unit_cell() {
        let a = 4.0_f64;
        let structure = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Ba", [0.0, 0.0, 0.0]),
                atom("Ti", [a / 2.0, a / 2.0, a / 2.0]),
                atom("O", [a / 2.0, a / 2.0, 0.0]),
                atom("O", [a / 2.0, 0.0, a / 2.0]),
                atom("O", [0.0, a / 2.0, a / 2.0]),
            ],
            formula: "BaTiO3".into(),
            is_periodic: true,
        };

        let bvs_ba = calculate_bvs_pbc(&structure, 0);
        let bvs_ti = calculate_bvs_pbc(&structure, 1);
        let bvs_o = calculate_bvs_pbc(&structure, 2);

        assert!(
            bvs_ba > 1.5 && bvs_ba < 3.5,
            "Ba BVS = {bvs_ba:.3}, expected ~2.0 (CN=12 needs all-images loop)"
        );
        assert!(
            bvs_ti > 2.5 && bvs_ti < 5.5,
            "Ti BVS = {bvs_ti:.3}, expected ~4.0 (CN=6)"
        );
        assert!(
            bvs_o > 1.0 && bvs_o < 3.0,
            "O BVS  = {bvs_o:.3}, expected ~2.0"
        );
    }

    /// Unit cell and supercell must give identical BVS. This was the bug
    /// visible in the screenshots: unit cell showed Ba=2.73, supercell 0.68.
    #[test]
    fn test_supercell_consistency() {
        let a = 4.0_f64;
        let uc = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Ba", [0.0, 0.0, 0.0]),
                atom("Ti", [a / 2.0, a / 2.0, a / 2.0]),
                atom("O", [a / 2.0, a / 2.0, 0.0]),
                atom("O", [a / 2.0, 0.0, a / 2.0]),
                atom("O", [0.0, a / 2.0, a / 2.0]),
            ],
            formula: "BaTiO3".into(),
            is_periodic: true,
        };
        // 1×2×1 supercell
        let sc = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, 2.0 * a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Ba", [0.0, 0.0, 0.0]),
                atom("Ti", [a / 2.0, a / 2.0, a / 2.0]),
                atom("O", [a / 2.0, a / 2.0, 0.0]),
                atom("O", [a / 2.0, 0.0, a / 2.0]),
                atom("O", [0.0, a / 2.0, a / 2.0]),
                atom("Ba", [0.0, a, 0.0]),
                atom("Ti", [a / 2.0, a + a / 2.0, a / 2.0]),
                atom("O", [a / 2.0, a + a / 2.0, 0.0]),
                atom("O", [a / 2.0, a, a / 2.0]),
                atom("O", [0.0, a + a / 2.0, a / 2.0]),
            ],
            formula: "BaTiO3".into(),
            is_periodic: true,
        };

        let bvs_uc = calculate_bvs_pbc(&uc, 0);
        let bvs_sc = calculate_bvs_pbc(&sc, 0);
        assert!(
            (bvs_uc - bvs_sc).abs() < 0.01,
            "UC Ba BVS={bvs_uc:.3} != SC Ba BVS={bvs_sc:.3}"
        );
    }

    /// Auto dispatch should use PBC for periodic, non-PBC for molecular.
    #[test]
    fn test_auto_dispatch() {
        let a = 4.0_f64;
        let periodic = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Ba", [0.0, 0.0, 0.0]),
                atom("Ti", [a / 2.0, a / 2.0, a / 2.0]),
                atom("O", [a / 2.0, a / 2.0, 0.0]),
                atom("O", [a / 2.0, 0.0, a / 2.0]),
                atom("O", [0.0, a / 2.0, a / 2.0]),
            ],
            formula: "BaTiO3".into(),
            is_periodic: true,
        };
        let molecular = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: periodic.atoms.clone(),
            formula: "BaTiO3".into(),
            is_periodic: false,
        };

        let bvs_periodic = calculate_bvs_auto(&periodic, 0);
        let bvs_molecular = calculate_bvs_auto(&molecular, 0);

        // Periodic Ba should have much higher BVS (CN=12 from images)
        // than molecular Ba (only direct neighbors in the box)
        assert!(
            bvs_periodic > bvs_molecular,
            "Periodic BVS ({bvs_periodic:.3}) should exceed molecular ({bvs_molecular:.3})"
        );
    }

    /// Degenerate lattice (zero volume) should gracefully fall back to non-PBC.
    #[test]
    fn test_degenerate_lattice_fallback() {
        let structure = Structure {
            lattice: [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            atoms: vec![atom("Na", [0.0, 0.0, 0.0]), atom("Cl", [2.82, 0.0, 0.0])],
            formula: "NaCl".into(),
            is_periodic: true, // claims periodic but lattice is degenerate
        };

        // Should not panic or return 0.0 — should fall back to non-PBC
        let bvs = calculate_bvs_pbc(&structure, 0);
        assert!(
            bvs > 0.0,
            "Degenerate lattice should fallback to non-PBC, got {bvs}"
        );
    }

    #[test]
    fn test_ideal_oxidation_states() {
        assert_eq!(get_ideal_oxidation_state("O"), 2.0);
        assert_eq!(get_ideal_oxidation_state("F"), 1.0);
        assert_eq!(get_ideal_oxidation_state("Ba"), 2.0);
        assert_eq!(get_ideal_oxidation_state("Ti"), 4.0);
        assert_eq!(get_ideal_oxidation_state("Al"), 3.0);
    }

    #[test]
    fn test_quality_thresholds() {
        assert_eq!(BVSQuality::from_deviation(0.10), BVSQuality::Excellent);
        assert_eq!(BVSQuality::from_deviation(0.20), BVSQuality::Good);
        assert_eq!(BVSQuality::from_deviation(0.35), BVSQuality::Acceptable);
        assert_eq!(BVSQuality::from_deviation(0.55), BVSQuality::Poor);
    }

    /// Anion-anion pairs must not produce BVS parameters.
    /// This was a critical bug: O-O bonds via the Brese-O'Keeffe fallback
    /// inflated O BVS from ~2.0 to ~9.7 in perovskites.
    #[test]
    fn test_anion_anion_rejected() {
        assert!(best_params("O", "O").is_none(), "O-O must return None");
        assert!(best_params("F", "Cl").is_none(), "F-Cl must return None");
        assert!(best_params("S", "O").is_none(), "S-O must return None");
        assert!(best_params("N", "F").is_none(), "N-F must return None");
    }

    /// Cation-anion pairs must produce parameters.
    #[test]
    fn test_cation_anion_accepted() {
        assert!(best_params("Ba", "O").is_some(), "Ba-O must return Some");
        assert!(best_params("Ti", "O").is_some(), "Ti-O must return Some");
        assert!(best_params("Na", "Cl").is_some(), "Na-Cl must return Some");
    }

    /// BaTiO3 oxygen BVS must be close to 2.0, not inflated by O-O bonds.
    /// Before the anion-anion fix, O showed BVS ~9.7 due to spurious O-O contributions.
    #[test]
    fn test_oxygen_bvs_not_inflated() {
        let a = 4.008_f64;
        let structure = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Ba", [a / 2.0, a / 2.0, a / 2.0]),
                atom("Ti", [0.0, 0.0, 0.0]),
                atom("O", [0.0, 0.0, a / 2.0]),
                atom("O", [a / 2.0, 0.0, 0.0]),
                atom("O", [0.0, a / 2.0, 0.0]),
            ],
            formula: "BaTiO3".into(),
            is_periodic: true,
        };

        let bvs_o = calculate_bvs_pbc(&structure, 2);
        assert!(
            bvs_o > 1.0 && bvs_o < 3.0,
            "O BVS = {bvs_o:.3}, expected ~2.0 (must not include O-O bonds)"
        );
    }
}
