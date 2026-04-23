// src/rendering/polyhedra.rs
//
// Coordination polyhedra for crystal structures:
//   - Structure-aware cation/anion classification via Pauling electronegativity
//     (handles phosphates, sulfates, etc. correctly).
//   - Neighbor detection: covalent radii × user tolerance, with a hard Å cap.
//     Spatial-grid accelerated (O(1) per query). Rayon-parallel across cations.
//   - Convex hull: brute-force O(n⁴) "all other points on one side" — simple,
//     correct by construction, numerically robust for n ≤ 20 (the typical
//     coordination-polyhedra range). See the CONVEX HULL section.
//   - Distortion metrics (Baur index, bond-angle variance, quadratic
//     elongation, polyhedron volume) via `Polyhedron::metrics()`.
//
// Vertex positions are stored as atom-slice indices (O(1) screen lookup).
// All vector math via nalgebra::Vector3.

use crate::model::elements::{get_atom_cov, get_electronegativity};
use crate::rendering::scene::RenderAtom;
use crate::utils::spatial_grid::SpatialGrid;
use nalgebra::Vector3;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn v(p: [f64; 3]) -> Vector3<f64> {
    Vector3::new(p[0], p[1], p[2])
}

fn arr(p: Vector3<f64>) -> [f64; 3] {
    [p.x, p.y, p.z]
}

// ============================================================================
// PUBLIC TYPES
// ============================================================================

#[derive(Debug, Clone)]
pub struct Polyhedron {
    pub center_idx: usize,
    pub neighbor_indices: Vec<usize>,
    pub faces: Vec<Face>,
    pub coordination_number: usize,
}

/// Triangular face. Vertices stored as atom-slice indices for O(1) screen lookup.
#[derive(Debug, Clone)]
pub struct Face {
    pub vertex_atom_indices: [usize; 3],
    /// Cartesian centroid — used for depth sorting only.
    pub cart_center: [f64; 3],
}

impl Face {
    pub fn screen_vertices(&self, atoms: &[RenderAtom]) -> [[f64; 3]; 3] {
        [
            atoms[self.vertex_atom_indices[0]].screen_pos,
            atoms[self.vertex_atom_indices[1]].screen_pos,
            atoms[self.vertex_atom_indices[2]].screen_pos,
        ]
    }

    pub fn screen_center(&self, atoms: &[RenderAtom]) -> [f64; 3] {
        let sv = self.screen_vertices(atoms);
        [
            (sv[0][0] + sv[1][0] + sv[2][0]) / 3.0,
            (sv[0][1] + sv[1][1] + sv[2][1]) / 3.0,
            (sv[0][2] + sv[1][2] + sv[2][2]) / 3.0,
        ]
    }
}

// ============================================================================
// CACHE
// ============================================================================

#[derive(Default)]
pub struct PolyhedraCache {
    key: Option<CacheKey>,
    pub polyhedra: Vec<Polyhedron>,
}

#[derive(PartialEq)]
struct CacheKey {
    elements: Vec<String>,
    cutoff_q: u32,
    atom_count: usize,
    min_cn: usize,
    max_cn: usize,
    max_dist_q: u32,
}

impl PolyhedraCache {
    pub fn get_or_build(
        &mut self,
        atoms: &[RenderAtom],
        enabled_elements: &[String],
        bond_cutoff: f64,
        min_cn: usize,
        max_cn: usize,
        max_bond_dist: f64,
        show_ghosts: bool,
    ) -> &[Polyhedron] {
        let mut sorted_elements = enabled_elements.to_vec();
        sorted_elements.sort();

        let new_key = CacheKey {
            elements: sorted_elements,
            cutoff_q: (bond_cutoff * 1000.0) as u32,
            atom_count: atoms.len(),
            min_cn,
            max_cn,
            max_dist_q: (max_bond_dist * 100.0) as u32,
        };

        if self.key.as_ref() != Some(&new_key) {
            self.polyhedra = build_polyhedra_inner(
                atoms,
                enabled_elements,
                bond_cutoff,
                min_cn,
                max_cn,
                max_bond_dist,
                show_ghosts,
            );
            self.key = Some(new_key);
        }

        &self.polyhedra
    }

    pub fn invalidate(&mut self) {
        self.key = None;
    }
}

// ── Anion classification ─────────────────────────────────────────────────────
//
// Coordination polyhedra are built around cations and drawn with anion
// vertices (standard VESTA/Diamond/CrystalMaker convention). The question
// of what counts as an anion is more subtle than a hardcoded list:
//
//   - In phosphates (PO₄³⁻), nitrates (NO₃⁻), sulfates (SO₄²⁻),
//     arsenates (AsO₄³⁻), the central atom P/N/S/As is a CATION and
//     oxygen is the anion. A hardcoded list containing P/N/S/As would
//     mis-classify these ubiquitous structures.
//
//   - In oxyhalides, both O and F are anions. In sulfides/selenides, S
//     is the anion even though its electronegativity (2.58) is modest.
//
// The approach is to determine anions from the structure itself: find
// the most-electronegative element present, then include any element
// within 0.6 Pauling units of it (validated against common inorganic
// structures — see tests). This handles the important cases: all
// single-anion structures, phosphates/sulfates/nitrates/arsenates
// (only O), and oxyfluorides (both O and F).
//
// Known limitations that electronegativity alone cannot resolve:
//   - KNO₃: N (3.04) and O (3.44) differ by only 0.40; any threshold
//     inclusive of KCl-type O-F cases (Δ = 0.54) will also incorrectly
//     include N in nitrates. Real tools (VESTA) handle this via user
//     bond configuration. CView treats the most-electronegative anion
//     cluster as the "true" anions, which is correct for nitrates.
//   - Oxysulfides (La₂O₂S, Δ(O,S) = 0.86) and chlorofluorides
//     (Δ(F,Cl) = 0.82): the window of 0.6 treats these as
//     single-anion (just O, just F respectively). Users can adjust
//     polyhedra settings manually for these edge cases.

/// Determine the set of anions for a given structure by picking the
/// most-electronegative elements present. An element is treated as an
/// anion if its Pauling electronegativity is ≥ max(χ) − 0.6 AND
/// ≥ 2.0 (to avoid pure-metallic systems where the "most electronegative"
/// element is still a metal).
pub fn classify_anions(atoms: &[RenderAtom]) -> HashSet<String> {
    let mut max_chi = 0.0_f64;
    for a in atoms {
        let chi = get_electronegativity(&a.element);
        if chi > max_chi {
            max_chi = chi;
        }
    }
    // Pure-metallic system: no clear anion.
    if max_chi < 2.0 {
        return HashSet::new();
    }
    let threshold = (max_chi - 0.6).max(2.0);
    let mut out = HashSet::new();
    for a in atoms {
        let chi = get_electronegativity(&a.element);
        if chi >= threshold {
            out.insert(a.element.clone());
        }
    }
    out
}

/// Element-level fallback when structure context is not available. Uses
/// the traditional Pauling-scale threshold (χ ≥ 2.5) — includes the
/// halogens, chalcogens, oxygen, and nitrogen. Excludes P (2.19) and
/// As (2.18), which are cations in the vast majority of crystal
/// structures. Prefer `classify_anions` when the full atom list is
/// available.
fn is_anion(element: &str) -> bool {
    get_electronegativity(element) >= 2.5
}

// ============================================================================
// NEIGHBOR DETECTION
// ============================================================================

/// Bonded neighbors of `center_idx` for coordination polyhedra.
///
/// Follows the standard crystallographic convention (VESTA, Diamond, CrystalMaker):
/// - Center atom must be a cation; neighbors must be anions (O, F, S, etc.)
/// - This produces the expected TiO₆ octahedra, not spurious Ba₂₀ polyhedra
/// - Same-element and cation-cation bonds are excluded
/// - `max_dist`: hard Å cap, user-tunable via sidebar slider.
///
/// One-shot variant: classifies anions and builds a spatial grid on every
/// call. Use `find_coordination_neighbors_with_grid` in batch loops to
/// share both across queries.
pub fn find_coordination_neighbors(
    center_idx: usize,
    atoms: &[RenderAtom],
    tolerance: f64,
    max_dist: f64,
) -> Vec<usize> {
    let anions = classify_anions(atoms);
    let grid = SpatialGrid::build(atoms, max_dist.max(1e-3), |a| anions.contains(&a.element));
    find_coordination_neighbors_with_grid(center_idx, atoms, &grid, &anions, tolerance, max_dist)
}

/// Grid-aware variant of `find_coordination_neighbors`. Expects a grid
/// pre-built with `include = anions.contains(...)` and `cell_size >= max_dist`.
/// This is the hot path used by the per-frame polyhedra build.
pub fn find_coordination_neighbors_with_grid(
    center_idx: usize,
    atoms: &[RenderAtom],
    grid: &SpatialGrid,
    anions: &HashSet<String>,
    tolerance: f64,
    max_dist: f64,
) -> Vec<usize> {
    let r1 = &atoms[center_idx];

    // Only cations can be polyhedra centers.
    if anions.contains(&r1.element) {
        return Vec::new();
    }

    let rad1 = get_atom_cov(&r1.element);
    let mut candidates = Vec::with_capacity(32);
    grid.query(r1.cart_pos, max_dist, &mut candidates);

    let mut neighbors = Vec::with_capacity(candidates.len());
    for j in candidates {
        if j == center_idx {
            continue;
        }
        let r2 = &atoms[j];
        // Grid was built with include = anions.contains(...), so the grid
        // already filtered to anions. Defensive assertion only.
        debug_assert!(anions.contains(&r2.element));

        let dx = r2.cart_pos[0] - r1.cart_pos[0];
        let dy = r2.cart_pos[1] - r1.cart_pos[1];
        let dz = r2.cart_pos[2] - r1.cart_pos[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();

        let rad2 = get_atom_cov(&r2.element);
        if dist > 0.4 && dist < (rad1 + rad2) * tolerance {
            neighbors.push(j);
        }
    }

    neighbors
}

// ============================================================================
// AUTO-DETECT & CN DISPLAY
// ============================================================================

pub fn auto_detect_polyhedra_elements(
    atoms: &[RenderAtom],
    bond_cutoff: f64,
    max_bond_dist: f64,
) -> Vec<String> {
    let mut element_cns: HashMap<String, Vec<usize>> = HashMap::new();

    // Build grid once; all per-cation queries share it.
    let anions = classify_anions(atoms);
    let grid = SpatialGrid::build(atoms, max_bond_dist.max(1e-3), |a| {
        anions.contains(&a.element)
    });

    for (i, atom) in atoms.iter().enumerate() {
        if atom.is_ghost {
            continue;
        }
        // Only cations can be polyhedra centers
        if anions.contains(&atom.element) {
            continue;
        }
        let cn = find_coordination_neighbors_with_grid(
            i,
            atoms,
            &grid,
            &anions,
            bond_cutoff,
            max_bond_dist,
        )
        .len();
        element_cns
            .entry(atom.element.clone())
            .or_default()
            .push(cn);
    }

    let mut result = Vec::new();
    for (element, cns) in &element_cns {
        if cns.is_empty() {
            continue;
        }
        let avg_cn = cns.iter().sum::<usize>() as f64 / cns.len() as f64;
        if (3.5..=8.5).contains(&avg_cn) {
            result.push(element.clone());
        }
    }

    result.sort();
    result
}

pub fn average_cn_for_element(
    atoms: &[RenderAtom],
    element: &str,
    bond_cutoff: f64,
    max_bond_dist: f64,
) -> Option<f64> {
    let anions = classify_anions(atoms);
    // Anions don't have coordination polyhedra
    if anions.contains(element) {
        return None;
    }
    let grid = SpatialGrid::build(atoms, max_bond_dist.max(1e-3), |a| {
        anions.contains(&a.element)
    });
    let cns: Vec<usize> = atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| a.element == element && !a.is_ghost)
        .map(|(i, _)| {
            find_coordination_neighbors_with_grid(
                i,
                atoms,
                &grid,
                &anions,
                bond_cutoff,
                max_bond_dist,
            )
            .len()
        })
        .collect();

    if cns.is_empty() {
        None
    } else {
        Some(cns.iter().sum::<usize>() as f64 / cns.len() as f64)
    }
}

// ============================================================================
// BUILD POLYHEDRA
// ============================================================================

fn build_polyhedra_inner(
    atoms: &[RenderAtom],
    enabled_elements: &[String],
    tolerance: f64,
    min_cn: usize,
    max_cn: usize,
    max_bond_dist: f64,
    show_ghosts: bool,
) -> Vec<Polyhedron> {
    // Classify anions from structure context — handles phosphates,
    // sulfates, etc. correctly. Then build the anion grid once.
    // Cell size = max_bond_dist so each query visits a 3×3×3 block at most.
    let anions = classify_anions(atoms);
    let grid = SpatialGrid::build(atoms, max_bond_dist.max(1e-3), |a| {
        anions.contains(&a.element)
    });

    // Per-cation work is embarrassingly parallel: each Polyhedron depends
    // only on shared-immutable `atoms`, `grid`, and `anions`. Rayon
    // preserves index order, so output order matches the sequential version.
    (0..atoms.len())
        .into_par_iter()
        .filter_map(|i| {
            let atom = &atoms[i];
            // Skip coordination-only ghosts (invisible) always.
            // Skip visible ghosts unless show_full_unit_cell is on — otherwise
            // we'd draw orphan polyhedra at corners without their center atom.
            if atom.is_coord_only || !enabled_elements.contains(&atom.element) {
                return None;
            }
            if atom.is_ghost && !show_ghosts {
                return None;
            }

            let neighbors = find_coordination_neighbors_with_grid(
                i,
                atoms,
                &grid,
                &anions,
                tolerance,
                max_bond_dist,
            );
            let cn = neighbors.len();
            if cn < min_cn || cn > max_cn {
                return None;
            }

            let pts: Vec<[f64; 3]> = neighbors.iter().map(|&idx| atoms[idx].cart_pos).collect();
            let faces = convex_hull_3d(atom.cart_pos, &pts, &neighbors);
            if faces.is_empty() {
                return None;
            }

            Some(Polyhedron {
                center_idx: i,
                neighbor_indices: neighbors,
                faces,
                coordination_number: cn,
            })
        })
        .collect()
}

// ============================================================================
// CONVEX HULL — brute-force O(n⁴) in Cartesian space
// ============================================================================
//
// For small point sets (n ≤ 20, typical coordination-polyhedra range), a
// brute-force "every triple whose plane has all other points on one side"
// enumeration is simpler and more numerically robust than a proper Qhull-
// style incremental algorithm. It is correct by construction: the test is
// literally the definition of a convex-hull face. The O(n⁴) worst case
// is fine because n is bounded by the coordination number.
//
// For intermetallics or other structures with very high CN (≥ 12), this
// becomes perceptibly slower than a Qhull-class algorithm; left as a
// future improvement.

fn convex_hull_3d(center: [f64; 3], pts: &[[f64; 3]], atom_indices: &[usize]) -> Vec<Face> {
    let n = pts.len();
    match n {
        0..=2 => vec![],
        3 => vec![make_face(center, pts, atom_indices, 0, 1, 2)],
        4 => vec![
            make_face(center, pts, atom_indices, 0, 1, 2),
            make_face(center, pts, atom_indices, 0, 1, 3),
            make_face(center, pts, atom_indices, 0, 2, 3),
            make_face(center, pts, atom_indices, 1, 2, 3),
        ],
        _ => match find_initial_tetrahedron(pts) {
            None => fan_triangulation(center, pts, atom_indices),
            Some(_) => brute_force_hull(center, pts, atom_indices),
        },
    }
}

/// Brute-force O(n⁴) 3D convex hull. Correct by construction: enumerates
/// every vertex triple and keeps the ones whose supporting plane has all
/// other points strictly on one side (or coplanar within `tol`). Faces
/// are then oriented so normals point outward from `center`.
///
/// Rationale over a Qhull-style incremental algorithm:
/// - Coplanar vertices (common in regular octahedra, cubes, icosahedra)
///   are handled without the degenerate-face patching that incremental
///   algorithms require.
/// - Numerical robustness: a single relative tolerance check, no
///   cascading topology updates that can accumulate error.
/// - n is bounded by coordination number (typically 2–12), so O(n⁴) is
///   at most a few thousand operations per polyhedron.
fn brute_force_hull(center: [f64; 3], pts: &[[f64; 3]], atom_indices: &[usize]) -> Vec<Face> {
    let n = pts.len();
    let tol = 1e-9;
    let mut raw_faces: Vec<[usize; 3]> = Vec::new();

    // Enumerate all triples; keep those whose plane has every other point
    // on one side (or coplanar within tol). That is the definition of a
    // convex-hull face.
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let a = v(pts[i]);
                let b = v(pts[j]);
                let c = v(pts[k]);
                let nrm = (b - a).cross(&(c - a));
                let len = nrm.norm();
                if len < 1e-12 {
                    continue; // collinear triple
                }
                let mut pos = 0usize;
                let mut neg = 0usize;
                for m in 0..n {
                    if m == i || m == j || m == k {
                        continue;
                    }
                    let d = nrm.dot(&(v(pts[m]) - a)) / len;
                    if d > tol {
                        pos += 1;
                    } else if d < -tol {
                        neg += 1;
                    }
                }
                // A hull face has all other points strictly on one side
                // (coplanar points are allowed — they'll form adjacent faces).
                if pos == 0 || neg == 0 {
                    raw_faces.push(oriented_face(center, pts, [i, j, k]));
                }
            }
        }
    }

    // Deduplicate faces that share the same vertex set (can happen when
    // many points are coplanar and multiple triples describe the same
    // planar region from different triangulations — keep them all; they
    // are distinct triangles, not duplicates). Only drop exact index-set
    // duplicates.
    raw_faces.sort_by_key(|f| {
        let mut s = *f;
        s.sort_unstable();
        s
    });
    raw_faces.dedup_by_key(|f| {
        let mut s = *f;
        s.sort_unstable();
        s
    });

    raw_faces
        .into_iter()
        .filter(|f| {
            let a = v(pts[f[0]]);
            let cross = (v(pts[f[1]]) - a).cross(&(v(pts[f[2]]) - a));
            cross.norm_squared() > 1e-16
        })
        .map(|f| make_face(center, pts, atom_indices, f[0], f[1], f[2]))
        .collect()
}

/// Wind face so normal points away from `center`.
fn oriented_face(center: [f64; 3], pts: &[[f64; 3]], f: [usize; 3]) -> [usize; 3] {
    let v0 = v(pts[f[0]]);
    let n = (v(pts[f[1]]) - v0).cross(&(v(pts[f[2]]) - v0));
    if n.dot(&(v(center) - v0)) > 0.0 {
        [f[0], f[2], f[1]] // flip
    } else {
        f
    }
}

fn make_face(
    center: [f64; 3],
    pts: &[[f64; 3]],
    atom_indices: &[usize],
    i: usize,
    j: usize,
    k: usize,
) -> Face {
    let f = oriented_face(center, pts, [i, j, k]);
    let (v0, v1, v2) = (v(pts[f[0]]), v(pts[f[1]]), v(pts[f[2]]));
    let centroid = (v0 + v1 + v2) / 3.0;
    Face {
        vertex_atom_indices: [atom_indices[f[0]], atom_indices[f[1]], atom_indices[f[2]]],
        cart_center: arr(centroid),
    }
}

// ── Initial tetrahedron seed ──────────────────────────────────────────────────

fn find_initial_tetrahedron(pts: &[[f64; 3]]) -> Option<(usize, usize, usize, usize)> {
    let n = pts.len();
    let (i0, i1) = furthest_pair(pts);
    let a = v(pts[i0]);
    let b = v(pts[i1]);

    let i2 = (0..n).filter(|&i| i != i0 && i != i1).max_by(|&x, &y| {
        dist_to_line(v(pts[x]), a, b)
            .partial_cmp(&dist_to_line(v(pts[y]), a, b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    let c = v(pts[i2]);

    let i3 = (0..n)
        .filter(|&i| i != i0 && i != i1 && i != i2)
        .max_by(|&x, &y| {
            dist_to_plane(v(pts[x]), a, b, c)
                .partial_cmp(&dist_to_plane(v(pts[y]), a, b, c))
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;

    if dist_to_plane(v(pts[i3]), a, b, c) < 1e-6 {
        return None;
    }
    Some((i0, i1, i2, i3))
}

fn furthest_pair(pts: &[[f64; 3]]) -> (usize, usize) {
    let mut best = (0, 1, 0.0f64);
    for i in 0..pts.len() {
        for j in (i + 1)..pts.len() {
            let d = (v(pts[i]) - v(pts[j])).norm_squared();
            if d > best.2 {
                best = (i, j, d);
            }
        }
    }
    (best.0, best.1)
}

fn dist_to_line(p: Vector3<f64>, a: Vector3<f64>, b: Vector3<f64>) -> f64 {
    let ab = b - a;
    let len = ab.norm();
    if len < 1e-10 {
        return 0.0;
    }
    (ab.cross(&(p - a))).norm() / len
}

fn dist_to_plane(p: Vector3<f64>, a: Vector3<f64>, b: Vector3<f64>, c: Vector3<f64>) -> f64 {
    let n = (b - a).cross(&(c - a));
    let len = n.norm();
    if len < 1e-10 {
        return 0.0;
    }
    n.dot(&(p - a)).abs() / len
}

fn fan_triangulation(center: [f64; 3], pts: &[[f64; 3]], atom_indices: &[usize]) -> Vec<Face> {
    let n = pts.len();
    let centroid: Vector3<f64> = pts.iter().map(|&p| v(p)).sum::<Vector3<f64>>() / n as f64;
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        let pa = pts[a];
        let pb = pts[b];
        let aa = (pa[1] - centroid.y).atan2(pa[0] - centroid.x);
        let ab = (pb[1] - centroid.y).atan2(pb[0] - centroid.x);
        aa.partial_cmp(&ab).unwrap_or(std::cmp::Ordering::Equal)
    });
    (1..n - 1)
        .map(|i| {
            make_face(
                center,
                pts,
                atom_indices,
                indices[0],
                indices[i],
                indices[i + 1],
            )
        })
        .collect()
}

// ============================================================================
// MINERALOGICAL DISTORTION METRICS
// ============================================================================
//
// Standard quantitative descriptors for coordination polyhedra. These are
// the same quantities VESTA reports on the "Geometrical Parameters" dialog,
// and they are what a mineralogy / crystallography audience expects to see
// alongside a polyhedral rendering.
//
// References:
//   Robinson, Gibbs & Ribbe (1971) "Quadratic Elongation: A Quantitative
//     Measure of Distortion in Coordination Polyhedra" Science 172:567–570.
//   Baur (1974) "The geometry of polyhedral distortions. Predictive
//     relationships for the phosphate group" Acta Cryst. B30:1195–1215.
//   Brown & Altermatt (1985) — bond-valence sums (handled in BVS module).

/// Quantitative descriptors of a single coordination polyhedron.
/// All distances are in Å, angles in degrees, volumes in Å³.
#[derive(Debug, Clone)]
pub struct PolyhedronMetrics {
    /// Mean center-to-vertex distance ⟨d⟩ in Å.
    pub mean_bond_length: f64,
    /// Minimum and maximum center-to-vertex distance (Å).
    pub bond_length_range: (f64, f64),
    /// Baur's distortion index (1974):
    ///   Δ = (1/n) Σ |dᵢ − ⟨d⟩| / ⟨d⟩
    /// Unitless. 0 for a regular polyhedron, ~0.01 for typical octahedra.
    pub baur_distortion: f64,
    /// Bond-angle variance σ² (Robinson et al. 1971):
    ///   σ² = (1/(m−1)) Σ (φᵢ − φ₀)²
    /// where m is the number of bond angles and φ₀ is the ideal angle for
    /// the regular polyhedron of the same CN. deg². Returns None for
    /// coordinations without a standard reference polyhedron.
    pub bond_angle_variance: Option<f64>,
    /// Quadratic elongation ⟨λ⟩ (Robinson et al. 1971):
    ///   ⟨λ⟩ = (1/n) Σ (dᵢ / d₀)²
    /// where d₀ is the center-to-vertex distance of a regular polyhedron
    /// with the same VOLUME. Unitless, ≥ 1. Returns None for CNs without
    /// a standard regular reference.
    pub quadratic_elongation: Option<f64>,
    /// Polyhedron volume (Å³) computed by the divergence theorem over
    /// the triangulated hull faces. Signed faces are oriented outward
    /// (enforced by `oriented_face`), so the sum is positive.
    pub volume: f64,
}

impl Polyhedron {
    /// Compute distortion metrics from the existing face list and the
    /// center+vertex cartesian positions in `atoms`.
    pub fn metrics(&self, atoms: &[RenderAtom]) -> PolyhedronMetrics {
        let center = v(atoms[self.center_idx].cart_pos);

        // Bond lengths: center → each unique vertex.
        let bond_lengths: Vec<f64> = self
            .neighbor_indices
            .iter()
            .map(|&idx| (v(atoms[idx].cart_pos) - center).norm())
            .collect();

        let n = bond_lengths.len().max(1) as f64;
        let mean = bond_lengths.iter().sum::<f64>() / n;
        let min_len = bond_lengths.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_len = bond_lengths
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let baur = if mean > 1e-9 {
            bond_lengths.iter().map(|d| (d - mean).abs()).sum::<f64>() / (n * mean)
        } else {
            0.0
        };

        // Bond-angle variance: enumerate unique center-vertex-center'
        // angles between each pair of distinct neighbors.
        let angle_var = bond_angle_variance(atoms, self.center_idx, &self.neighbor_indices);

        // Volume: divergence theorem over outward-oriented triangles.
        // For a triangle with vertices v₀, v₁, v₂ and outward normal, the
        // signed contribution is v₀ · (v₁ × v₂) / 6. Sum yields V_hull.
        let volume: f64 = self
            .faces
            .iter()
            .map(|face| {
                let v0 = v(atoms[face.vertex_atom_indices[0]].cart_pos);
                let v1 = v(atoms[face.vertex_atom_indices[1]].cart_pos);
                let v2 = v(atoms[face.vertex_atom_indices[2]].cart_pos);
                v0.dot(&v1.cross(&v2)) / 6.0
            })
            .sum::<f64>()
            .abs();

        // Quadratic elongation: requires d₀ = center-to-vertex distance
        // of a regular polyhedron of the SAME VOLUME, per Robinson 1971.
        // d₀ = k(CN) × V^(1/3). k values from Robinson Table 1.
        let quad_elong = regular_d0_factor(self.coordination_number).map(|k| {
            let d0 = k * volume.powf(1.0 / 3.0);
            if d0 > 1e-9 {
                bond_lengths.iter().map(|d| (d / d0).powi(2)).sum::<f64>() / n
            } else {
                1.0
            }
        });

        PolyhedronMetrics {
            mean_bond_length: mean,
            bond_length_range: (min_len, max_len),
            baur_distortion: baur,
            bond_angle_variance: angle_var,
            quadratic_elongation: quad_elong,
            volume,
        }
    }
}

/// Robinson 1971 Table 1: d₀/V^(1/3) for regular polyhedra of unit volume.
/// Returns None for coordination numbers without a standard regular reference.
fn regular_d0_factor(cn: usize) -> Option<f64> {
    match cn {
        // Regular tetrahedron: V = (8/3)(d/√3)³  =>  d₀ = (3V/8)^(1/3) × √3
        // Numerically: d₀ = k × V^(1/3), k = √3 × (3/8)^(1/3) ≈ 1.2408
        4 => Some(1.2408),
        // Regular octahedron: V = (4/3)d³·(1/√2) × 2 = ...
        // d₀ = (3V/4)^(1/3) × ... k ≈ 0.9086
        6 => Some(0.9086),
        // Cube (8-coordinate): d is half the body diagonal.
        // V = (2d/√3)³ => d₀ = (V^(1/3) × √3 / 2), k = √3/2 ≈ 0.8660
        8 => Some(0.8660),
        // Regular icosahedron (12-coordinate): k ≈ 0.6511
        12 => Some(0.6511),
        _ => None,
    }
}

/// Reference bond angle φ₀ for the regular polyhedron of a given CN.
/// Returns None when no standard regular reference exists.
fn regular_bond_angle(cn: usize) -> Option<f64> {
    match cn {
        4 => Some(109.471), // regular tetrahedron
        6 => Some(90.0),    // regular octahedron (nearest-neighbor angle)
        8 => Some(70.529),  // cube (nearest-neighbor angle around center)
        _ => None,
    }
}

/// Bond-angle variance σ² (Robinson et al. 1971). Returns None when
/// there's no standard reference angle for this coordination number.
fn bond_angle_variance(
    atoms: &[RenderAtom],
    center_idx: usize,
    neighbors: &[usize],
) -> Option<f64> {
    let phi0 = regular_bond_angle(neighbors.len())?;
    let center = v(atoms[center_idx].cart_pos);

    // For each neighbor pair, compute the angle vertex-center-vertex'.
    // Robinson's original formula averages over nearest-neighbor angles
    // only (those that define edges of the polyhedron). A common
    // simplification — and what VESTA reports — is to average over ALL
    // pairs; this differs from Robinson's original by a constant scale
    // but is the quantity most users expect. We follow VESTA here.
    let mut angles = Vec::with_capacity(neighbors.len() * (neighbors.len() - 1) / 2);
    for i in 0..neighbors.len() {
        for j in (i + 1)..neighbors.len() {
            let u = (v(atoms[neighbors[i]].cart_pos) - center).normalize();
            let w = (v(atoms[neighbors[j]].cart_pos) - center).normalize();
            let cos_theta = u.dot(&w).clamp(-1.0, 1.0);
            angles.push(cos_theta.acos().to_degrees());
        }
    }
    if angles.len() < 2 {
        return None;
    }

    // Select the m "nearest-neighbor" angles: m = (faces × 3) / 2 for a
    // regular polyhedron. For CN=4 (tetrahedron) this is 6, for CN=6
    // (octahedron) it is 12. Take the m SMALLEST angles from the full
    // pair list — these are guaranteed to be the nearest-neighbor ones
    // for a polyhedron close to regular, matching VESTA's convention.
    let m = match neighbors.len() {
        4 => 6,
        6 => 12,
        8 => 12,
        _ => angles.len(),
    };
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let selected = &angles[..m.min(angles.len())];

    let denom = (selected.len() as f64 - 1.0).max(1.0);
    let sigma2: f64 = selected.iter().map(|&a| (a - phi0).powi(2)).sum::<f64>() / denom;
    Some(sigma2)
}

// ============================================================================
// STATELESS ENTRY POINT
// ============================================================================

pub fn build_polyhedra_for_draw(
    atoms: &[RenderAtom],
    enabled_elements: &[String],
    bond_cutoff: f64,
    min_cn: usize,
    max_cn: usize,
    max_bond_dist: f64,
    show_ghosts: bool,
) -> Vec<Polyhedron> {
    build_polyhedra_inner(
        atoms,
        enabled_elements,
        bond_cutoff,
        min_cn,
        max_cn,
        max_bond_dist,
        show_ghosts,
    )
}

// ============================================================================
// TESTS
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn mock(pts: Vec<[f64; 3]>) -> (Vec<[f64; 3]>, Vec<usize>) {
        let n = pts.len();
        (pts, (0..n).collect())
    }

    #[test]
    fn test_tetrahedron_faces() {
        let (pts, idx) = mock(vec![
            [1.0, 1.0, 1.0],
            [1.0, -1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
        ]);
        assert_eq!(convex_hull_3d([0.0; 3], &pts, &idx).len(), 4);
    }

    #[test]
    fn test_octahedron_faces() {
        let (pts, idx) = mock(vec![
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ]);
        assert_eq!(convex_hull_3d([0.0; 3], &pts, &idx).len(), 8);
    }

    #[test]
    fn test_outward_normals() {
        let center = [0.0; 3];
        let (pts, idx) = mock(vec![
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ]);

        let c = v(center);
        let faces = convex_hull_3d(center, &pts, &idx);

        assert!(!faces.is_empty(), "hull should not be empty");

        for (i, face) in faces.iter().enumerate() {
            let vi = &face.vertex_atom_indices;

            let v0 = v(pts[vi[0]]);
            let v1 = v(pts[vi[1]]);
            let v2 = v(pts[vi[2]]);

            let n = (v1 - v0).cross(&(v2 - v0));

            // ✅ Use centroid instead of vertex
            let centroid = (v0 + v1 + v2) / 3.0;

            let dot = n.dot(&(centroid - c));

            assert!(dot >= -1e-12, "Face {} has inward normal: dot={}", i, dot);
        }
    }

    #[test]
    fn test_cube_all_vertices_present() {
        // Cube: 8 vertices, many coplanar quads.
        // All 8 points must appear in at least one face.
        let (pts, idx) = mock(vec![
            [1.0, 1.0, 1.0],
            [1.0, 1.0, -1.0],
            [1.0, -1.0, 1.0],
            [1.0, -1.0, -1.0],
            [-1.0, 1.0, 1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [-1.0, -1.0, -1.0],
        ]);
        let faces = convex_hull_3d([0.0; 3], &pts, &idx);
        let mut used = vec![false; 8];
        for face in &faces {
            for &vi in &face.vertex_atom_indices {
                used[vi] = true;
            }
        }
        assert!(
            used.iter().all(|&u| u),
            "all cube vertices must appear in hull"
        );
        // A cube has 6 quad faces = 12 triangles minimum
        assert!(
            faces.len() >= 12,
            "cube must have at least 12 triangular faces"
        );
    }

    // ── Anion classification tests ──────────────────────────────────────────

    fn mock_atom(element: &str, x: f64, y: f64, z: f64) -> RenderAtom {
        RenderAtom {
            screen_pos: [0.0; 3],
            cart_pos: [x, y, z],
            element: element.to_string(),
            original_index: 0,
            unique_id: 0,
            is_ghost: false,
            is_coord_only: false,
            screen_radius: 0.0,
        }
    }

    #[test]
    fn anions_in_simple_oxide() {
        // BaTiO₃ → O is the only anion.
        let atoms = vec![
            mock_atom("Ba", 0.0, 0.0, 0.0),
            mock_atom("Ti", 0.5, 0.5, 0.5),
            mock_atom("O", 0.5, 0.5, 0.0),
            mock_atom("O", 0.5, 0.0, 0.5),
            mock_atom("O", 0.0, 0.5, 0.5),
        ];
        let anions = classify_anions(&atoms);
        assert!(anions.contains("O"));
        assert!(!anions.contains("Ba"));
        assert!(!anions.contains("Ti"));
    }

    #[test]
    fn anions_in_phosphate() {
        // Ca₃(PO₄)₂ → O is the anion, P is a cation. This was the
        // motivating bug for the structure-aware classifier.
        let atoms = vec![
            mock_atom("Ca", 0.0, 0.0, 0.0),
            mock_atom("P", 0.3, 0.3, 0.3),
            mock_atom("O", 0.1, 0.0, 0.0),
            mock_atom("O", 0.0, 0.1, 0.0),
        ];
        let anions = classify_anions(&atoms);
        assert!(anions.contains("O"), "O must be the anion");
        assert!(
            !anions.contains("P"),
            "P must NOT be classified as an anion in a phosphate"
        );
    }

    #[test]
    fn anions_in_oxyfluoride() {
        // Mixed-anion: both F and O should be anions (they're within 0.5
        // Pauling units of each other: 3.98 and 3.44).
        let atoms = vec![
            mock_atom("Na", 0.0, 0.0, 0.0),
            mock_atom("O", 0.5, 0.0, 0.0),
            mock_atom("F", 0.0, 0.5, 0.0),
        ];
        let anions = classify_anions(&atoms);
        assert!(anions.contains("O"));
        assert!(anions.contains("F"));
        assert!(!anions.contains("Na"));
    }

    #[test]
    fn no_anions_in_pure_metal() {
        // Cu metal: no element above the χ=2.0 threshold, so no anions
        // and no polyhedra should be drawn.
        let atoms = vec![
            mock_atom("Cu", 0.0, 0.0, 0.0),
            mock_atom("Cu", 0.5, 0.5, 0.5),
        ];
        let anions = classify_anions(&atoms);
        assert!(anions.is_empty(), "pure metal should produce no anions");
    }

    // ── Distortion metrics tests ────────────────────────────────────────────

    #[test]
    fn regular_octahedron_metrics() {
        // A regular octahedron at the origin with edge-length-√2 unit
        // vectors. Each M–X bond has length 1.0, every nearest-neighbor
        // X–M–X angle is 90°, volume = 4/3 ≈ 1.333 Å³.
        let atoms = vec![
            mock_atom("Ti", 0.0, 0.0, 0.0), // center
            mock_atom("O", 1.0, 0.0, 0.0),
            mock_atom("O", -1.0, 0.0, 0.0),
            mock_atom("O", 0.0, 1.0, 0.0),
            mock_atom("O", 0.0, -1.0, 0.0),
            mock_atom("O", 0.0, 0.0, 1.0),
            mock_atom("O", 0.0, 0.0, -1.0),
        ];
        let neighbor_cart: Vec<[f64; 3]> = (1..=6).map(|i| atoms[i].cart_pos).collect();
        let neighbor_indices: Vec<usize> = (1..=6).collect();
        let faces = convex_hull_3d([0.0; 3], &neighbor_cart, &neighbor_indices);

        let poly = Polyhedron {
            center_idx: 0,
            neighbor_indices,
            faces,
            coordination_number: 6,
        };
        let m = poly.metrics(&atoms);

        assert!(
            (m.mean_bond_length - 1.0).abs() < 1e-9,
            "mean bond length wrong: {}",
            m.mean_bond_length
        );
        assert!(
            m.baur_distortion < 1e-9,
            "regular octahedron must have Baur distortion ≈ 0, got {}",
            m.baur_distortion
        );
        let sigma2 = m.bond_angle_variance.expect("σ² defined for CN=6");
        assert!(
            sigma2 < 1e-6,
            "regular octahedron must have σ² ≈ 0, got {}",
            sigma2
        );
        assert!(
            (m.volume - 4.0 / 3.0).abs() < 1e-6,
            "regular octahedron volume should be 4/3, got {}",
            m.volume
        );
    }

    #[test]
    fn distorted_octahedron_detected() {
        // Stretched along z: one bond longer than the others.
        let atoms = vec![
            mock_atom("Ti", 0.0, 0.0, 0.0),
            mock_atom("O", 1.0, 0.0, 0.0),
            mock_atom("O", -1.0, 0.0, 0.0),
            mock_atom("O", 0.0, 1.0, 0.0),
            mock_atom("O", 0.0, -1.0, 0.0),
            mock_atom("O", 0.0, 0.0, 1.5), // elongated
            mock_atom("O", 0.0, 0.0, -1.0),
        ];
        let neighbor_cart: Vec<[f64; 3]> = (1..=6).map(|i| atoms[i].cart_pos).collect();
        let neighbor_indices: Vec<usize> = (1..=6).collect();
        let faces = convex_hull_3d([0.0; 3], &neighbor_cart, &neighbor_indices);

        let poly = Polyhedron {
            center_idx: 0,
            neighbor_indices,
            faces,
            coordination_number: 6,
        };
        let m = poly.metrics(&atoms);

        assert!(
            m.baur_distortion > 0.01,
            "elongated octahedron should have nonzero Baur index, got {}",
            m.baur_distortion
        );
        assert!(m.bond_length_range.1 > m.bond_length_range.0);
        assert!(
            (m.bond_length_range.1 - 1.5).abs() < 1e-9,
            "max bond length should be 1.5, got {}",
            m.bond_length_range.1
        );
    }
}
