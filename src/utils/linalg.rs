// src/utils/linalg.rs

use nalgebra::{Matrix3, Vector3};

/// Convert fractional coordinates to Cartesian using lattice matrix
pub fn frac_to_cart(frac: [f64; 3], lattice: [[f64; 3]; 3]) -> [f64; 3] {
    let frac_vec = Vector3::from(frac);
    // Construct matrix from rows
    let lat_mat = Matrix3::new(
        lattice[0][0],
        lattice[0][1],
        lattice[0][2],
        lattice[1][0],
        lattice[1][1],
        lattice[1][2],
        lattice[2][0],
        lattice[2][1],
        lattice[2][2],
    );

    // Multiply: Cartesian = Lattice^T * Fractional
    let cart_vec = lat_mat.transpose() * frac_vec;

    [cart_vec.x, cart_vec.y, cart_vec.z]
}

/// Convert Cartesian coordinates to fractional using lattice matrix
/// Returns Option because matrix might be singular (e.g., collapsed cell)
pub fn cart_to_frac(cart: [f64; 3], lattice: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let cart_vec = Vector3::from(cart);
    let lat_mat = Matrix3::new(
        lattice[0][0],
        lattice[0][1],
        lattice[0][2],
        lattice[1][0],
        lattice[1][1],
        lattice[1][2],
        lattice[2][0],
        lattice[2][1],
        lattice[2][2],
    );

    // Inverse of Transpose
    if let Some(inv_lat) = lat_mat.transpose().try_inverse() {
        let frac_vec = inv_lat * cart_vec;
        Some([frac_vec.x, frac_vec.y, frac_vec.z])
    } else {
        None
    }
}

/// Inverts a 3x3 matrix (used for Supercell transformations)
pub fn invert_matrix_3x3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mat = Matrix3::new(
        m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
    );

    match mat.try_inverse() {
        Some(inv) => [
            [inv.m11, inv.m12, inv.m13],
            [inv.m21, inv.m22, inv.m23],
            [inv.m31, inv.m32, inv.m33],
        ],
        None => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], // Identity fallback
    }
}
