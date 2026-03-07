// src/rendering/polyhedra.rs
// - Convex hull in Cartesian space, incremental algorithm
// - Face vertices stored as atom indices (O(1) screen_pos lookup)
// - Bond detection: covalent radii + same-large-element suppression + user distance cap
// - All vector math via nalgebra::Vector3

use crate::model::elements::get_atom_cov;
use crate::rendering::scene::RenderAtom;
use nalgebra::Vector3;
use std::collections::HashMap;

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

// ── Anion classification (mirrors BVS calculator) ────────────────────────────

fn is_anion(element: &str) -> bool {
    matches!(
        element,
        "O" | "S" | "Se" | "Te" | "F" | "Cl" | "Br" | "I" | "N" | "P" | "As"
    )
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
pub fn find_coordination_neighbors(
    center_idx: usize,
    atoms: &[RenderAtom],
    tolerance: f64,
    max_dist: f64,
) -> Vec<usize> {
    let r1 = &atoms[center_idx];

    // Only cations can be polyhedra centers
    if is_anion(&r1.element) {
        return Vec::new();
    }

    let rad1 = get_atom_cov(&r1.element);
    let p1 = v(r1.cart_pos);
    let mut neighbors = Vec::new();

    for (j, r2) in atoms.iter().enumerate() {
        if center_idx == j {
            continue;
        }
        // Only anions as vertices (cation-anion bonds only)
        if !is_anion(&r2.element) {
            continue;
        }
        let dist = (v(r2.cart_pos) - p1).norm();
        if dist > max_dist {
            continue;
        }
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

    for (i, atom) in atoms.iter().enumerate() {
        if atom.is_ghost {
            continue;
        }
        // Only cations can be polyhedra centers
        if is_anion(&atom.element) {
            continue;
        }
        let cn = find_coordination_neighbors(i, atoms, bond_cutoff, max_bond_dist).len();
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
        if avg_cn >= 3.5 && avg_cn <= 8.5 {
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
    // Anions don't have coordination polyhedra
    if is_anion(element) {
        return None;
    }
    let cns: Vec<usize> = atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| a.element == element && !a.is_ghost)
        .map(|(i, _)| find_coordination_neighbors(i, atoms, bond_cutoff, max_bond_dist).len())
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
    let mut polyhedra = Vec::new();

    for (i, atom) in atoms.iter().enumerate() {
        // Skip coordination-only ghosts (invisible) always.
        // Skip visible ghosts unless show_full_unit_cell is on — otherwise
        // we'd draw orphan polyhedra at corners without their center atom.
        if atom.is_coord_only || !enabled_elements.contains(&atom.element) {
            continue;
        }
        if atom.is_ghost && !show_ghosts {
            continue;
        }

        let neighbors = find_coordination_neighbors(i, atoms, tolerance, max_bond_dist);
        let cn = neighbors.len();

        if cn < min_cn || cn > max_cn {
            continue;
        }

        let pts: Vec<[f64; 3]> = neighbors.iter().map(|&idx| atoms[idx].cart_pos).collect();
        let faces = convex_hull_3d(atom.cart_pos, &pts, &neighbors);

        if faces.is_empty() {
            continue;
        }

        polyhedra.push(Polyhedron {
            center_idx: i,
            neighbor_indices: neighbors,
            faces,
            coordination_number: cn,
        });
    }

    polyhedra
}

// ============================================================================
// CONVEX HULL — incremental, Cartesian space
// ============================================================================

fn convex_hull_3d(center: [f64; 3], pts: &[[f64; 3]], atom_indices: &[usize]) -> Vec<Face> {
    let n = pts.len();
    match n {
        0 | 1 | 2 => vec![],
        3 => vec![make_face(center, pts, atom_indices, 0, 1, 2)],
        4 => vec![
            make_face(center, pts, atom_indices, 0, 1, 2),
            make_face(center, pts, atom_indices, 0, 1, 3),
            make_face(center, pts, atom_indices, 0, 2, 3),
            make_face(center, pts, atom_indices, 1, 2, 3),
        ],
        _ => match find_initial_tetrahedron(pts) {
            None => fan_triangulation(center, pts, atom_indices),
            Some((i0, i1, i2, i3)) => incremental_hull(center, pts, atom_indices, i0, i1, i2, i3),
        },
    }
}

fn incremental_hull(
    center: [f64; 3],
    pts: &[[f64; 3]],
    atom_indices: &[usize],
    i0: usize,
    i1: usize,
    i2: usize,
    i3: usize,
) -> Vec<Face> {
    let mut hull: Vec<[usize; 3]> = vec![
        oriented_face(center, pts, [i0, i1, i2]),
        oriented_face(center, pts, [i0, i1, i3]),
        oriented_face(center, pts, [i0, i2, i3]),
        oriented_face(center, pts, [i1, i2, i3]),
    ];

    for pi in 0..pts.len() {
        if [i0, i1, i2, i3].contains(&pi) {
            continue;
        }
        let p = pts[pi];

        let visible: Vec<usize> = hull
            .iter()
            .enumerate()
            .filter(|(_, f)| face_visible_from(pts, f, p))
            .map(|(i, _)| i)
            .collect();

        if visible.is_empty() {
            continue;
        }

        let mut horizon: Vec<[usize; 2]> = Vec::new();
        for &vi in &visible {
            for edge in face_edges(hull[vi]) {
                let shared = visible
                    .iter()
                    .filter(|&&vj| vj != vi && face_has_edge(hull[vj], edge))
                    .count();
                if shared == 0 {
                    horizon.push(edge);
                }
            }
        }

        let mut vis_sorted = visible.clone();
        vis_sorted.sort_unstable();
        for &vi in vis_sorted.iter().rev() {
            hull.swap_remove(vi);
        }
        for edge in horizon {
            hull.push(oriented_face(center, pts, [edge[0], edge[1], pi]));
        }
    }

    // Post-process: subdivide faces to include any coplanar points the
    // incremental algorithm skipped (e.g. equatorial vertices of a regular
    // octahedron).
    ensure_all_vertices(center, pts, &mut hull);

    // Filter out degenerate (zero/near-zero area) sub-triangles that arise
    // when a coplanar vertex sits exactly on an existing edge.  These have
    // collinear vertices, so oriented_face / face_normal produce unstable
    // results that flicker on rotation.
    hull.into_iter()
        .filter(|f| {
            let a = v(pts[f[0]]);
            let cross = (v(pts[f[1]]) - a).cross(&(v(pts[f[2]]) - a));
            cross.norm_squared() > 1e-16
        })
        .map(|f| make_face(center, pts, atom_indices, f[0], f[1], f[2]))
        .collect()
}

fn face_visible_from(pts: &[[f64; 3]], f: &[usize; 3], p: [f64; 3]) -> bool {
    let v0 = v(pts[f[0]]);
    let n = (v(pts[f[1]]) - v0).cross(&(v(pts[f[2]]) - v0));
    n.dot(&(v(p) - v0)) > 1e-10
}

/// After building the convex hull, ensure every input point appears in at least
/// one face.  Points that lie exactly in the plane of an existing face (common
/// in regular octahedra, cubes, etc.) may be skipped by the incremental
/// algorithm.  For each missing point we find the face it is coplanar with and
/// subdivide that face to include it.
fn ensure_all_vertices(center: [f64; 3], pts: &[[f64; 3]], hull: &mut Vec<[usize; 3]>) {
    let n_pts = pts.len();
    // Iterate until no more missing points (rare: usually one pass suffices)
    for _round in 0..4 {
        // Collect which point indices appear in the hull
        let mut present = vec![false; n_pts];
        for f in hull.iter() {
            for &vi in f {
                if vi < n_pts {
                    present[vi] = true;
                }
            }
        }

        let missing: Vec<usize> = (0..n_pts).filter(|&i| !present[i]).collect();
        if missing.is_empty() {
            return;
        }

        for &mi in &missing {
            let p = v(pts[mi]);
            // Find the face this point is coplanar with (smallest |dot| product)
            let mut best_fi: Option<usize> = None;
            let mut best_dist = f64::MAX;

            for (fi, f) in hull.iter().enumerate() {
                let v0 = v(pts[f[0]]);
                let n = (v(pts[f[1]]) - v0).cross(&(v(pts[f[2]]) - v0));
                let len = n.norm();
                if len < 1e-14 {
                    continue; // degenerate face
                }
                let d = (n.dot(&(p - v0)) / len).abs();
                if d < best_dist {
                    best_dist = d;
                    best_fi = Some(fi);
                }
            }

            if let Some(fi) = best_fi {
                // Subdivide the face [a, b, c] into three faces:
                //   [a, b, mi], [b, c, mi], [c, a, mi]
                let f = hull[fi];
                let new_faces = [
                    oriented_face(center, pts, [f[0], f[1], mi]),
                    oriented_face(center, pts, [f[1], f[2], mi]),
                    oriented_face(center, pts, [f[2], f[0], mi]),
                ];
                hull[fi] = new_faces[0]; // replace original
                hull.push(new_faces[1]);
                hull.push(new_faces[2]);
            }
        }
    }
}

fn face_edges(f: [usize; 3]) -> [[usize; 2]; 3] {
    [[f[0], f[1]], [f[1], f[2]], [f[2], f[0]]]
}

fn face_has_edge(f: [usize; 3], edge: [usize; 2]) -> bool {
    face_edges(f)
        .iter()
        .any(|e| (e[0] == edge[0] && e[1] == edge[1]) || (e[0] == edge[1] && e[1] == edge[0]))
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
        for face in convex_hull_3d(center, &pts, &idx) {
            let v0 = v(pts[face.vertex_atom_indices[0]]);
            let v1 = v(pts[face.vertex_atom_indices[1]]);
            let v2 = v(pts[face.vertex_atom_indices[2]]);
            let n = (v1 - v0).cross(&(v2 - v0));
            assert!(n.dot(&(v0 - v(center))) > 0.0, "normal must point outward");
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
}
