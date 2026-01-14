// src/physics/analysis/kpath.rs

use crate::model::bs_data::{self, BrillouinZoneData};
use crate::model::elements::get_atomic_number;
use crate::model::structure::Structure;
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;
use nalgebra::{Matrix3, Vector3};

const SYMPREC: f64 = 1e-3; // Slightly looser tolerance for real-world files

#[derive(Debug, Clone)]
pub struct KPoint {
  pub label: String,
  pub coords_frac: [f64; 3], // Fractional in Primitive Reciprocal Basis
  pub coords_cart: [f64; 3], // Cartesian for visualization
}

#[derive(Debug, Clone)]
pub struct KPathResult {
  pub spacegroup_str: String,
  pub number: i32,
  pub lattice_type: String,
  pub kpoints: Vec<KPoint>,
  pub path_segments: Vec<Vec<KPoint>>,
  pub bz_lines: Vec<([f64; 3], [f64; 3])>,
  pub rec_lattice: Matrix3<f64>, // The primitive reciprocal lattice
}

pub fn calculate_kpath(structure: &Structure) -> Option<KPathResult> {
  // 1. Convert Input Structure to Moyo Cell
  let input_lattice = Matrix3::from_columns(&[
    Vector3::from(structure.lattice[0]),
    Vector3::from(structure.lattice[1]),
    Vector3::from(structure.lattice[2]),
  ]);

  let mut positions = Vec::new();
  let mut numbers = Vec::new();

  // We need fractional coords relative to the input lattice for Moyo
  // But structure.atoms usually stores cartesian.
  // Let's assume structure.atoms is Cartesian as per your main.rs logic
  let inv_basis = input_lattice.try_inverse()?;
  for atom in &structure.atoms {
    let cart = Vector3::from(atom.position);
    let frac = inv_basis * cart;
    positions.push(frac);
    numbers.push(get_atomic_number(&atom.element) as i32);
  }

  let moyo_cell = Cell::new(
    Lattice::new(input_lattice.transpose()), // Moyo wants Row-Major lattice
    positions,
    numbers,
  );

  // 2. Get Symmetry Dataset (Standardization happens here)
  let dataset = MoyoDataset::new(
    &moyo_cell,
    SYMPREC,
    AngleTolerance::Default,
    Setting::Spglib, // Use Spglib conventions (compatible with Setyawan-Curtarolo)
    true,            // Standardize
  )
  .ok()?;

  let sg_num = dataset.number;
  println!("[KPATH] Detected Space Group: #{}", sg_num);

  // 3. Extract the STANDARDIZED PRIMITIVE Lattice
  // Moyo returns std_cell.lattice in Row-Major.
  // We Transpose to get Column vectors (a, b, c)
  let std_prim_lattice = dataset.std_cell.lattice.basis.transpose();

  // 4. Calculate Reciprocal Lattice of the PRIMITIVE Cell
  // b_i = 2*PI * (a_i)^-1
  // Matrix form: B = 2*PI * (A^-1)^T
  let two_pi = 2.0 * std::f64::consts::PI;
  let rec_lattice = match std_prim_lattice.try_inverse() {
    Some(inv) => inv.transpose() * two_pi,
    None => return None,
  };

  // 5. Lookup K-points and Path from Table
  let data = bs_data::get_sc_data(sg_num)?;

  // 6. Construct Result
  let mut linear_kpoints = Vec::new(); // Flattened list for VASP
  let mut path_segments = Vec::new();

  // Helper to make a KPoint struct
  let make_kp = |label: &str| -> Option<KPoint> {
    if let Some(frac) = data.special_points.get(label) {
      let f_vec = Vector3::new(frac[0], frac[1], frac[2]);
      let c_vec = rec_lattice * f_vec; // Convert Frac -> Cart
      Some(KPoint {
        label: label.to_string(),
        coords_frac: *frac,
        coords_cart: [c_vec.x, c_vec.y, c_vec.z],
      })
    } else {
      None
    }
  };

  for segment_labels in &data.path {
    let mut segment_pts = Vec::new();
    for label in segment_labels {
      if let Some(kp) = make_kp(label) {
        segment_pts.push(kp.clone());
        linear_kpoints.push(kp);
      }
    }
    path_segments.push(segment_pts);
  }

  // 7. Transform Wireframe from Frac -> Cart
  // The lookup table stores wireframe in fractional Reciprocal coords (usually).
  // Or if Setyawan defines them in Cartesian relative to reciprocal basis vectors?
  // Actually, Setyawan's paper defines vertices in terms of b1, b2, b3.
  // So we just multiply by rec_lattice.
  let mut cart_lines = Vec::new();
  for (start_f, end_f) in &data.wireframe {
    let s = rec_lattice * Vector3::from(*start_f);
    let e = rec_lattice * Vector3::from(*end_f);
    cart_lines.push(([s.x, s.y, s.z], [e.x, e.y, e.z]));
  }

  Some(KPathResult {
    spacegroup_str: format!("{} ({})", sg_num, dataset.hall_number),
    number: sg_num,
    lattice_type: data.lattice_type,
    kpoints: linear_kpoints,
    path_segments,
    bz_lines: cart_lines,
    rec_lattice,
  })
}
