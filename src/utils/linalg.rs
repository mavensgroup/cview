// src/utils/linalg.rs

use nalgebra::{Matrix3, Vector3};

/// Convert fractional coordinates to Cartesian using lattice matrix
pub fn frac_to_cart(frac: [f64; 3], lattice: [[f64; 3]; 3]) -> [f64; 3] {
    let frac_vec = Vector3::from(frac);
    let lat_mat = lattice_to_matrix3(lattice);
    let cart_vec = lat_mat.transpose() * frac_vec;
    [cart_vec.x, cart_vec.y, cart_vec.z]
}

/// Convert Cartesian coordinates to fractional using lattice matrix.
/// Returns None if the lattice matrix is singular.
pub fn cart_to_frac(cart: [f64; 3], lattice: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let cart_vec = Vector3::from(cart);
    let lat_mat = lattice_to_matrix3(lattice);
    lat_mat.transpose().try_inverse().map(|inv| {
        let frac_vec = inv * cart_vec;
        [frac_vec.x, frac_vec.y, frac_vec.z]
    })
}

/// Invert a 3×3 matrix. Returns identity on singular input.
pub fn invert_matrix_3x3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mat = arr_to_matrix3(m);
    match mat.try_inverse() {
        Some(inv) => matrix3_to_arr(inv),
        None => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// Determinant of a 3×3 matrix.
pub fn mat3_det(m: [[f64; 3]; 3]) -> f64 {
    arr_to_matrix3(m).determinant()
}

/// Multiply two 3×3 matrices: result = a * b
pub fn mat3_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    matrix3_to_arr(arr_to_matrix3(a) * arr_to_matrix3(b))
}

/// Apply a 3×3 matrix to a vector: result = m * v
pub fn mat3_mul_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    let result = arr_to_matrix3(m) * Vector3::from(v);
    [result.x, result.y, result.z]
}

// ── Private helpers ──────────────────────────────────────────────────────────

fn lattice_to_matrix3(lattice: [[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(
        lattice[0][0],
        lattice[0][1],
        lattice[0][2],
        lattice[1][0],
        lattice[1][1],
        lattice[1][2],
        lattice[2][0],
        lattice[2][1],
        lattice[2][2],
    )
}

fn arr_to_matrix3(m: [[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(
        m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
    )
}

fn matrix3_to_arr(m: Matrix3<f64>) -> [[f64; 3]; 3] {
    [
        [m.m11, m.m12, m.m13],
        [m.m21, m.m22, m.m23],
        [m.m31, m.m32, m.m33],
    ]
}
