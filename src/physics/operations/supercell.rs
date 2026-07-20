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
        crate::utils::console::log_warn(
            "Transformation matrix is singular (det ≈ 0) — returning original structure.",
        );
        return structure.clone();
    }

    // Convention: lattice ROWS are the vectors and A_new = M·A_old, so
    // a'_i = Σ_j m_ij a_j and a point with new-frac n sits at old-frac
    // s = Mᵀ·n. The new-frac of an old-frac point is therefore
    // n = (Mᵀ)⁻¹·s — NOT M⁻¹·s, which is only equal for symmetric
    // transforms (plain diagonal supercells). Cyclic axis permutations and
    // shears landed atoms at wrong positions with M⁻¹.
    let mt = [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ];
    let inv_mt = invert_matrix_3x3(mt);

    // Search range per axis: old-frac bounding box of the new cell's 8
    // corners (s = Mᵀ·n, n ∈ {0,1}³), padded by one. Exact for any integer
    // transform — the old max|m_ij| cube could leave holes for strongly
    // sheared matrices.
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for corner in 0..8u8 {
        let n = [
            (corner & 1) as f64,
            ((corner >> 1) & 1) as f64,
            ((corner >> 2) & 1) as f64,
        ];
        for (ax, mt_row) in mt.iter().enumerate() {
            let s = mt_row[0] * n[0] + mt_row[1] * n[1] + mt_row[2] * n[2];
            lo[ax] = lo[ax].min(s.floor() as i32 - 1);
            hi[ax] = hi[ax].max(s.ceil() as i32 + 1);
        }
    }

    let mut new_atoms = Vec::new();
    let mut atom_counter = 0;
    let eps = 1e-4;

    for i in lo[0]..=hi[0] {
        for j in lo[1]..=hi[1] {
            for k in lo[2]..=hi[2] {
                for atom in &structure.atoms {
                    let old_frac =
                        cart_to_frac(atom.position, structure.lattice).unwrap_or([0.0, 0.0, 0.0]);

                    // Shift by lattice image (i, j, k)
                    let shifted = [
                        old_frac[0] + i as f64,
                        old_frac[1] + j as f64,
                        old_frac[2] + k as f64,
                    ];

                    // Express in new fractional coordinates: n = (Mᵀ)⁻¹ s
                    let new_frac = [
                        inv_mt[0][0] * shifted[0]
                            + inv_mt[0][1] * shifted[1]
                            + inv_mt[0][2] * shifted[2],
                        inv_mt[1][0] * shifted[0]
                            + inv_mt[1][1] * shifted[1]
                            + inv_mt[1][2] * shifted[2],
                        inv_mt[2][0] * shifted[0]
                            + inv_mt[2][1] * shifted[1]
                            + inv_mt[2][2] * shifted[2],
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
        is_periodic: structure.is_periodic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    fn one_atom_cubic() -> Structure {
        Structure {
            lattice: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            atoms: vec![Atom {
                element: "Na".into(),
                position: [0.1, 0.2, 0.3],
                original_index: 0,
                oxidation: None,
                occupancy: 1.0,
            }],
            formula: "Na".into(),
            is_periodic: true,
        }
    }

    /// A cell redefinition must not move the crystal: every output atom's
    /// Cartesian position must coincide with an input atom's position
    /// modulo the OLD lattice.
    fn assert_crystal_preserved(old: &Structure, new: &Structure) {
        for atom in &new.atoms {
            let f = cart_to_frac(atom.position, old.lattice).unwrap();
            let matched = old.atoms.iter().any(|o| {
                let fo = cart_to_frac(o.position, old.lattice).unwrap();
                (0..3).all(|i| {
                    let d = (f[i] - fo[i]).rem_euclid(1.0);
                    d < 1e-6 || d > 1.0 - 1e-6
                })
            });
            assert!(
                matched,
                "atom at {:?} is not on the original crystal lattice",
                atom.position
            );
        }
    }

    /// Cyclic axis permutation (non-symmetric, det = 1): 1 atom out, at
    /// the same Cartesian position. The old M⁻¹ (instead of (Mᵀ)⁻¹)
    /// mapping placed it wrongly.
    #[test]
    fn cyclic_permutation_preserves_positions() {
        let s = one_atom_cubic();
        let out = transform(&s, [[0, 1, 0], [0, 0, 1], [1, 0, 0]]);
        assert_eq!(out.atoms.len(), 1);
        assert_crystal_preserved(&s, &out);
    }

    /// Strong shear (det = 2): atom count must be |det| × N and all atoms
    /// must stay on the crystal. The old max|m_ij| search cube could miss
    /// atoms for shears like this.
    #[test]
    fn sheared_supercell_complete() {
        let s = one_atom_cubic();
        let out = transform(&s, [[1, 0, 7], [0, 1, 0], [0, 0, 2]]);
        assert_eq!(out.atoms.len(), 2, "expected |det| × N = 2 atoms");
        assert_crystal_preserved(&s, &out);
    }

    /// Plain 2×2×2 supercell regression.
    #[test]
    fn diagonal_supercell() {
        let s = one_atom_cubic();
        let out = transform(&s, [[2, 0, 0], [0, 2, 0], [0, 0, 2]]);
        assert_eq!(out.atoms.len(), 8);
        assert_crystal_preserved(&s, &out);
    }
}
