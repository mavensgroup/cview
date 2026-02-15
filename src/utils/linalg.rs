// src/utils/linalg.rs

use nalgebra::{Matrix3, Vector3};

/// Convert fractional coordinates to Cartesian using lattice matrix
///
/// # Arguments
/// * `frac` - Fractional coordinates [x, y, z] in range [0, 1]
/// * `lattice` - Lattice vectors as row matrix [[ax, ay, az], [bx, by, bz], [cx, cy, cz]]
///
/// # Returns
/// Cartesian coordinates in Angstroms
///
/// # Formula
/// ```text
/// Cartesian = Lattice^T × Fractional
/// ```
pub fn frac_to_cart(frac: [f64; 3], lattice: [[f64; 3]; 3]) -> [f64; 3] {
  let frac_vec = Vector3::from(frac);
  let lat_mat = Matrix3::from_row_slice(&[
    lattice[0][0],
    lattice[0][1],
    lattice[0][2],
    lattice[1][0],
    lattice[1][1],
    lattice[1][2],
    lattice[2][0],
    lattice[2][1],
    lattice[2][2],
  ]);

  // Multiply: Cartesian = Lattice^T × Fractional
  let cart_vec = lat_mat.transpose() * frac_vec;

  [cart_vec.x, cart_vec.y, cart_vec.z]
}

/// Convert Cartesian coordinates to fractional using lattice matrix
///
/// # Arguments
/// * `cart` - Cartesian coordinates in Angstroms
/// * `lattice` - Lattice vectors as row matrix [[ax, ay, az], [bx, by, bz], [cx, cy, cz]]
///
/// # Returns
/// Fractional coordinates [x, y, z] or None if lattice is singular
///
/// # Formula
/// ```text
/// Fractional = (Lattice^T)^-1 × Cartesian
/// ```
pub fn cart_to_frac(cart: [f64; 3], lattice: [[f64; 3]; 3]) -> Option<[f64; 3]> {
  let cart_vec = Vector3::from(cart);
  let lat_mat = Matrix3::from_row_slice(&[
    lattice[0][0],
    lattice[0][1],
    lattice[0][2],
    lattice[1][0],
    lattice[1][1],
    lattice[1][2],
    lattice[2][0],
    lattice[2][1],
    lattice[2][2],
  ]);

  // Invert lattice matrix transpose
  let inv_lat = lat_mat.transpose().try_inverse()?;

  // Multiply: Fractional = (Lattice^T)^-1 × Cartesian
  let frac_vec = inv_lat * cart_vec;

  Some([frac_vec.x, frac_vec.y, frac_vec.z])
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_cubic_lattice() {
    // Simple cubic lattice 5.0 Å
    let lattice = [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]];

    let frac = [0.5, 0.5, 0.5];
    let cart = frac_to_cart(frac, lattice);

    assert!((cart[0] - 2.5).abs() < 1e-10);
    assert!((cart[1] - 2.5).abs() < 1e-10);
    assert!((cart[2] - 2.5).abs() < 1e-10);
  }

  #[test]
  fn test_roundtrip() {
    // Non-orthogonal lattice
    let lattice = [[4.0, 0.0, 0.0], [2.0, 3.46, 0.0], [0.0, 0.0, 5.0]];

    let frac_orig = [0.333, 0.667, 0.25];
    let cart = frac_to_cart(frac_orig, lattice);
    let frac_back = cart_to_frac(cart, lattice).unwrap();

    assert!((frac_back[0] - frac_orig[0]).abs() < 1e-10);
    assert!((frac_back[1] - frac_orig[1]).abs() < 1e-10);
    assert!((frac_back[2] - frac_orig[2]).abs() < 1e-10);
  }

  #[test]
  fn test_origin() {
    let lattice = [[3.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 5.0]];

    let frac = [0.0, 0.0, 0.0];
    let cart = frac_to_cart(frac, lattice);

    assert!((cart[0]).abs() < 1e-10);
    assert!((cart[1]).abs() < 1e-10);
    assert!((cart[2]).abs() < 1e-10);
  }
}
