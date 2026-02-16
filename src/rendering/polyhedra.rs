// src/rendering/polyhedra.rs
// POLYHEDRA RENDERING - Reuses existing bond detection from painter.rs
// Implements coordination polyhedra for structural analysis

use crate::model::elements::get_atom_cov;
use crate::rendering::scene::RenderAtom;

/// A coordination polyhedron around a central atom
#[derive(Debug, Clone)]
pub struct Polyhedron {
    pub center_idx: usize,            // Index in RenderAtom list
    pub neighbor_indices: Vec<usize>, // Indices of coordinating atoms
    pub faces: Vec<Face>,
}

/// A polygonal face of the polyhedron
#[derive(Debug, Clone)]
pub struct Face {
    pub vertices: Vec<[f64; 3]>, // Screen positions of vertices (already rotated!)
    pub center: [f64; 3],        // Face center for depth sorting
}

/// Extract neighbors using EXISTING bond detection logic from painter.rs
/// This is the EXACT same algorithm - just returns indices instead of drawing
pub fn find_coordination_neighbors(
    center_idx: usize,
    atoms: &[RenderAtom],
    tolerance: f64,
    scale: f64,
) -> Vec<usize> {
    let mut neighbors = Vec::new();

    let r1 = &atoms[center_idx];
    let rad1 = get_atom_cov(&r1.element);

    // SAME LOGIC AS PAINTER.RS - Use Cartesian distance for bond detection
    for (j, r2) in atoms.iter().enumerate() {
        if center_idx == j {
            continue; // Skip self
        }

        // Calculate CARTESIAN distance (not screen distance!)
        let dx = r2.cart_pos[0] - r1.cart_pos[0];
        let dy = r2.cart_pos[1] - r1.cart_pos[1];
        let dz = r2.cart_pos[2] - r1.cart_pos[2];

        let dist = (dx * dx + dy * dy + dz * dz).sqrt();

        // Early cutoff (in Angstroms)
        if dist > 4.0 {
            continue;
        }

        let rad2 = get_atom_cov(&r2.element);

        // Bond criteria (EXACT same as painter.rs)
        let max_bond_dist = (rad1 + rad2) * tolerance;
        let min_bond_dist = 0.4;

        if dist > min_bond_dist && dist < max_bond_dist {
            neighbors.push(j);
        }
    }

    neighbors
}

/// Build polyhedra for selected atoms
pub fn build_polyhedra(
    atoms: &[RenderAtom],
    selected_elements: &[String], // Which elements to show polyhedra for
    tolerance: f64,
    scale: f64,
    min_cn: usize, // Minimum coordination number (e.g., 4)
    max_cn: usize, // Maximum coordination number (e.g., 12)
) -> Vec<Polyhedron> {
    let mut polyhedra = Vec::new();

    for (i, atom) in atoms.iter().enumerate() {
        // Only create polyhedra for selected elements
        if !selected_elements.contains(&atom.element) {
            continue;
        }

        // Find neighbors (reuses bond detection!)
        let neighbors = find_coordination_neighbors(i, atoms, tolerance, scale);
        let cn = neighbors.len();

        // Only create if coordination is in range
        if cn < min_cn || cn > max_cn {
            continue;
        }

        // Get neighbor positions
        let neighbor_positions: Vec<[f64; 3]> = neighbors
            .iter()
            .map(|&idx| atoms[idx].screen_pos) // Already rotated positions!
            .collect();

        // Calculate convex hull faces
        let faces = calculate_faces(atom.screen_pos, &neighbor_positions);

        polyhedra.push(Polyhedron {
            center_idx: i,
            neighbor_indices: neighbors,
            faces,
        });
    }

    polyhedra
}

/// Calculate polyhedron faces using convex hull
/// Simple algorithm: For each triple of neighbors, check if face is on convex hull
fn calculate_faces(center: [f64; 3], neighbors: &[[f64; 3]]) -> Vec<Face> {
    let n = neighbors.len();
    let mut faces = Vec::new();

    // Special cases for common coordination numbers
    match n {
        4 => {
            // Tetrahedral: 4 triangular faces
            faces.push(make_face(center, neighbors, &[0, 1, 2]));
            faces.push(make_face(center, neighbors, &[0, 1, 3]));
            faces.push(make_face(center, neighbors, &[0, 2, 3]));
            faces.push(make_face(center, neighbors, &[1, 2, 3]));
        }
        6 => {
            // Octahedral: 8 triangular faces
            // We'll use general algorithm but optimized for octahedra
            faces = general_convex_hull(center, neighbors);
        }
        _ => {
            // General case: full convex hull
            faces = general_convex_hull(center, neighbors);
        }
    }

    faces
}

/// General convex hull algorithm for arbitrary coordination
fn general_convex_hull(center: [f64; 3], neighbors: &[[f64; 3]]) -> Vec<Face> {
    let n = neighbors.len();
    let mut faces = Vec::new();

    // Try all possible triangular faces
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                // Check if this triangle is on the convex hull
                let v0 = neighbors[i];
                let v1 = neighbors[j];
                let v2 = neighbors[k];

                // Calculate face normal
                let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
                let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];

                let normal = cross_product(e1, e2);

                // Vector from center to first vertex
                let to_face = [v0[0] - center[0], v0[1] - center[1], v0[2] - center[2]];

                // If normal points away from center, this is an outer face
                let dot = dot_product(normal, to_face);

                if dot > 0.0 {
                    // Check if all other vertices are on the correct side
                    let is_convex_face = (0..n).all(|m| {
                        if m == i || m == j || m == k {
                            return true;
                        }

                        let vm = neighbors[m];
                        let to_vm = [vm[0] - v0[0], vm[1] - v0[1], vm[2] - v0[2]];
                        let side = dot_product(normal, to_vm);

                        side <= 0.001 // On or inside the face
                    });

                    if is_convex_face {
                        faces.push(make_face(center, neighbors, &[i, j, k]));
                    }
                }
            }
        }
    }

    // Remove duplicate faces (can happen with symmetric polyhedra)
    dedup_faces(&mut faces);

    faces
}

/// Helper: Create a face from vertex indices
fn make_face(_center: [f64; 3], neighbors: &[[f64; 3]], indices: &[usize]) -> Face {
    let vertices: Vec<[f64; 3]> = indices.iter().map(|&i| neighbors[i]).collect();

    // Calculate face center
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut cz = 0.0;
    for v in &vertices {
        cx += v[0];
        cy += v[1];
        cz += v[2];
    }
    let n = vertices.len() as f64;

    Face {
        vertices,
        center: [cx / n, cy / n, cz / n],
    }
}

/// Remove duplicate faces
fn dedup_faces(faces: &mut Vec<Face>) {
    let mut i = 0;
    while i < faces.len() {
        let mut j = i + 1;
        while j < faces.len() {
            if faces_are_same(&faces[i], &faces[j]) {
                faces.remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
    }
}

/// Check if two faces are the same (same vertices in any order)
fn faces_are_same(f1: &Face, f2: &Face) -> bool {
    if f1.vertices.len() != f2.vertices.len() {
        return false;
    }

    // Check if all vertices of f1 are in f2
    f1.vertices.iter().all(|v1| {
        f2.vertices.iter().any(|v2| {
            let dx = v1[0] - v2[0];
            let dy = v1[1] - v2[1];
            let dz = v1[2] - v2[2];
            dx * dx + dy * dy + dz * dz < 0.01
        })
    })
}

/// Cross product of two 3D vectors
fn cross_product(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Dot product of two 3D vectors
fn dot_product(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Sort faces by depth (back to front for proper transparency)
pub fn sort_faces_by_depth(faces: &mut [Face]) {
    faces.sort_by(|a, b| {
        // Sort by z-depth (back to front)
        b.center[2]
            .partial_cmp(&a.center[2])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tetrahedral_faces() {
        // Tetrahedral coordination should produce 4 faces
        let center = [0.0, 0.0, 0.0];
        let neighbors = vec![
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, -1.0, -1.0],
        ];

        let faces = calculate_faces(center, &neighbors);
        assert_eq!(faces.len(), 4);
    }
}
