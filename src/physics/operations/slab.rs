// src/physics/operations/slab.rs

use crate::model::structure::{Atom, Structure};
use crate::physics::operations::miller_algo::MillerMath;
use crate::utils::linalg::cart_to_frac;
use nalgebra::{Matrix3, Vector3};

const TOLERANCE: f64 = 1e-5;

pub fn generate_slab(
    structure: &Structure,
    h: i32,
    k: i32,
    l: i32,
    thickness: u32,
    vacuum: f64,
) -> Result<Structure, String> {
    // ========== INPUT VALIDATION ==========
    if thickness == 0 {
        return Err("Thickness must be greater than 0".to_string());
    }
    if vacuum < 0.0 {
        return Err("Vacuum spacing cannot be negative".to_string());
    }
    if h == 0 && k == 0 && l == 0 {
        return Err("Miller indices (0,0,0) are invalid".to_string());
    }
    if structure.atoms.is_empty() {
        return Err("Input structure has no atoms".to_string());
    }

    // ========== 1. CONSTRUCT LATTICE MATRIX ==========
    // Lattice vectors as columns
    let lat_matrix = Matrix3::from_columns(&[
        Vector3::from(structure.lattice[0]),
        Vector3::from(structure.lattice[1]),
        Vector3::from(structure.lattice[2]),
    ]);

    let det_orig = lat_matrix.determinant();
    if det_orig.abs() < TOLERANCE {
        return Err("Original lattice is singular (zero volume)".to_string());
    }

    // ========== 2. FIND PLANE BASIS VECTORS ==========
    // CHANGED: Use the unified MillerMath "Brain"
    let math = MillerMath::new(h, k, l);
    let (u_vec, v_vec, w_vec) = math.find_basis()?;

    // Transformation matrix: columns are the new basis in terms of old basis indices
    let m_transform = Matrix3::new(
        u_vec.x as f64,
        v_vec.x as f64,
        w_vec.x as f64,
        u_vec.y as f64,
        v_vec.y as f64,
        w_vec.y as f64,
        u_vec.z as f64,
        v_vec.z as f64,
        w_vec.z as f64,
    );

    let det_transform = m_transform.determinant().abs();
    if det_transform < TOLERANCE {
        return Err(format!(
            "Singular transformation for Miller indices ({},{},{})",
            h, k, l
        ));
    }

    let m_inv = m_transform
        .try_inverse()
        .ok_or("Failed to invert transformation matrix")?;

    // ========== 3. NEW PRIMITIVE LATTICE ==========
    let lat_primitive = lat_matrix * m_transform;

    // ========== 4. MAP ATOMS TO PRIMITIVE CELL ==========
    // Search range per axis: map the 8 corners of the new cell (fractional
    // (0|1, 0|1, 0|1)) through M into old fractional coordinates
    // (f_old = M f_new, column convention) and take the integer bounding
    // box ± 1. Exact for any integer basis — the previous det^(1/3)-based
    // cube could leave holes for anisotropic (hkl) bases.
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for corner in 0..8u8 {
        let n = Vector3::new(
            (corner & 1) as f64,
            ((corner >> 1) & 1) as f64,
            ((corner >> 2) & 1) as f64,
        );
        let s = m_transform * n;
        for ax in 0..3 {
            lo[ax] = lo[ax].min(s[ax].floor() as i32 - 1);
            hi[ax] = hi[ax].max(s[ax].ceil() as i32 + 1);
        }
    }
    let mut primitive_atoms: Vec<(String, Vector3<f64>)> = Vec::new();

    for i in lo[0]..=hi[0] {
        for j in lo[1]..=hi[1] {
            for k_idx in lo[2]..=hi[2] {
                let shift = Vector3::new(i as f64, j as f64, k_idx as f64);

                for atom in &structure.atoms {
                    let frac_orig = match cart_to_frac(atom.position, structure.lattice) {
                        Some(f) => Vector3::from(f),
                        None => continue,
                    };
                    let frac_shifted = frac_orig + shift;
                    let frac_new = m_inv * frac_shifted;

                    if is_in_unit_cell(frac_new) {
                        primitive_atoms.push((atom.element.clone(), frac_new));
                    }
                }
            }
        }
    }

    if primitive_atoms.is_empty() {
        return Err(format!(
            "No atoms mapped to primitive cell for ({},{},{})",
            h, k, l
        ));
    }

    // ========== 5. REMOVE DUPLICATE ATOMS ==========
    primitive_atoms = remove_duplicates(primitive_atoms);

    // ========== 6. REPLICATE LAYERS ==========
    let mut slab_atoms: Vec<(String, Vector3<f64>)> = Vec::new();

    for layer in 0..thickness {
        for (element, frac_pos) in &primitive_atoms {
            // Stack layers along the new Z axis
            let new_frac = Vector3::new(
                frac_pos.x,
                frac_pos.y,
                (frac_pos.z + layer as f64) / thickness as f64,
            );
            slab_atoms.push((element.clone(), new_frac));
        }
    }

    // ========== 7. BUILD SLAB LATTICE WITH VACUUM ==========
    let mut lat_slab = lat_primitive;

    // Scale c-vector by thickness
    let c_vector = lat_primitive.column(2) * thickness as f64;
    lat_slab.set_column(2, &c_vector);

    let a_vec = lat_slab.column(0).into_owned();
    let b_vec = lat_slab.column(1).into_owned();
    let c_vec = lat_slab.column(2).into_owned();

    // Surface normal (perpendicular to slab plane a, b)
    let normal = a_vec.cross(&b_vec);
    let normal_len = normal.norm();
    if normal_len < TOLERANCE {
        return Err("Lattice vectors a and b are parallel".to_string());
    }
    let normal_unit = normal / normal_len;

    // Add vacuum along the surface normal, on the side c already points to
    // (a left-handed stacking vector would otherwise shrink the gap).
    let c_proj_old = c_vec.dot(&normal_unit);
    if c_proj_old.abs() < TOLERANCE {
        return Err("c-vector has zero projection along the surface normal".to_string());
    }
    let c_new = c_vec + normal_unit * vacuum * c_proj_old.signum();
    lat_slab.set_column(2, &c_new);

    // ========== 8-9. CARTESIAN POSITIONS FROM THE PRE-VACUUM LATTICE ==========
    // Wrap fractional coordinates in the PRE-vacuum cell, convert to
    // Cartesian there, and keep those Cartesian positions unchanged when
    // the vacuum-extended c is swapped in (the ASE/pymatgen construction).
    // Rescaling fractional z against the new cell instead — the previous
    // approach — shears any slab whose stacking vector is oblique to the
    // surface: the in-plane part of each position becomes c_∥·z·s instead
    // of c_∥·z, changing interlayer bond lengths. Only c ⊥ (a,b) slabs
    // were unaffected.
    let lat_pre = {
        let mut m = lat_slab;
        m.set_column(2, &c_vec);
        m
    };
    let mut final_atoms: Vec<Atom> = Vec::new();

    for (idx, (element, mut frac_pos)) in slab_atoms.into_iter().enumerate() {
        frac_pos.x = wrap_coordinate(frac_pos.x);
        frac_pos.y = wrap_coordinate(frac_pos.y);
        frac_pos.z = wrap_coordinate(frac_pos.z);

        let cart_pos = lat_pre * frac_pos;

        final_atoms.push(Atom {
            element,
            position: [cart_pos.x, cart_pos.y, cart_pos.z],
            original_index: idx,
            // Slab construction is by element + lattice transform; per-atom
            // oxidation hints from the source aren't tracked through the
            // tuple pipeline. BVS will fall back to inference.
            oxidation: None,
            occupancy: 1.0,
        });
    }

    final_atoms = remove_duplicate_atoms(final_atoms);

    // ========== 10. BUILD OUTPUT STRUCTURE ==========
    let new_lattice = [
        [lat_slab[(0, 0)], lat_slab[(1, 0)], lat_slab[(2, 0)]],
        [lat_slab[(0, 1)], lat_slab[(1, 1)], lat_slab[(2, 1)]],
        [lat_slab[(0, 2)], lat_slab[(1, 2)], lat_slab[(2, 2)]],
    ];

    Ok(Structure {
        lattice: new_lattice,
        atoms: final_atoms,
        formula: format!("{}x({}{}{}) Slab", thickness, h, k, l),
        is_periodic: true,
    })
}

// ========== HELPER FUNCTIONS ==========

fn is_in_unit_cell(frac: Vector3<f64>) -> bool {
    frac.x >= -TOLERANCE
        && frac.x < 1.0 - TOLERANCE
        && frac.y >= -TOLERANCE
        && frac.y < 1.0 - TOLERANCE
        && frac.z >= -TOLERANCE
        && frac.z < 1.0 - TOLERANCE
}

fn wrap_coordinate(mut x: f64) -> f64 {
    while x < -TOLERANCE {
        x += 1.0;
    }
    while x >= 1.0 - TOLERANCE {
        x -= 1.0;
    }
    if x.abs() < TOLERANCE {
        x = 0.0;
    }
    x
}

fn remove_duplicates(atoms: Vec<(String, Vector3<f64>)>) -> Vec<(String, Vector3<f64>)> {
    let mut unique_atoms: Vec<(String, Vector3<f64>)> = Vec::new();
    for (element, pos) in atoms {
        let wrapped_pos = Vector3::new(
            wrap_coordinate(pos.x),
            wrap_coordinate(pos.y),
            wrap_coordinate(pos.z),
        );
        let mut is_duplicate = false;
        for (_, seen_pos) in &unique_atoms {
            if are_positions_equal(wrapped_pos, *seen_pos) {
                is_duplicate = true;
                break;
            }
        }
        if !is_duplicate {
            unique_atoms.push((element, wrapped_pos));
        }
    }
    unique_atoms
}

fn remove_duplicate_atoms(atoms: Vec<Atom>) -> Vec<Atom> {
    let mut unique_atoms = Vec::new();
    let mut seen_positions = Vec::new();
    for atom in atoms {
        let pos = Vector3::from(atom.position);
        let mut is_duplicate = false;
        for seen_pos in &seen_positions {
            if are_positions_equal(pos, *seen_pos) {
                is_duplicate = true;
                break;
            }
        }
        if !is_duplicate {
            seen_positions.push(pos);
            unique_atoms.push(atom);
        }
    }
    unique_atoms
}

fn are_positions_equal(p1: Vector3<f64>, p2: Vector3<f64>) -> bool {
    (p1 - p2).norm() < 1e-4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::linalg::frac_to_cart;

    /// Rutile TiO2 (P4_2/mnm), a = 4.5937, c = 2.9587, O u = 0.3053.
    fn rutile() -> Structure {
        let (a, c) = (4.5937, 2.9587);
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, c]];
        let u = 0.3053;
        let frac: [(&str, [f64; 3]); 6] = [
            ("Ti", [0.0, 0.0, 0.0]),
            ("Ti", [0.5, 0.5, 0.5]),
            ("O", [u, u, 0.0]),
            ("O", [1.0 - u, 1.0 - u, 0.0]),
            ("O", [0.5 + u, 0.5 - u, 0.5]),
            ("O", [0.5 - u, 0.5 + u, 0.5]),
        ];
        Structure {
            lattice: lat,
            atoms: frac
                .iter()
                .enumerate()
                .map(|(i, (e, f))| Atom {
                    element: e.to_string(),
                    position: frac_to_cart(*f, lat),
                    original_index: i,
                    oxidation: None,
                    occupancy: 1.0,
                })
                .collect(),
            formula: "TiO2".into(),
            is_periodic: true,
        }
    }

    fn sorted_positions(s: &Structure) -> Vec<(String, [i64; 3])> {
        let mut v: Vec<(String, [i64; 3])> = s
            .atoms
            .iter()
            .map(|a| {
                (
                    a.element.clone(),
                    [
                        (a.position[0] * 1e6).round() as i64,
                        (a.position[1] * 1e6).round() as i64,
                        (a.position[2] * 1e6).round() as i64,
                    ],
                )
            })
            .collect();
        v.sort();
        v
    }

    /// S-1 acceptance: vacuum insertion must not move a single atom. A
    /// (101) rutile slab has an oblique stacking vector; the old
    /// fractional-z rescaling sheared it, changing interlayer bond
    /// lengths. Atom Cartesian positions with vacuum = 0 and vacuum = 15 Å
    /// must be identical.
    #[test]
    fn vacuum_does_not_shear_oblique_slab() {
        let bulk = rutile();
        let no_vac = generate_slab(&bulk, 1, 0, 1, 2, 0.0).expect("slab failed");
        let with_vac = generate_slab(&bulk, 1, 0, 1, 2, 15.0).expect("slab failed");

        assert_eq!(no_vac.atoms.len(), with_vac.atoms.len());
        assert_eq!(
            sorted_positions(&no_vac),
            sorted_positions(&with_vac),
            "vacuum insertion moved atoms — slab is sheared"
        );
    }

    /// S-2: non-coprime indices must reduce, not fail. (200) ≡ (100).
    #[test]
    fn non_coprime_indices_reduce() {
        let bulk = rutile();
        let s200 = generate_slab(&bulk, 2, 0, 0, 1, 10.0).expect("(200) should not fail");
        let s100 = generate_slab(&bulk, 1, 0, 0, 1, 10.0).expect("(100) failed");
        assert_eq!(sorted_positions(&s200), sorted_positions(&s100));
    }

    /// Slab must conserve stoichiometry: N_slab = N_bulk × |det| × thickness.
    #[test]
    fn slab_atom_count_conserved() {
        let bulk = rutile();
        for (h, k, l) in [(1, 0, 1), (1, 1, 0), (1, 1, 1)] {
            let slab = generate_slab(&bulk, h, k, l, 2, 10.0).expect("slab failed");
            let math = MillerMath::new(h, k, l);
            let (u, v, w) = math.find_basis().unwrap();
            let det = {
                let m = Matrix3::new(
                    u.x as f64, v.x as f64, w.x as f64, u.y as f64, v.y as f64, w.y as f64,
                    u.z as f64, v.z as f64, w.z as f64,
                );
                m.determinant().abs().round() as usize
            };
            assert_eq!(
                slab.atoms.len(),
                bulk.atoms.len() * det * 2,
                "({h}{k}{l}) slab atom count wrong"
            );
        }
    }
}
