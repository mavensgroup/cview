// src/physics/operations/supercell.rs

use crate::model::structure::Structure;
use crate::utils::linalg::{cart_to_frac, frac_to_cart, invert_matrix_3x3, mat3_det, mat3_mul};

/// Transform a structure by an integer 3×3 matrix.
/// Handles supercells (diagonal), axis swaps, and arbitrary cell redefinitions.
pub fn transform(structure: &Structure, matrix: [[i32; 3]; 3]) -> Structure {
    let m: [[f64; 3]; 3] = [
        [
            matrix[0][0] as f64,
            matrix[0][1] as f64,
            matrix[0][2] as f64,
        ],
        [
            matrix[1][0] as f64,
            matrix[1][1] as f64,
            matrix[1][2] as f64,
        ],
        [
            matrix[2][0] as f64,
            matrix[2][1] as f64,
            matrix[2][2] as f64,
        ],
    ];

    let det = mat3_det(m);
    if det.abs() < 1e-6 {
        eprintln!("Transformation matrix is singular (det ≈ 0) — returning original structure.");
        return structure.clone();
    }

    let inv_m = invert_matrix_3x3(m);

    // Search range: how many old cells can fit inside the new one
    let max_coeff = m
        .iter()
        .flat_map(|r| r.iter())
        .fold(0.0f64, |a, &b| a.max(b.abs()));
    let range = (max_coeff.ceil() as i32 + 1).max(2);

    let mut new_atoms = Vec::new();
    let mut atom_counter = 0;
    let eps = 1e-4;

    for i in -range..=range {
        for j in -range..=range {
            for k in -range..=range {
                for atom in &structure.atoms {
                    let old_frac =
                        cart_to_frac(atom.position, structure.lattice).unwrap_or([0.0, 0.0, 0.0]);

                    // Shift by lattice image (i, j, k)
                    let shifted = [
                        old_frac[0] + i as f64,
                        old_frac[1] + j as f64,
                        old_frac[2] + k as f64,
                    ];

                    // Express in new fractional coordinates
                    let new_frac = [
                        inv_m[0][0] * shifted[0]
                            + inv_m[0][1] * shifted[1]
                            + inv_m[0][2] * shifted[2],
                        inv_m[1][0] * shifted[0]
                            + inv_m[1][1] * shifted[1]
                            + inv_m[1][2] * shifted[2],
                        inv_m[2][0] * shifted[0]
                            + inv_m[2][1] * shifted[1]
                            + inv_m[2][2] * shifted[2],
                    ];

                    // Keep only atoms inside the new cell [0, 1)
                    if new_frac[0] >= -eps
                        && new_frac[0] < 1.0 - eps
                        && new_frac[1] >= -eps
                        && new_frac[1] < 1.0 - eps
                        && new_frac[2] >= -eps
                        && new_frac[2] < 1.0 - eps
                    {
                        let mut new_atom = atom.clone();
                        new_atom.position = [
                            new_frac[0].rem_euclid(1.0),
                            new_frac[1].rem_euclid(1.0),
                            new_frac[2].rem_euclid(1.0),
                        ];
                        new_atom.original_index = atom_counter;
                        new_atoms.push(new_atom);
                        atom_counter += 1;
                    }
                }
            }
        }
    }

    // New lattice = M * old_lattice  (row vectors)
    let new_lattice = mat3_mul(m, structure.lattice);

    // Convert stored fractional positions back to Cartesian with new lattice
    for atom in &mut new_atoms {
        atom.position = frac_to_cart(atom.position, new_lattice);
    }

    Structure {
        atoms: new_atoms,
        lattice: new_lattice,
        formula: structure.formula.clone(),
    }
}
