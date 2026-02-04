// src/physics/operations/slab.rs

use crate::model::structure::{Atom, Structure};
use crate::physics::operations::miller_algo::MillerMath; // <--- CHANGED: Import the Struct
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
    // Convention: Lattice vectors are COLUMNS of the matrix
    let lat_matrix = Matrix3::new(
        structure.lattice[0][0],
        structure.lattice[1][0],
        structure.lattice[2][0],
        structure.lattice[0][1],
        structure.lattice[1][1],
        structure.lattice[2][1],
        structure.lattice[0][2],
        structure.lattice[1][2],
        structure.lattice[2][2],
    );

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
    // New lattice vectors: A_new = A_old * M
    let lat_primitive = lat_matrix * m_transform;

    // ========== 4. MAP ATOMS TO PRIMITIVE CELL ==========
    let lat_inv = lat_matrix
        .try_inverse()
        .ok_or("Cannot invert original lattice matrix")?;

    // Search range based on volume expansion
    let search_range = (det_transform.powf(1.0 / 3.0).ceil() as i32) + 2;
    let mut primitive_atoms: Vec<(String, Vector3<f64>)> = Vec::new();

    for i in -search_range..=search_range {
        for j in -search_range..=search_range {
            for k_idx in -search_range..=search_range {
                let shift = Vector3::new(i as f64, j as f64, k_idx as f64);

                for atom in &structure.atoms {
                    let cart_pos =
                        Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                    let frac_orig = lat_inv * cart_pos;

                    // Apply supercell shift
                    let frac_shifted = frac_orig + shift;

                    // Transform to new fractional coordinates: r_new = M_inv * r_old
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

    // Add vacuum along the surface normal
    let c_new = c_vec + normal_unit * vacuum;
    lat_slab.set_column(2, &c_new);

    // ========== 8. RESCALE FRACTIONAL COORDINATES ==========
    let c_proj_old = c_vec.dot(&normal_unit);
    let c_proj_new = c_new.dot(&normal_unit);

    if c_proj_new.abs() < TOLERANCE {
        return Err("New c-vector has zero projection along normal".to_string());
    }

    let z_scale = c_proj_old / c_proj_new;

    // ========== 9. CONVERT TO CARTESIAN COORDINATES ==========
    let mut final_atoms: Vec<Atom> = Vec::new();

    for (idx, (element, mut frac_pos)) in slab_atoms.into_iter().enumerate() {
        frac_pos.z *= z_scale;
        frac_pos.x = wrap_coordinate(frac_pos.x);
        frac_pos.y = wrap_coordinate(frac_pos.y);
        frac_pos.z = wrap_coordinate(frac_pos.z);

        let cart_pos = lat_slab * frac_pos;

        final_atoms.push(Atom {
            element,
            position: [cart_pos.x, cart_pos.y, cart_pos.z],
            original_index: idx,
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
        let pos = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
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
