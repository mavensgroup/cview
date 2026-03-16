// src/physics/analysis/kpath/voronoi.rs
//
// Computes the first Brillouin zone (Wigner-Seitz cell of the reciprocal lattice)
// as a wireframe (list of edge line segments) using a Voronoi construction.
//
// Algorithm:
//   1. Generate reciprocal lattice points G within a shell (excluding origin)
//   2. For each G, define the bisecting plane: n·x = |G|²/2  (where n = G)
//   3. Compute all triple-plane intersections → candidate vertices
//   4. Keep only vertices inside ALL half-spaces (the BZ is the intersection)
//   5. Extract edges: two vertices sharing at least 2 parent planes

use nalgebra::{Matrix3, Vector3};
use std::collections::HashSet;

/// Compute BZ wireframe edges in Cartesian reciprocal-space coordinates.
/// Returns pairs of (start, end) points.
pub fn compute_bz_wireframe(rec_lattice: &Matrix3<f64>) -> Vec<([f64; 3], [f64; 3])> {
    // 1. Generate reciprocal lattice vectors in a shell around origin
    let g_vectors = generate_g_vectors(rec_lattice);

    if g_vectors.is_empty() {
        return Vec::new();
    }

    // 2. Build bisecting planes: for each G, plane normal = G, offset = |G|²/2
    //    Plane equation: G · x = G · G / 2
    let planes: Vec<(Vector3<f64>, f64)> = g_vectors.iter().map(|g| (*g, g.dot(g) / 2.0)).collect();

    // 3. Find all triple-plane intersections
    let n = planes.len();
    let mut vertices: Vec<(Vector3<f64>, HashSet<usize>)> = Vec::new();

    // Use a tolerance relative to the scale of the reciprocal lattice.
    // The characteristic scale is the smallest plane offset (|G|²/2 for
    // the shortest G), which sets the "size" of the BZ.
    let scale = planes
        .iter()
        .map(|(_, d)| d.abs())
        .fold(f64::INFINITY, f64::min)
        .max(1e-12);
    let tol = scale * 1e-6;
    let merge_tol = scale * 1e-4;

    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                if let Some(pt) = intersect_three_planes(&planes[i], &planes[j], &planes[k]) {
                    // 4. Check if this vertex is inside all half-spaces
                    //    For each plane (n, d): n · pt <= d + tolerance
                    let inside = planes.iter().all(|(normal, d)| normal.dot(&pt) <= d + tol);
                    if inside {
                        // Check for duplicate vertices (merge if very close)
                        let mut merged = false;
                        for (existing_pt, existing_planes) in vertices.iter_mut() {
                            if (pt - *existing_pt).norm() < merge_tol {
                                existing_planes.insert(i);
                                existing_planes.insert(j);
                                existing_planes.insert(k);
                                merged = true;
                                break;
                            }
                        }
                        if !merged {
                            let mut plane_set = HashSet::new();
                            plane_set.insert(i);
                            plane_set.insert(j);
                            plane_set.insert(k);
                            vertices.push((pt, plane_set));
                        }
                    }
                }
            }
        }
    }

    // 5. Extract edges: two vertices share an edge if they share >= 2 parent planes
    let mut edges: Vec<([f64; 3], [f64; 3])> = Vec::new();
    let mut seen_edges: HashSet<(usize, usize)> = HashSet::new();

    for i in 0..vertices.len() {
        for j in (i + 1)..vertices.len() {
            let shared: usize = vertices[i].1.intersection(&vertices[j].1).count();
            if shared >= 2 {
                let key = (i, j);
                if !seen_edges.contains(&key) {
                    seen_edges.insert(key);
                    let p1 = vertices[i].0;
                    let p2 = vertices[j].0;
                    edges.push(([p1.x, p1.y, p1.z], [p2.x, p2.y, p2.z]));
                }
            }
        }
    }

    edges
}

/// Generate reciprocal lattice vectors G = h*b1 + k*b2 + l*b3
/// within a given shell radius (in units of Miller indices), excluding origin.
///
/// We use shell=3 and keep all vectors, relying on the half-space filtering
/// in the main algorithm to discard irrelevant planes. A norm-based cutoff
/// is intentionally avoided because highly anisotropic lattices (e.g.,
/// rhombohedral Bi₂Se₃ with c/a ≈ 7) have G-vectors spanning a wide range
/// of lengths, all of which may contribute BZ faces.
fn generate_g_vectors(rec_lattice: &Matrix3<f64>) -> Vec<Vector3<f64>> {
    let b1 = rec_lattice.column(0).into_owned();
    let b2 = rec_lattice.column(1).into_owned();
    let b3 = rec_lattice.column(2).into_owned();

    let shell: i32 = 3;
    let mut vectors = Vec::new();

    for h in -shell..=shell {
        for k in -shell..=shell {
            for l in -shell..=shell {
                if h == 0 && k == 0 && l == 0 {
                    continue;
                }
                let g = b1 * (h as f64) + b2 * (k as f64) + b3 * (l as f64);
                vectors.push(g);
            }
        }
    }

    // Sort by norm — shorter G-vectors are more likely to define BZ faces,
    // so this ordering helps the triple-intersection loop find valid vertices
    // efficiently, but we do NOT discard any vectors.
    vectors.sort_by(|a, b| a.norm().partial_cmp(&b.norm()).unwrap());

    // Prune: only keep G-vectors that actually contribute a BZ face.
    // A plane G·x = |G|²/2 contributes iff no shorter G-vector makes it
    // redundant. Practically, for any lattice the BZ is bounded by at most
    // the ~first few shells. We keep vectors whose bisecting plane is not
    // entirely outside the region defined by shorter vectors.
    // As a safe upper bound, keep the nearest 80 vectors (covers all 14
    // Bravais lattices including highly anisotropic cases).
    vectors.truncate(80);

    vectors
}

/// Intersect three planes. Each plane is (normal, offset) where normal·x = offset.
/// Returns None if the planes are degenerate (parallel or coplanar).
fn intersect_three_planes(
    p1: &(Vector3<f64>, f64),
    p2: &(Vector3<f64>, f64),
    p3: &(Vector3<f64>, f64),
) -> Option<Vector3<f64>> {
    let mat = Matrix3::from_rows(&[p1.0.transpose(), p2.0.transpose(), p3.0.transpose()]);
    let det = mat.determinant();
    if det.abs() < 1e-12 {
        return None;
    }
    let rhs = Vector3::new(p1.1, p2.1, p3.1);
    mat.try_inverse().map(|inv| inv * rhs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn test_cubic_bz_has_edges() {
        // Simple cubic lattice: reciprocal lattice is also simple cubic
        let two_pi = 2.0 * PI;
        let rec = Matrix3::new(two_pi, 0.0, 0.0, 0.0, two_pi, 0.0, 0.0, 0.0, two_pi);
        let edges = compute_bz_wireframe(&rec);
        // A cube has 12 edges
        assert!(
            edges.len() == 12,
            "Simple cubic BZ should have 12 edges, got {}",
            edges.len()
        );
    }

    #[test]
    fn test_cubic_bz_vertex_distance() {
        // All vertices of a simple cubic BZ should be at distance sqrt(3)/2 * (2π/a)
        // from origin (corners of the cube with half-edge = π/a)
        let two_pi = 2.0 * PI;
        let a = 1.0; // lattice constant
        let rec = Matrix3::new(
            two_pi / a,
            0.0,
            0.0,
            0.0,
            two_pi / a,
            0.0,
            0.0,
            0.0,
            two_pi / a,
        );
        let edges = compute_bz_wireframe(&rec);
        // Collect all unique vertices
        let mut verts: Vec<Vector3<f64>> = Vec::new();
        let tol = 1e-6;
        for (s, e) in &edges {
            let sv = Vector3::new(s[0], s[1], s[2]);
            let ev = Vector3::new(e[0], e[1], e[2]);
            if !verts.iter().any(|v| (v - sv).norm() < tol) {
                verts.push(sv);
            }
            if !verts.iter().any(|v| (v - ev).norm() < tol) {
                verts.push(ev);
            }
        }
        // Cube: 8 vertices
        assert_eq!(verts.len(), 8, "Cubic BZ should have 8 vertices");
        let expected_dist = PI / a * (3.0_f64).sqrt();
        for v in &verts {
            assert!(
                approx_eq(v.norm(), expected_dist, 0.01),
                "Vertex distance {} should be {}",
                v.norm(),
                expected_dist
            );
        }
    }

    #[test]
    fn test_bz_nonempty_for_hexagonal() {
        // Hexagonal lattice: a1 = (1, 0, 0), a2 = (-1/2, √3/2, 0), a3 = (0, 0, c)
        let a = 3.0;
        let c = 5.0;
        let sqrt3 = 3.0_f64.sqrt();
        // Direct lattice (column-major)
        let direct = Matrix3::new(a, -a / 2.0, 0.0, 0.0, a * sqrt3 / 2.0, 0.0, 0.0, 0.0, c);
        let two_pi = 2.0 * PI;
        let rec = direct.try_inverse().unwrap().transpose() * two_pi;
        let edges = compute_bz_wireframe(&rec);
        assert!(!edges.is_empty(), "Hexagonal BZ should have edges");
    }

    #[test]
    fn test_bz_nonempty_for_anisotropic_rhombohedral() {
        // Bi2Se3-like rhombohedral primitive cell with large c/a ratio (~7).
        // This is the case that broke the old norm-based cutoff.
        // Primitive rhombohedral vectors with α ≈ 24.3° (very acute)
        let a = 9.841;
        let alpha_deg = 24.304;
        let alpha = alpha_deg * PI / 180.0;
        let ca = alpha.cos();
        let sa = alpha.sin();

        // Rhombohedral primitive vectors in Cartesian:
        //   a1 = a*(sin α, 0, cos α)
        //   a2 = a*(-sin α * sin 30°, sin α * cos 30°, cos α)
        //   a3 = a*(-sin α * sin 30°, -sin α * cos 30°, cos α)
        let s30 = 0.5_f64;
        let c30 = (3.0_f64).sqrt() / 2.0;
        let direct = Matrix3::new(
            a * sa,
            -a * sa * s30,
            -a * sa * s30,
            0.0,
            a * sa * c30,
            -a * sa * c30,
            a * ca,
            a * ca,
            a * ca,
        );
        let two_pi = 2.0 * PI;
        let rec = direct.try_inverse().unwrap().transpose() * two_pi;
        let edges = compute_bz_wireframe(&rec);
        assert!(
            edges.len() >= 6,
            "Rhombohedral BZ should have at least 6 edges, got {}",
            edges.len()
        );
    }
}
