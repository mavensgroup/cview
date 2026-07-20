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
// 1. **PBC done right — a single unit cell is sufficient input.** Image
//    enumeration uses the perpendicular spacing of each lattice direction
//    (`d_i = V/|a_j × a_k|`), and interatomic offsets are minimum-image
//    wrapped before enumeration, so every periodic image within CUTOFF is
//    counted for any cell shape and for any input coordinate wrapping
//    (atoms at fractional 1.0, unwrapped XYZ/supercell output, …). BVS on
//    the unit cell and on any supercell of it are identical by
//    construction (regression-tested).
//
// 2. **Per-atom oxidation state honoured.** When a parser supplied
//    `Atom.oxidation`, it overrides every guess. This is the fix for
//    mixed-valence systems like Fe₃O₄ where a CIF distinguishes Fe²⁺ and
//    Fe³⁺ sites explicitly.
//
// 3. **Per-site role resolution for amphoteric elements.** H, N, P, As,
//    S, Se, Te can be cation or anion depending on chemistry. The role is
//    inferred per site from the nearest neighbor's electronegativity:
//    H₂O → H⁺, NaH → H⁻, BaSO₄ → S⁶⁺, ZnS → S²⁻, NaNO₃ → N⁵⁺,
//    Li₃N → N³⁻ — automatically. Polyanionic chemistry (sulfates,
//    phosphates, nitrates, arsenates) therefore works.
//
// 4. **Cation–cation rejection.** BVS is defined for heteropolar bonds;
//    same-sign pairs are never counted.
//
// 5. **Parameter provenance surfaced.** Every atom reports whether its
//    parameters came from the exact IUCr entry ("IUCr"), an IUCr entry at
//    a substituted valence ("IUCr*"), or the O'Keeffe-Brese estimation
//    ("B&OK"). The estimation uses the authors' fitted (r, c) parameters
//    and their published formula — accuracy ±0.05 Å in R0, ~15% in
//    valence (verified against their own pair tables in the test suite).
//
// 6. **Pair-parameter cache + rayon parallelism.** The 1000-arm table
//    match runs once per distinct species pair, not per neighbor; atoms
//    are processed in parallel.
//
// 7. **Coordination number** computed alongside BVS using Brown's
//    convention: count bonds with v_ij > 0.04 v.u.
//
// 8. **GII-banded quality.** The quality banner uses the Global
//    Instability Index (GII < 0.1 stable, > 0.2 strained — Salinas-
//    Sanchez 1992, Brown 2002); mean |Δ| is reported as a statistic only.
//
// ## Known limitations (documented, not silent)
//
// - **Occupancy weighting is mean-field.** Each neighbor's bond valence
//   is scaled by its occupancy (v_ij × occ_j — the bond-valence analogue
//   of the virtual-crystal approximation), and the central atom's own
//   occupancy does not scale its BVS: an occupied site wants full
//   valence. This gives the correct AVERAGE valence for substitutional
//   disorder (CPA-style Fe₀.₇Cr₀.₃, split sites) but no local
//   relaxation or short-range order around specific configurations.
//
// - **O and halogens are anion-always.** Cl⁷⁺/Br⁷⁺/I⁵⁺ oxo-cations
//   (perchlorates, iodates) are not recognized unless the file supplies
//   explicit oxidation states — then they work via the override.
//
// - **No self-consistent valence assignment** for mixed-valence systems
//   without explicit oxidation states (e.g. Fe₃O₄ from a bare XYZ): the
//   priority list picks one state per element. Supply charges in the CIF
//   or set per-site oxidation to resolve.
//
// - **Estimation coverage is partial by design.** Elements whose
//   O'Keeffe-Brese parameters failed validation against the published
//   tables (Cu, Au, Pd, Rh, In, Sn, Sb, most lanthanides/actinides) have
//   no estimation fallback; untabulated pairs involving them are skipped
//   and reported "n/a" rather than computed with a wrong R0.
//
// References:
//   I.D. Brown & D. Altermatt, Acta Cryst. B41 (1985) 244–247.
//   N.E. Brese & M. O'Keeffe, Acta Cryst. B47 (1991) 192–197.
//   M. O'Keeffe & N.E. Brese, J. Am. Chem. Soc. 113 (1991) 3226–3229.
//   I.D. Brown, "The Chemical Bond in Inorganic Chemistry: The Bond Valence
//   Model", IUCr Monograph 12, OUP, 2002.

use crate::model::bvs::BvsParams;
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
    /// Tabulated IUCr bvparm2020 entry for the exact (element, valence)
    /// pair — most accurate.
    Iucr,
    /// Tabulated IUCr entry, but for a *different valence* of the same
    /// element pair (e.g. an explicit Fe²⁺ site computed with Fe³⁺
    /// parameters because the requested pair is untabulated). Accurate
    /// bond-length scale, wrong valence — treat deviations with caution.
    IucrSubstituted,
    /// O'Keeffe-Brese (1991) estimation — R0 good to ~±0.05 Å.
    BresOKeeffe,
    /// No bonds matched (atom was isolated, or pair couldn't be classified).
    NotApplicable,
}

impl ParamSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParamSource::Iucr => "IUCr",
            ParamSource::IucrSubstituted => "IUCr*",
            ParamSource::BresOKeeffe => "B&OK",
            ParamSource::NotApplicable => "n/a",
        }
    }

    /// Quality rank for picking the best source across an atom's bonds.
    fn rank(&self) -> u8 {
        match self {
            ParamSource::Iucr => 3,
            ParamSource::IucrSubstituted => 2,
            ParamSource::BresOKeeffe => 1,
            ParamSource::NotApplicable => 0,
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
    /// Bands per literature norms (Brown 2002, Salinas-Sanchez 1992).
    /// NOTE: the literature bands are defined on the **GII** (< 0.10
    /// stable, > 0.20 strained) — pass `StructureBVS::gii` here, not the
    /// mean |Δ| (which is a different, typically smaller statistic).
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
/// `d_i = V / |a_j × a_k|`. The pair loop first wraps Δfrac to [-1/2, 1/2)
/// per component (minimum image), so an image at offset n is at least
/// `(|n| - 1/2)·d_i` away along that direction; covering every image within
/// CUTOFF therefore needs `n_i = ⌈CUTOFF/d_i + 1/2⌉` on each side. This is
/// rigorous for any cell shape AND any input coordinates — including atoms
/// parsed at fractional 1.0 or outside [0,1), which the pre-wrap bound
/// `⌈CUTOFF/d_i⌉` silently under-covered by one shell.
fn image_ranges(lat_mat: &Matrix3<f64>) -> [i32; 3] {
    let a = lat_mat.row(0).transpose();
    let b = lat_mat.row(1).transpose();
    let c = lat_mat.row(2).transpose();
    let v = cell_volume(lat_mat).max(1e-12);

    let d_a = v / b.cross(&c).norm().max(1e-12);
    let d_b = v / c.cross(&a).norm().max(1e-12);
    let d_c = v / a.cross(&b).norm().max(1e-12);

    [
        (CUTOFF / d_a + 0.5).ceil() as i32,
        (CUTOFF / d_b + 0.5).ceil() as i32,
        (CUTOFF / d_c + 0.5).ceil() as i32,
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
        "S" => &[6, 4, 9],
        "Se" => &[4, 6, 9],
        "Te" => &[4, 6, 9],
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

/// Elements whose role (cation vs anion) genuinely depends on the chemical
/// environment: H (proton/hydride) and the oxo-anion formers. In BaSO₄ the
/// S is S⁶⁺ bonded to O; in ZnS it is S²⁻ bonded to Zn. O and the halogens
/// are deliberately NOT here — they are treated as anions always, so
/// perchlorates/iodates remain outside the model (see module header).
fn is_dual_role(element: &str) -> bool {
    matches!(element, "H" | "N" | "P" | "As" | "S" | "Se" | "Te")
}

/// Electronegativity of the nearest neighbor (PBC-aware, minimum image).
/// `skip_same_element` excludes neighbors of the same element — used for H,
/// where the closest H of the same molecule must not decide the role.
fn nearest_neighbor_chi(
    structure: &Structure,
    atom_idx: usize,
    skip_same_element: bool,
) -> Option<f64> {
    let lat = lattice_matrix(structure.lattice);
    let pbc = structure.is_periodic && cell_volume(&lat) >= MIN_LATTICE_VOLUME;

    let element_i = &structure.atoms[atom_idx].element;
    let p_i = Vector3::from(structure.atoms[atom_idx].position);
    let inv_lt = lat.transpose().try_inverse();

    let mut best_d2 = f64::MAX;
    let mut best_chi = None;

    for (j, neighbor) in structure.atoms.iter().enumerate() {
        if j == atom_idx || (skip_same_element && neighbor.element == *element_i) {
            continue;
        }
        let chi_j = get_electronegativity(&neighbor.element);
        if chi_j <= 0.0 {
            continue;
        }
        let p_j = Vector3::from(neighbor.position);

        let d2_min = if let (true, Some(inv)) = (pbc, inv_lt) {
            // Minimum image: wrap Δfrac to [-1/2, 1/2) per component. Exact
            // for the nearest-neighbor question in all but pathologically
            // oblique cells, and immune to unwrapped input coordinates.
            let mut df = inv * p_j - inv * p_i;
            for k in 0..3 {
                df[k] -= df[k].round();
            }
            let mut min2 = f64::MAX;
            for nx in -1..=1_i32 {
                for ny in -1..=1_i32 {
                    for nz in -1..=1_i32 {
                        let img = df + Vector3::new(nx as f64, ny as f64, nz as f64);
                        let d2 = (lat.transpose() * img).norm_squared();
                        if d2 < min2 {
                            min2 = d2;
                        }
                    }
                }
            }
            min2
        } else {
            (p_i - p_j).norm_squared()
        };

        if d2_min < best_d2 {
            best_d2 = d2_min;
            best_chi = Some(chi_j);
        }
    }
    best_chi
}

/// Per-site role resolution for dual-role elements: if the nearest neighbor
/// is more electronegative than the atom itself, the site acts as a cation
/// (first cation valence); otherwise as an anion. This is the generalization
/// of the classic amphoteric-H rule (H₂O → H⁺, NaH → H⁻) to the oxo-anion
/// formers: S in BaSO₄ → S⁶⁺, S in ZnS → S²⁻, N in NaNO₃ → N⁵⁺, N in
/// Li₃N → N³⁻. Same-element neighbors count (pyrite S-S dimers stay S²⁻:
/// equal χ is not "more electronegative").
fn classify_dual_role(structure: &Structure, atom_idx: usize) -> i32 {
    let element = &structure.atoms[atom_idx].element;
    let own_chi = if element == "H" {
        H_ELECTRONEGATIVITY
    } else {
        get_electronegativity(element)
    };
    let cation_v = cation_valences(element)[0];
    let anion_v = anion_valences(element)[0];

    match nearest_neighbor_chi(structure, atom_idx, element == "H") {
        Some(chi) if chi > own_chi => cation_v,
        Some(_) => anion_v,
        // No classifiable neighbor — fall back to the anion role (the
        // historical H behaviour: isolated H is treated as hydride).
        None => anion_v,
    }
}

/// Resolve every atom's working valence:
///
/// 1. Explicit `Atom.oxidation` from the parser → use as-is.
/// 2. Dual-role elements (H, N, P, As, S, Se, Te) without an explicit
///    state → classify per-site by nearest-neighbor electronegativity.
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
            if is_dual_role(&atom.element) {
                return classify_dual_role(structure, i);
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
/// valences, tracking WHERE the parameters came from. Table lookups use
/// `get_bvs_params_tabulated` (no hidden estimation), so the priority-list
/// walk actually reaches tabulated substitute valences; only step 5 is the
/// O'Keeffe-Brese estimate.
fn resolve_pair_params(
    cation: &str,
    val_c: i32,
    anion: &str,
    val_a: i32,
) -> Option<(BvsParams, ParamSource)> {
    use crate::model::bvs::{estimate_bvs_params, get_bvs_params_tabulated};

    // 1. Exact charges given.
    if let Some(p) = get_bvs_params_tabulated(cation, val_c, anion, val_a) {
        return Some((p, ParamSource::Iucr));
    }

    // 2. Priority list for cation, anion held fixed.
    if val_a != 9 {
        for &v in cation_valences(cation) {
            if v == val_c {
                continue;
            }
            if let Some(p) = get_bvs_params_tabulated(cation, v, anion, val_a) {
                return Some((p, ParamSource::IucrSubstituted));
            }
        }
    }

    // 3. Priority list for anion, cation held fixed.
    if val_c != 9 {
        for &v in anion_valences(anion) {
            if v == val_a {
                continue;
            }
            if let Some(p) = get_bvs_params_tabulated(cation, val_c, anion, v) {
                return Some((p, ParamSource::IucrSubstituted));
            }
        }
    }

    // 4. Both lists.
    for &vc in cation_valences(cation) {
        for &va in anion_valences(anion) {
            if let Some(p) = get_bvs_params_tabulated(cation, vc, anion, va) {
                return Some((p, ParamSource::IucrSubstituted));
            }
        }
    }

    // 5. O'Keeffe-Brese estimation (valence-independent).
    estimate_bvs_params(cation, anion).map(|p| (p, ParamSource::BresOKeeffe))
}

/// Cache key: the resolved-valence-tagged pair as the calculator sees it.
type PairKey = (String, i32, String, i32);

/// Pair lookup result with the parameter provenance
/// (drives the "Source" column in the report).
#[derive(Clone, Copy)]
struct PairEntry {
    params: Option<BvsParams>,
    source: ParamSource,
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

            let (params, source) = match resolve_pair_params(c, vc, a, va) {
                Some((p, s)) => (Some(p), s),
                None => (None, ParamSource::NotApplicable),
            };
            cache.insert(key, PairEntry { params, source });
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
    let mut best_source = ParamSource::NotApplicable;
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
            // Minimum-image wrap of Δfrac to [-1/2, 1/2) per component:
            // makes the image enumeration independent of how the input
            // coordinates were wrapped (frac = 1.0, supercell leftovers, …).
            let mut dfrac = inv * pos_j - frac_i;
            for k in 0..3 {
                dfrac[k] -= dfrac[k].round();
            }
            for nx in -ranges[0]..=ranges[0] {
                for ny in -ranges[1]..=ranges[1] {
                    for nz in -ranges[2]..=ranges[2] {
                        if j == atom_idx && nx == 0 && ny == 0 && nz == 0 {
                            continue;
                        }
                        let img = dfrac + Vector3::new(nx as f64, ny as f64, nz as f64);
                        let dist = (lat_mat.transpose() * img).norm();
                        if (MIN_DIST..=CUTOFF).contains(&dist) {
                            // Weight by the NEIGHBOR's occupancy: a
                            // half-occupied ligand contributes half its
                            // valence on average. The central atom's own
                            // occupancy does not scale its BVS — when the
                            // site is occupied, it wants full valence.
                            let v_ij =
                                ((params.r0 - dist) / params.b).exp() * neighbor.occupancy;
                            bvs += v_ij;
                            if v_ij > BOND_VALENCE_THRESHOLD {
                                cn += 1;
                            }
                            if entry.source.rank() > best_source.rank() {
                                best_source = entry.source;
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
                let v_ij = ((params.r0 - dist) / params.b).exp() * neighbor.occupancy;
                bvs += v_ij;
                if v_ij > BOND_VALENCE_THRESHOLD {
                    cn += 1;
                }
                if entry.source.rank() > best_source.rank() {
                    best_source = entry.source;
                }
            }
        }
    }

    let assumed_v = if v_i == 9 || v_i == 0 { 0 } else { v_i };
    let expected = (assumed_v.unsigned_abs() as f64).max(0.0);
    let source = if !had_any_pair {
        ParamSource::NotApplicable
    } else {
        best_source
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
    // Literature quality bands are defined on the GII (Salinas-Sanchez 1992;
    // Brown 2002), not on mean |Δ|.
    BVSQuality::from_deviation(analyze_structure(structure).gii)
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
            occupancy: 1.0,
        }
    }

    fn atom_ox(element: &str, pos: [f64; 3], ox: i32) -> Atom {
        Atom {
            element: element.into(),
            position: pos,
            original_index: 0,
            oxidation: Some(ox),
            occupancy: 1.0,
        }
    }

    fn atom_occ(element: &str, pos: [f64; 3], occ: f64) -> Atom {
        Atom {
            element: element.into(),
            position: pos,
            original_index: 0,
            oxidation: None,
            occupancy: occ,
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

    /// BVS-2: oxo-anion formers must be classified per-site. A sulfate
    /// group: S surrounded by 4 O at the ideal S-O distance must resolve
    /// to S⁶⁺ (not S²⁻) and give BVS ≈ 6.
    #[test]
    fn sulfate_sulfur_is_cation() {
        let d = 1.473_f64; // ideal S-O in sulfate
        let t = d / 3.0_f64.sqrt();
        let s = Structure {
            lattice: [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            atoms: vec![
                atom("S", [5.0, 5.0, 5.0]),
                atom("O", [5.0 + t, 5.0 + t, 5.0 + t]),
                atom("O", [5.0 + t, 5.0 - t, 5.0 - t]),
                atom("O", [5.0 - t, 5.0 + t, 5.0 - t]),
                atom("O", [5.0 - t, 5.0 - t, 5.0 + t]),
            ],
            formula: "SO4".into(),
            is_periodic: true,
        };
        let v = resolve_valences(&s);
        assert_eq!(v[0], 6, "S in sulfate must resolve to +6");
        assert_eq!(v[1], -2, "O must stay -2");

        let r = analyze_structure(&s);
        assert!(
            (5.5..=6.5).contains(&r.atoms[0].bvs),
            "S BVS should be ≈ 6, got {}",
            r.atoms[0].bvs
        );
    }

    /// BVS-2 regression: in sulfides the S must STAY an anion. Sphalerite
    /// ZnS: S nearest neighbor is Zn (χ 1.65 < χ_S 2.58) → S²⁻, Zn BVS ≈ 2.
    #[test]
    fn sphalerite_sulfur_stays_anion() {
        let a = 5.41_f64;
        let fcc = [[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [0.5, 0.0, 0.5], [0.0, 0.5, 0.5]];
        let mut atoms_v = Vec::new();
        for f in fcc {
            atoms_v.push(atom("Zn", [f[0] * a, f[1] * a, f[2] * a]));
        }
        for f in fcc {
            atoms_v.push(atom(
                "S",
                [(f[0] + 0.25) * a, (f[1] + 0.25) * a, (f[2] + 0.25) * a],
            ));
        }
        let s = Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: atoms_v,
            formula: "ZnS".into(),
            is_periodic: true,
        };
        let v = resolve_valences(&s);
        assert_eq!(v[4], -2, "S in ZnS must stay -2");

        let r = analyze_structure(&s);
        assert!(
            (1.5..=2.5).contains(&r.atoms[0].bvs),
            "Zn BVS should be ≈ 2, got {}",
            r.atoms[0].bvs
        );
    }

    /// BVS-2 regression: N in a nitride (Li environment) stays N³⁻.
    #[test]
    fn nitride_nitrogen_stays_anion() {
        let s = Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms: vec![
                atom("N", [0.0, 0.0, 0.0]),
                atom("Li", [2.0, 0.0, 0.0]),
                atom("Li", [0.0, 2.0, 0.0]),
                atom("Li", [0.0, 0.0, 2.0]),
            ],
            formula: "Li3N".into(),
            is_periodic: true,
        };
        let v = resolve_valences(&s);
        assert_eq!(v[0], -3, "N with Li neighbors must resolve to -3");
    }

    /// BVS-3: coordinates at the cell boundary (fractional 1.0 instead of
    /// 0.0) must give the identical BVS — this is the "single unit cell is
    /// enough" guarantee.
    #[test]
    fn boundary_coordinates_identical_bvs() {
        let a = 5.64_f64;
        let make = |cl_x: f64| Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Na", [0.0, 0.0, 0.0]),
                atom("Na", [a / 2.0, a / 2.0, 0.0]),
                atom("Na", [a / 2.0, 0.0, a / 2.0]),
                atom("Na", [0.0, a / 2.0, a / 2.0]),
                atom("Cl", [cl_x, 0.0, 0.0]), // frac 0.5 vs 1.5 — same site
                atom("Cl", [0.0, a / 2.0, 0.0]),
                atom("Cl", [0.0, 0.0, a / 2.0]),
                atom("Cl", [a / 2.0, a / 2.0, a / 2.0]),
            ],
            formula: "NaCl".into(),
            is_periodic: true,
        };
        let wrapped = analyze_structure(&make(a / 2.0));
        let unwrapped = analyze_structure(&make(1.5 * a));
        for i in 0..8 {
            assert!(
                (wrapped.atoms[i].bvs - unwrapped.atoms[i].bvs).abs() < 1e-9,
                "atom {i}: {} vs {}",
                wrapped.atoms[i].bvs,
                unwrapped.atoms[i].bvs
            );
        }
        // And the value itself must be sane rock-salt chemistry.
        assert!(
            (0.7..=1.4).contains(&wrapped.atoms[0].bvs),
            "Na BVS {}",
            wrapped.atoms[0].bvs
        );
    }

    /// Occupancy weighting: a rock-salt cell where the anion sublattice is
    /// half-occupied must give the cation exactly half the BVS of the
    /// fully-occupied cell; and a split Fe/Cr site (occ 0.7/0.3, coincident)
    /// must see the full weighted anion shell, not a doubled one.
    #[test]
    fn occupancy_weights_bvs() {
        let a = 5.64_f64;
        let make = |occ: f64| Structure {
            lattice: [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]],
            atoms: vec![
                atom("Na", [0.0, 0.0, 0.0]),
                atom("Na", [a / 2.0, a / 2.0, 0.0]),
                atom("Na", [a / 2.0, 0.0, a / 2.0]),
                atom("Na", [0.0, a / 2.0, a / 2.0]),
                atom_occ("Cl", [a / 2.0, 0.0, 0.0], occ),
                atom_occ("Cl", [0.0, a / 2.0, 0.0], occ),
                atom_occ("Cl", [0.0, 0.0, a / 2.0], occ),
                atom_occ("Cl", [a / 2.0, a / 2.0, a / 2.0], occ),
            ],
            formula: "NaCl".into(),
            is_periodic: true,
        };
        let full = analyze_structure(&make(1.0)).atoms[0].bvs;
        let half = analyze_structure(&make(0.5)).atoms[0].bvs;
        assert!(
            (half - full / 2.0).abs() < 1e-9,
            "half-occupied anions must halve the BVS: {half} vs {full}/2"
        );

        // Split cation site: Fe (0.7) and Cr (0.3) coincident. Their mutual
        // distance is 0 < MIN_DIST so they don't bond each other; each sees
        // the full O octahedron and reports its own (site-conditional) BVS.
        let a2 = 4.2_f64;
        let split = Structure {
            lattice: [[a2, 0.0, 0.0], [0.0, a2, 0.0], [0.0, 0.0, a2]],
            atoms: vec![
                atom_occ("Fe", [0.0, 0.0, 0.0], 0.7),
                atom_occ("Cr", [0.0, 0.0, 0.0], 0.3),
                atom("O", [a2 / 2.0, 0.0, 0.0]),
                atom("O", [0.0, a2 / 2.0, 0.0]),
                atom("O", [0.0, 0.0, a2 / 2.0]),
            ],
            formula: "(Fe,Cr)O".into(),
            is_periodic: true,
        };
        let r = analyze_structure(&split);
        assert!(r.atoms[0].bvs > 0.5, "Fe must bond to O: {}", r.atoms[0].bvs);
        assert!(r.atoms[1].bvs > 0.5, "Cr must bond to O: {}", r.atoms[1].bvs);
        // O sees Fe weighted 0.7 + Cr weighted 0.3 — one cation's worth,
        // not two: its BVS must be far below the doubled value.
        let o_bvs = r.atoms[2].bvs;
        assert!(
            o_bvs < 1.15 * r.atoms[0].bvs.max(r.atoms[1].bvs),
            "O BVS {o_bvs} looks double-counted"
        );
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
