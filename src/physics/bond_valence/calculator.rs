// src/physics/bond_valence/calculator.rs
//
// Bond Valence Sum (BVS) — rigorous, parallel implementation.
//
// ## Theory
//
// For each cation–anion pair (i, j) with separation R_ij, the bond valence is
//
//     v_ij = exp((R0 - R_ij) / B)
//
// summed (i over all neighbors j of an atom, plus all periodic images of j
// within CUTOFF). The atom's BVS is Σ v_ij. For a chemically sound
// assignment, BVS ≈ |formal valence| of the atom.
//
// Quality of fit:
//   • per-atom deviation  Δ_i = BVS_i − V_i
//   • Global Instability Index  GII = sqrt(<Δ_i²>) over atoms with known V_i
//
// Parameters R0, B come from:
//   1. The IUCr bvparm2020.cif table (1000+ explicit cation/anion pairs);
//   2. Brese & O'Keeffe (1991) χ-and-r empirical fallback when the pair is
//      not tabulated.
// Both live in `model::bvs` — no duplicate database here.
//
// ## What this implementation guarantees
//
// 1. **PBC done right.** Image enumeration uses the perpendicular spacing
//    of each lattice direction (`d_i = V/|a_j × a_k|`) so even highly
//    oblique cells include every image within CUTOFF — not just the
//    direct-lattice-length approximation that under-counts for monoclinic
//    or triclinic cells.
//
// 2. **Per-atom oxidation state honoured.** When a parser supplied
//    `Atom.oxidation`, it overrides every priority-list guess. This is the
//    fix for mixed-valence systems like Fe₃O₄ where a CIF distinguishes
//    Fe²⁺ and Fe³⁺ sites explicitly.
//
// 3. **Amphoteric H by environment.** For H atoms with no explicit
//    oxidation, the role (proton vs hydride) is inferred from the nearest
//    non-H neighbor's electronegativity. So H₂O gives H⁺, NaH gives H⁻,
//    automatically.
//
// 4. **Cation–cation rejection.** BVS is defined for ionic bonds. Two
//    cations are not counted; the val=9 sentinel is *only* used for the
//    Brese-O'Keeffe fallback on a real cation–anion pair.
//
// 5. **Pair-parameter cache.** The pair lookup (a 1000-arm match in the
//    IUCr table) runs once per (cation_el, val_c, anion_el, val_a) tuple
//    instead of once per neighbor evaluation. Hot loop reads from a
//    HashMap.
//
// 6. **Rayon-parallel** at the per-atom level. Each atom's image-pair
//    summation is independent; the work distributes well even for small
//    cells with many images.
//
// 7. **Coordination number** computed alongside BVS using Brown's
//    convention: count bonds with v_ij > 0.04 v.u.
//
// ## What this does *not* do
//
// - **Self-consistent valence assignment** for mixed-valence systems
//   without explicit oxidation states. The priority list still wins ties.
//   The proper fix is parser-side: preserve explicit charges from the CIF.
//   For systems where that fails, the user can edit the structure to set
//   per-site oxidation explicitly.
//
// References:
//   I.D. Brown & D. Altermatt, Acta Cryst. B41 (1985) 244–247.
//   N.E. Brese & M. O'Keeffe, Acta Cryst. B47 (1991) 192–197.
//   I.D. Brown, "The Chemical Bond in Inorganic Chemistry: The Bond Valence
//   Model", IUCr Monograph 12, OUP, 2002.

use crate::model::bvs::{get_bvs_params, BvsParams};
use crate::model::elements::get_electronegativity;
use crate::model::structure::Structure;
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;
use std::collections::HashMap;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Cutoff for image-pair evaluation (Å). Above ~6 Å, exp((R0-R)/B) for any
/// realistic R0 < 3 Å is < 1e-3 v.u. — a negligible contribution.
pub const CUTOFF: f64 = 6.0;

/// Distance below which two atoms are considered overlapping; pair excluded.
pub const MIN_DIST: f64 = 0.5;

/// Minimum unit-cell volume (Å³) accepted as a valid lattice for PBC.
/// Below this we treat the structure as molecular and skip image enumeration.
pub const MIN_LATTICE_VOLUME: f64 = 0.01;

/// Bond-valence threshold (v.u.) for counting a contribution toward CN.
/// Brown 2002 convention: contributions below this are below table noise.
pub const BOND_VALENCE_THRESHOLD: f64 = 0.04;

/// Pauling electronegativity of H. Used only to classify amphoteric H.
const H_ELECTRONEGATIVITY: f64 = 2.20;

// ─── Public output types ──────────────────────────────────────────────────────

/// Per-atom BVS analysis result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtomBVS {
    /// Calculated bond-valence sum, v.u.
    pub bvs: f64,
    /// Working oxidation state assigned to this atom for the calculation.
    /// 0 means "unknown / no chemically sound state available".
    pub assumed_v: i32,
    /// Expected magnitude (|assumed_v| as f64). 0.0 when assumed_v is 0.
    pub expected: f64,
    /// Coordination number (count of bonds with v_ij > BOND_VALENCE_THRESHOLD).
    pub coordination: usize,
    /// Best parameter source seen across this atom's bonds.
    pub source: ParamSource,
}

/// Source of the parameters used for an atom's strongest bond.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamSource {
    /// Tabulated IUCr bvparm2020 entry — most accurate.
    Iucr,
    /// Brese-O'Keeffe empirical fallback.
    BresOKeeffe,
    /// No bonds matched (atom was isolated, or pair couldn't be classified).
    NotApplicable,
}

impl ParamSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParamSource::Iucr => "IUCr",
            ParamSource::BresOKeeffe => "B&OK",
            ParamSource::NotApplicable => "n/a",
        }
    }
}

impl AtomBVS {
    /// Signed deviation BVS - expected. Positive = over-bonded.
    pub fn deviation(&self) -> f64 {
        if self.expected > 0.0 {
            self.bvs - self.expected
        } else {
            0.0
        }
    }
    /// Magnitude |Δ| — comparable across atoms.
    pub fn abs_deviation(&self) -> f64 {
        self.deviation().abs()
    }
    /// True when the assumed valence was zero (no chemistry-grade ideal known).
    pub fn is_unknown(&self) -> bool {
        self.assumed_v == 0
    }
}

/// Aggregated structure-level BVS analysis.
#[derive(Debug, Clone)]
pub struct StructureBVS {
    /// Per-atom result, parallel to `Structure.atoms`.
    pub atoms: Vec<AtomBVS>,
    /// Global Instability Index = sqrt(<Δ²>) over atoms with known states.
    pub gii: f64,
    /// Mean |Δ| over atoms with known states.
    pub mean_abs_dev: f64,
    /// Maximum |Δ| over atoms with known states.
    pub max_abs_dev: f64,
    /// Number of atoms whose deviation contributed to the metrics.
    pub validated: usize,
}

// ─── Quality enum (kept for compatibility) ───────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BVSQuality {
    /// Well-determined ionic structure (literature: < 0.10 v.u.).
    Excellent,
    /// Acceptable refinement (< 0.20 v.u.).
    Good,
    /// Strained or under-constrained (< 0.40 v.u.).
    Acceptable,
    /// Indicates wrong oxidation states, missing atoms, or bad positions.
    Poor,
}

impl BVSQuality {
    /// Bands tightened to literature norms (Brown 2002, Salinas-Sanchez 1992).
    pub fn from_deviation(d: f64) -> Self {
        if d < 0.10 {
            Self::Excellent
        } else if d < 0.20 {
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

// ─── Lattice / image-range helpers ────────────────────────────────────────────

fn lattice_matrix(lat: [[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    )
}

fn cell_volume(lat_mat: &Matrix3<f64>) -> f64 {
    lat_mat.determinant().abs()
}

/// Required image-search range for each lattice direction.
///
/// The perpendicular spacing of lattice planes normal to direction `i` is
/// `d_i = V / |a_j × a_k|`. To cover every image whose Cartesian distance
/// to the home cell is ≤ CUTOFF, we need `n_i = ⌈CUTOFF / d_i⌉` images on
/// each side. This is rigorous for any cell shape — including the
/// monoclinic and triclinic cases where the simpler `CUTOFF / |a_i|` bound
/// can under-count.
fn image_ranges(lat_mat: &Matrix3<f64>) -> [i32; 3] {
    let a = lat_mat.row(0).transpose();
    let b = lat_mat.row(1).transpose();
    let c = lat_mat.row(2).transpose();
    let v = cell_volume(lat_mat).max(1e-12);

    let d_a = v / b.cross(&c).norm().max(1e-12);
    let d_b = v / c.cross(&a).norm().max(1e-12);
    let d_c = v / a.cross(&b).norm().max(1e-12);

    [
        (CUTOFF / d_a).ceil() as i32,
        (CUTOFF / d_b).ceil() as i32,
        (CUTOFF / d_c).ceil() as i32,
    ]
}

// ─── Valence resolution ───────────────────────────────────────────────────────

/// Plausible anion valences in priority order (ends with the val=9 sentinel
/// so the IUCr "average/unspecified" entries and the Brese-O'Keeffe
/// fallback both remain reachable).
fn anion_valences(element: &str) -> &'static [i32] {
    match element {
        "O" | "S" | "Se" | "Te" => &[-2, 9],
        "F" | "Cl" | "Br" | "I" => &[-1, 9],
        "N" | "P" | "As" => &[-3, 9],
        "H" => &[-1, 9],
        _ => &[9],
    }
}

/// Plausible cation valences in priority order.
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

/// Element's *primary* role under the priority lists. Used only as a tiebreaker
/// and for `is_anion`-style classification when explicit oxidation is missing.
fn primary_role(element: &str) -> Role {
    let av = anion_valences(element)[0];
    if av < 0 {
        return Role::Anion;
    }
    let cv = cation_valences(element)[0];
    if cv > 0 && cv != 9 {
        return Role::Cation;
    }
    Role::Ambiguous
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
    Cation,
    Anion,
    Ambiguous,
}

/// Classify an amphoteric H atom by its nearest non-H neighbor's
/// electronegativity. Falls back to anion (hydride) if no neighbor found.
fn classify_h(structure: &Structure, atom_idx: usize) -> i32 {
    let lat = lattice_matrix(structure.lattice);
    let pbc = structure.is_periodic && cell_volume(&lat) >= MIN_LATTICE_VOLUME;

    let p_h = Vector3::from(structure.atoms[atom_idx].position);
    let inv_lt = lat.transpose().try_inverse();
    let ranges = image_ranges(&lat);

    let mut best_d2 = f64::MAX;
    let mut best_chi = f64::NAN;

    for (j, neighbor) in structure.atoms.iter().enumerate() {
        if j == atom_idx || neighbor.element == "H" {
            continue;
        }
        let p_j = Vector3::from(neighbor.position);
        let chi_j = get_electronegativity(&neighbor.element);
        if chi_j <= 0.0 {
            continue;
        }

        let d2_min = if pbc && inv_lt.is_some() {
            let inv = inv_lt.unwrap();
            let frac_j = inv * p_j;
            let frac_i = inv * p_h;
            let mut min2 = f64::MAX;
            for nx in -ranges[0]..=ranges[0] {
                for ny in -ranges[1]..=ranges[1] {
                    for nz in -ranges[2]..=ranges[2] {
                        let img =
                            frac_j + Vector3::new(nx as f64, ny as f64, nz as f64);
                        let d2 = (lat.transpose() * (img - frac_i)).norm_squared();
                        if d2 < min2 {
                            min2 = d2;
                        }
                    }
                }
            }
            min2
        } else {
            (p_h - p_j).norm_squared()
        };

        if d2_min < best_d2 {
            best_d2 = d2_min;
            best_chi = chi_j;
        }
    }

    if best_chi.is_nan() {
        // No non-H neighbor found — assume hydride.
        return -1;
    }
    if best_chi > H_ELECTRONEGATIVITY {
        1 // proton: bonded to a more electronegative atom (O, F, N, …)
    } else {
        -1 // hydride: bonded to a metal
    }
}

/// Resolve every atom's working valence:
///
/// 1. Explicit `Atom.oxidation` from the parser → use as-is.
/// 2. H without an explicit state → classify by neighbor electronegativity.
/// 3. Anything else → first entry of the cation/anion priority list (the
///    val=9 sentinel becomes 0 here, meaning "unknown ideal").
fn resolve_valences(structure: &Structure) -> Vec<i32> {
    structure
        .atoms
        .iter()
        .enumerate()
        .map(|(i, atom)| {
            if let Some(v) = atom.oxidation {
                return v;
            }
            if atom.element == "H" {
                return classify_h(structure, i);
            }
            match primary_role(&atom.element) {
                Role::Anion => anion_valences(&atom.element)[0],
                Role::Cation => cation_valences(&atom.element)[0],
                Role::Ambiguous => 0,
            }
        })
        .collect()
}

// ─── Pair-parameter resolution & cache ───────────────────────────────────────

/// Resolve parameters for a directed (cation, anion) bond given working
/// valences. Tries the explicit valences first; if the IUCr table doesn't
/// contain that exact pair, walks the priority lists; finally falls through
/// to the val=9 Brese-O'Keeffe fallback inside `get_bvs_params`.
fn resolve_pair_params(
    cation: &str,
    val_c: i32,
    anion: &str,
    val_a: i32,
) -> Option<BvsParams> {
    // 1. Exact charges given.
    if let Some(p) = get_bvs_params(cation, val_c, anion, val_a) {
        return Some(p);
    }

    // 2. Priority list for cation, anion held fixed.
    if val_a != 9 {
        for &v in cation_valences(cation) {
            if v == val_c {
                continue;
            }
            if let Some(p) = get_bvs_params(cation, v, anion, val_a) {
                return Some(p);
            }
        }
    }

    // 3. Priority list for anion, cation held fixed.
    if val_c != 9 {
        for &v in anion_valences(anion) {
            if v == val_a {
                continue;
            }
            if let Some(p) = get_bvs_params(cation, val_c, anion, v) {
                return Some(p);
            }
        }
    }

    // 4. Both lists.
    for &vc in cation_valences(cation) {
        for &va in anion_valences(anion) {
            if let Some(p) = get_bvs_params(cation, vc, anion, va) {
                return Some(p);
            }
        }
    }

    // 5. Brese-O'Keeffe fallback (val=9 both).
    get_bvs_params(cation, 9, anion, 9)
}

/// Cache key: the resolved-valence-tagged pair as the calculator sees it.
type PairKey = (String, i32, String, i32);

/// Pair lookup result with a flag for whether IUCr returned an exact hit
/// (used to drive the "Source" column in the report).
#[derive(Clone, Copy)]
struct PairEntry {
    params: Option<BvsParams>,
    is_iucr: bool,
}

/// Pre-compute parameters for every distinct (cation, val_c, anion, val_a)
/// tuple actually used in the structure. The hot loop reads from this map.
fn build_pair_cache(
    structure: &Structure,
    valences: &[i32],
) -> HashMap<PairKey, PairEntry> {
    // Distinct (element, valence) tags actually present.
    let mut species: Vec<(String, i32)> = structure
        .atoms
        .iter()
        .zip(valences.iter())
        .map(|(a, &v)| (a.element.clone(), v))
        .collect();
    species.sort();
    species.dedup();

    // Iterate over directed (cation, anion) pairs only.
    let mut cache: HashMap<PairKey, PairEntry> = HashMap::new();
    for (i, (el_a, v_a)) in species.iter().enumerate() {
        for (j, (el_b, v_b)) in species.iter().enumerate() {
            if i == j {
                continue;
            }
            // Cation–cation, anion–anion, and ambiguous–anything pairs are
            // not BVS bonds. Ambiguous (v=0) is treated as "no bond".
            let (c, vc, a, va) = match (v_a.signum(), v_b.signum()) {
                (1, -1) => (el_a.as_str(), *v_a, el_b.as_str(), *v_b),
                (-1, 1) => (el_b.as_str(), *v_b, el_a.as_str(), *v_a),
                _ => continue,
            };
            let key = (c.to_string(), vc, a.to_string(), va);
            if cache.contains_key(&key) {
                continue;
            }

            // IUCr exact-match probe: the val=9 sentinel routes through
            // the Brese-O'Keeffe path. Anything else is an IUCr table hit.
            let exact = get_bvs_params(c, vc, a, va);
            let params = exact.or_else(|| resolve_pair_params(c, vc, a, va));
            let is_iucr = exact.is_some() && (vc != 9 || va != 9);

            cache.insert(key, PairEntry { params, is_iucr });
        }
    }
    cache
}

// ─── Core BVS computation ─────────────────────────────────────────────────────

/// Single-atom BVS with full PBC, returning rich per-atom stats.
fn analyze_atom(
    structure: &Structure,
    atom_idx: usize,
    valences: &[i32],
    cache: &HashMap<PairKey, PairEntry>,
    lat_mat: &Matrix3<f64>,
    inv_lat_t: Option<Matrix3<f64>>,
    ranges: [i32; 3],
    use_pbc: bool,
) -> AtomBVS {
    let v_i = valences[atom_idx];
    let atom = &structure.atoms[atom_idx];
    let pos_i = Vector3::from(atom.position);

    let frac_i = match (use_pbc, inv_lat_t) {
        (true, Some(inv)) => inv * pos_i,
        _ => Vector3::zeros(), // unused when use_pbc is false
    };

    let mut bvs = 0.0_f64;
    let mut cn = 0_usize;
    let mut best_iucr = false;
    let mut had_any_pair = false;

    for (j, neighbor) in structure.atoms.iter().enumerate() {
        let v_j = valences[j];

        // Direction-independent role check: at least one cation, at most one.
        let (cation, vc, anion, va) = match (v_i.signum(), v_j.signum()) {
            (1, -1) => (
                atom.element.as_str(),
                v_i,
                neighbor.element.as_str(),
                v_j,
            ),
            (-1, 1) => (
                neighbor.element.as_str(),
                v_j,
                atom.element.as_str(),
                v_i,
            ),
            _ => continue,
        };

        let entry = match cache.get(&(
            cation.to_string(),
            vc,
            anion.to_string(),
            va,
        )) {
            Some(e) => *e,
            None => continue,
        };
        let params = match entry.params {
            Some(p) => p,
            None => continue,
        };
        had_any_pair = true;

        let pos_j = Vector3::from(neighbor.position);

        if use_pbc {
            let inv = inv_lat_t.expect("use_pbc requires invertible lattice");
            let frac_j = inv * pos_j;
            for nx in -ranges[0]..=ranges[0] {
                for ny in -ranges[1]..=ranges[1] {
                    for nz in -ranges[2]..=ranges[2] {
                        if j == atom_idx && nx == 0 && ny == 0 && nz == 0 {
                            continue;
                        }
                        let img = frac_j + Vector3::new(nx as f64, ny as f64, nz as f64);
                        let dist = (lat_mat.transpose() * (img - frac_i)).norm();
                        if (MIN_DIST..=CUTOFF).contains(&dist) {
                            let v_ij = ((params.r0 - dist) / params.b).exp();
                            bvs += v_ij;
                            if v_ij > BOND_VALENCE_THRESHOLD {
                                cn += 1;
                            }
                            if entry.is_iucr {
                                best_iucr = true;
                            }
                        }
                    }
                }
            }
        } else {
            if j == atom_idx {
                continue;
            }
            let dist = (pos_i - pos_j).norm();
            if (MIN_DIST..=CUTOFF).contains(&dist) {
                let v_ij = ((params.r0 - dist) / params.b).exp();
                bvs += v_ij;
                if v_ij > BOND_VALENCE_THRESHOLD {
                    cn += 1;
                }
                if entry.is_iucr {
                    best_iucr = true;
                }
            }
        }
    }

    let assumed_v = if v_i == 9 || v_i == 0 { 0 } else { v_i };
    let expected = (assumed_v.unsigned_abs() as f64).max(0.0);
    let source = if !had_any_pair {
        ParamSource::NotApplicable
    } else if best_iucr {
        ParamSource::Iucr
    } else {
        ParamSource::BresOKeeffe
    };

    AtomBVS {
        bvs,
        assumed_v,
        expected,
        coordination: cn,
        source,
    }
}

/// Full-structure BVS analysis. Parallel over atoms via rayon.
pub fn analyze_structure(structure: &Structure) -> StructureBVS {
    let lat_mat = lattice_matrix(structure.lattice);
    let vol = cell_volume(&lat_mat);
    let inv_lat_t = lat_mat.transpose().try_inverse();
    let use_pbc = structure.is_periodic && vol >= MIN_LATTICE_VOLUME && inv_lat_t.is_some();
    let ranges = if use_pbc {
        image_ranges(&lat_mat)
    } else {
        [0, 0, 0]
    };

    let valences = resolve_valences(structure);
    let cache = build_pair_cache(structure, &valences);

    let atoms: Vec<AtomBVS> = (0..structure.atoms.len())
        .into_par_iter()
        .map(|i| {
            analyze_atom(
                structure, i, &valences, &cache, &lat_mat, inv_lat_t, ranges, use_pbc,
            )
        })
        .collect();

    let (sum_sq, sum_abs, max_abs, n) = atoms.iter().fold(
        (0.0_f64, 0.0_f64, 0.0_f64, 0_usize),
        |(ssq, sabs, mx, n), a| {
            if a.expected > 0.0 {
                let d = a.deviation();
                let abs = d.abs();
                (ssq + d * d, sabs + abs, mx.max(abs), n + 1)
            } else {
                (ssq, sabs, mx, n)
            }
        },
    );

    let gii = if n > 0 { (sum_sq / n as f64).sqrt() } else { 0.0 };
    let mean_abs_dev = if n > 0 { sum_abs / n as f64 } else { 0.0 };

    StructureBVS {
        atoms,
        gii,
        mean_abs_dev,
        max_abs_dev: max_abs,
        validated: n,
    }
}

// ─── Backward-compatible thin wrappers ───────────────────────────────────────
//
// Kept so the painter's BVS-coloring path, the report module, and existing
// tests don't have to change.

pub fn calculate_bvs_pbc(structure: &Structure, atom_idx: usize) -> f64 {
    let lat_mat = lattice_matrix(structure.lattice);
    let vol = cell_volume(&lat_mat);
    let inv_lat_t = lat_mat.transpose().try_inverse();
    if vol < MIN_LATTICE_VOLUME || inv_lat_t.is_none() {
        return calculate_bvs(structure, atom_idx);
    }
    let valences = resolve_valences(structure);
    let cache = build_pair_cache(structure, &valences);
    let ranges = image_ranges(&lat_mat);
    analyze_atom(
        structure,
        atom_idx,
        &valences,
        &cache,
        &lat_mat,
        inv_lat_t,
        ranges,
        true,
    )
    .bvs
}

pub fn calculate_bvs(structure: &Structure, atom_idx: usize) -> f64 {
    let lat_mat = lattice_matrix(structure.lattice);
    let valences = resolve_valences(structure);
    let cache = build_pair_cache(structure, &valences);
    analyze_atom(
        structure,
        atom_idx,
        &valences,
        &cache,
        &lat_mat,
        None,
        [0, 0, 0],
        false,
    )
    .bvs
}

pub fn calculate_bvs_auto(structure: &Structure, atom_idx: usize) -> f64 {
    if structure.is_periodic {
        calculate_bvs_pbc(structure, atom_idx)
    } else {
        calculate_bvs(structure, atom_idx)
    }
}

/// Fast batch (parallel) — used by the painter's per-frame BVS color cache.
pub fn calculate_bvs_all_auto(structure: &Structure) -> Vec<f64> {
    analyze_structure(structure)
        .atoms
        .into_iter()
        .map(|a| a.bvs)
        .collect()
}

pub fn calculate_bvs_all(structure: &Structure) -> Vec<f64> {
    let lat_mat = lattice_matrix(structure.lattice);
    let valences = resolve_valences(structure);
    let cache = build_pair_cache(structure, &valences);
    (0..structure.atoms.len())
        .into_par_iter()
        .map(|i| {
            analyze_atom(
                structure, i, &valences, &cache, &lat_mat, None, [0, 0, 0], false,
            )
            .bvs
        })
        .collect()
}

pub fn calculate_bvs_all_pbc(structure: &Structure) -> Vec<f64> {
    let lat_mat = lattice_matrix(structure.lattice);
    let inv_lat_t = lat_mat.transpose().try_inverse();
    if cell_volume(&lat_mat) < MIN_LATTICE_VOLUME || inv_lat_t.is_none() {
        return calculate_bvs_all(structure);
    }
    let valences = resolve_valences(structure);
    let cache = build_pair_cache(structure, &valences);
    let ranges = image_ranges(&lat_mat);
    (0..structure.atoms.len())
        .into_par_iter()
        .map(|i| {
            analyze_atom(
                structure, i, &valences, &cache, &lat_mat, inv_lat_t, ranges, true,
            )
            .bvs
        })
        .collect()
}

// ─── Quality / ideal-state helpers ────────────────────────────────────────────

/// Magnitude of the *primary* expected oxidation state for an element. Used
/// by the painter's BVS color gradient. Returns 0.0 when ambiguous.
pub fn get_ideal_oxidation_state(element: &str) -> f64 {
    let av = anion_valences(element)[0];
    if av < 0 {
        return av.unsigned_abs() as f64;
    }
    let cv = cation_valences(element)[0];
    if cv > 0 && cv != 9 {
        return cv as f64;
    }
    0.0
}

pub fn calculate_bvs_deviation(structure: &Structure, atom_idx: usize) -> f64 {
    let r = analyze_structure(structure);
    r.atoms[atom_idx].abs_deviation()
}

pub fn calculate_structure_quality(structure: &Structure) -> (f64, f64, usize) {
    let r = analyze_structure(structure);
    (r.mean_abs_dev, r.max_abs_dev, r.validated)
}

pub fn assess_structure_quality(structure: &Structure) -> BVSQuality {
    BVSQuality::from_deviation(analyze_structure(structure).mean_abs_dev)
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
            oxidation: None,
        }
    }

    fn atom_ox(element: &str, pos: [f64; 3], ox: i32) -> Atom {
        Atom {
            element: element.into(),
            position: pos,
            original_index: 0,
            oxidation: Some(ox),
        }
    }

    /// BaTiO₃: Ba CN=12 needs 4 images of each O. Minimum-image gives ~0.69
    /// instead of the correct ~2.0.
    #[test]
    fn batio3_canonical_bvs() {
        let a = 4.0_f64;
        let s = Structure {
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
        let r = analyze_structure(&s);
        assert!((1.5..=3.5).contains(&r.atoms[0].bvs), "Ba {}", r.atoms[0].bvs);
        assert!((2.5..=5.5).contains(&r.atoms[1].bvs), "Ti {}", r.atoms[1].bvs);
        assert!((1.0..=3.0).contains(&r.atoms[2].bvs), "O {}", r.atoms[2].bvs);
    }

    /// Unit cell and supercell must give identical BVS at corresponding sites.
    #[test]
    fn batio3_supercell_consistency() {
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
        let bvs_uc = analyze_structure(&uc).atoms[0].bvs;
        let bvs_sc = analyze_structure(&sc).atoms[0].bvs;
        assert!((bvs_uc - bvs_sc).abs() < 0.01);
    }

    /// O BVS in BaTiO₃ must be ≈ 2.0 (no spurious O-O contributions).
    #[test]
    fn anion_anion_rejected() {
        let a = 4.008_f64;
        let s = Structure {
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
        let r = analyze_structure(&s);
        assert!((1.0..=3.0).contains(&r.atoms[2].bvs), "O {}", r.atoms[2].bvs);
    }

    /// Per-atom oxidation override: Fe²⁺ vs Fe³⁺ at the same coordinates
    /// must yield different BVS expectations.
    #[test]
    fn explicit_oxidation_state_used() {
        let a = 4.0_f64;
        let s_fe2 = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom_ox("Fe", [0.0, 0.0, 0.0], 2),
                atom("O", [a / 2.0, 0.0, 0.0]),
                atom("O", [0.0, a / 2.0, 0.0]),
                atom("O", [0.0, 0.0, a / 2.0]),
            ],
            formula: "FeO".into(),
            is_periodic: true,
        };
        let s_fe3 = Structure {
            atoms: vec![
                atom_ox("Fe", [0.0, 0.0, 0.0], 3),
                atom("O", [a / 2.0, 0.0, 0.0]),
                atom("O", [0.0, a / 2.0, 0.0]),
                atom("O", [0.0, 0.0, a / 2.0]),
            ],
            ..s_fe2.clone()
        };
        let r2 = analyze_structure(&s_fe2);
        let r3 = analyze_structure(&s_fe3);
        assert_eq!(r2.atoms[0].assumed_v, 2);
        assert_eq!(r3.atoms[0].assumed_v, 3);
        assert_eq!(r2.atoms[0].expected, 2.0);
        assert_eq!(r3.atoms[0].expected, 3.0);
    }

    /// H is amphoteric: NaH → H is hydride (-1); H₂O → H is proton (+1).
    /// Without explicit oxidation, classification by neighbor χ must pick.
    #[test]
    fn hydrogen_amphoteric_classification() {
        let a = 4.0_f64;
        // NaH (rock salt): H closest to Na (χ=0.93) → H is anion.
        let nah = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Na", [0.0, 0.0, 0.0]),
                atom("H", [a / 2.0, 0.0, 0.0]),
            ],
            formula: "NaH".into(),
            is_periodic: true,
        };
        let v = resolve_valences(&nah);
        assert_eq!(v[1], -1, "H in NaH should be hydride");

        // Isolated H–O molecule: H closest to O (χ=3.44) → H is proton.
        let oh = Structure {
            lattice: [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            atoms: vec![
                atom("O", [0.0, 0.0, 0.0]),
                atom("H", [0.96, 0.0, 0.0]),
            ],
            formula: "OH".into(),
            is_periodic: false,
        };
        let v = resolve_valences(&oh);
        assert_eq!(v[1], 1, "H in OH should be proton");
    }

    /// Cation-cation pairs must not contribute. Two Na cations at typical
    /// metallic distance → BVS = 0 from the calculator.
    #[test]
    fn cation_cation_excluded() {
        let s = Structure {
            lattice: [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]],
            atoms: vec![
                atom_ox("Na", [0.0, 0.0, 0.0], 1),
                atom_ox("Na", [3.0, 0.0, 0.0], 1),
            ],
            formula: "Na".into(),
            is_periodic: false,
        };
        let r = analyze_structure(&s);
        assert!(r.atoms[0].bvs < 1e-10);
    }

    /// Image range: a 30°-skew monoclinic cell needs more images on the
    /// short-perpendicular axis than the lattice-length bound predicts.
    /// We just verify the function returns a strictly positive integer
    /// for each axis on a representative monoclinic cell.
    #[test]
    fn image_range_monoclinic_is_positive() {
        let lat = lattice_matrix([
            [5.0, 0.0, 0.0],
            [0.0, 5.0, 0.0],
            [3.0, 0.0, 4.0], // β ≈ 53° (not orthogonal)
        ]);
        let r = image_ranges(&lat);
        assert!(r[0] >= 1 && r[1] >= 1 && r[2] >= 1, "{:?}", r);
    }

    /// GII must be non-negative and at most the maximum |Δ|.
    #[test]
    fn gii_bounds() {
        let a = 4.0_f64;
        let s = Structure {
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
        let r = analyze_structure(&s);
        assert!(r.gii >= 0.0);
        assert!(r.gii <= r.max_abs_dev + 1e-12);
        assert!(r.validated == 5);
    }

    /// CN for Ti in BaTiO₃ should be 6 (octahedral).
    #[test]
    fn coordination_number_octahedral_ti() {
        let a = 4.0_f64;
        let s = Structure {
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
        let r = analyze_structure(&s);
        assert_eq!(r.atoms[1].coordination, 6, "Ti CN should be 6");
    }

    /// Classic ideal oxidation table used by the painter color gradient.
    #[test]
    fn ideal_oxidation_states() {
        assert_eq!(get_ideal_oxidation_state("O"), 2.0);
        assert_eq!(get_ideal_oxidation_state("F"), 1.0);
        assert_eq!(get_ideal_oxidation_state("Ba"), 2.0);
        assert_eq!(get_ideal_oxidation_state("Ti"), 4.0);
        assert_eq!(get_ideal_oxidation_state("Al"), 3.0);
    }

    #[test]
    fn quality_thresholds_literature_bands() {
        assert_eq!(BVSQuality::from_deviation(0.05), BVSQuality::Excellent);
        assert_eq!(BVSQuality::from_deviation(0.15), BVSQuality::Good);
        assert_eq!(BVSQuality::from_deviation(0.30), BVSQuality::Acceptable);
        assert_eq!(BVSQuality::from_deviation(0.50), BVSQuality::Poor);
    }

    /// Degenerate lattice (zero volume) falls back to non-PBC silently.
    #[test]
    fn degenerate_lattice_fallback() {
        let s = Structure {
            lattice: [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            atoms: vec![atom("Na", [0.0, 0.0, 0.0]), atom("Cl", [2.82, 0.0, 0.0])],
            formula: "NaCl".into(),
            is_periodic: true,
        };
        let bvs = calculate_bvs_pbc(&s, 0);
        assert!(bvs > 0.0);
    }
}
