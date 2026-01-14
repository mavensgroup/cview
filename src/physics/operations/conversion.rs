use crate::model::structure::{Atom, Structure};
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;
use nalgebra::{Matrix3, Vector3};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
pub enum CellType {
  Primitive,
  Conventional,
}

pub fn convert_structure(structure: &Structure, cell_type: CellType) -> Result<Structure, String> {
  // 1. Map Elements to Integers (Moyo works with atomic numbers/integers)
  let mut distinct_elements: Vec<String> =
    structure.atoms.iter().map(|a| a.element.clone()).collect();
  distinct_elements.sort();
  distinct_elements.dedup();

  let element_map: HashMap<String, i32> = distinct_elements
    .iter()
    .enumerate()
    .map(|(i, el)| (el.clone(), (i + 1) as i32))
    .collect();

  let reverse_map: HashMap<i32, String> = distinct_elements
    .iter()
    .enumerate()
    .map(|(i, el)| ((i + 1) as i32, el.clone()))
    .collect();

  // 2. Prepare Data for Moyo
  // Moyo requires:
  // - Lattice in Row-Major format (Vectors are rows)
  // - Positions in Fractional coordinates

  // Create Column-Basis Matrix (standard physics: columns are lattice vectors)
  // We assume structure.lattice is [[x,y,z], [x,y,z], [x,y,z]] (Row vectors)
  let lat = structure.lattice;
  let basis_cols = Matrix3::from_columns(&[
    Vector3::new(lat[0][0], lat[0][1], lat[0][2]),
    Vector3::new(lat[1][0], lat[1][1], lat[1][2]),
    Vector3::new(lat[2][0], lat[2][1], lat[2][2]),
  ]);

  // Calculate Inverse to convert Cartesian -> Fractional
  let inv_basis = basis_cols
    .try_inverse()
    .ok_or("Lattice is singular (volume is zero)")?;

  let mut positions = Vec::new();
  let mut numbers = Vec::new();

  for atom in &structure.atoms {
    let cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
    let frac = inv_basis * cart; // Matrix * Vector
    positions.push(frac);
    numbers.push(*element_map.get(&atom.element).unwrap());
  }

  // Create Moyo Cell
  // Note: Moyo Lattice::new takes vectors as ROWS.
  // Since basis_cols has vectors as columns, transpose makes them rows.
  let moyo_cell = Cell::new(Lattice::new(basis_cols.transpose()), positions, numbers);

  // 3. Run Moyo Symmetry Search
  let dataset = MoyoDataset::new(
    &moyo_cell,
    1e-4, // Tolerance
    AngleTolerance::Default,
    Setting::Spglib, // Use Spglib setting for standard conventional cells
    true,            // Enable standardization
  )
  .map_err(|e| format!("Symmetry search failed: {:?}", e))?;

  // 4. Select the Output Cell
  // FIXED: Correct field name is `prim_std_cell`
  let result_cell = match cell_type {
    CellType::Primitive => &dataset.prim_std_cell,
    CellType::Conventional => &dataset.std_cell,
  };

  // 5. Convert Moyo Cell Back to Structure (Fractional -> Cartesian)

  // Moyo returns lattice as Rows. Transpose to get Columns (a, b, c)
  let new_basis_cols = result_cell.lattice.basis.transpose();

  // Convert back to Array form for Structure struct
  let new_lattice = [
    [
      new_basis_cols[(0, 0)],
      new_basis_cols[(1, 0)],
      new_basis_cols[(2, 0)],
    ], // Row 0 (Vector a)
    [
      new_basis_cols[(0, 1)],
      new_basis_cols[(1, 1)],
      new_basis_cols[(2, 1)],
    ], // Row 1 (Vector b)
    [
      new_basis_cols[(0, 2)],
      new_basis_cols[(1, 2)],
      new_basis_cols[(2, 2)],
    ], // Row 2 (Vector c)
  ];

  let mut new_atoms = Vec::new();
  for (i, pos_frac) in result_cell.positions.iter().enumerate() {
    // Convert Frac -> Cart
    let pos_cart = new_basis_cols * pos_frac;

    // Get Element String
    let type_id = result_cell.numbers[i];
    let element_str = reverse_map
      .get(&type_id)
      .cloned()
      .unwrap_or_else(|| "X".to_string());

    new_atoms.push(Atom {
      element: element_str,
      position: [pos_cart.x, pos_cart.y, pos_cart.z],
      original_index: 0, // Reset index as atoms are reordered/reduced
    });
  }

  // FIXED: Calculate Formula
  let mut formula_counts: HashMap<String, i32> = HashMap::new();
  for atom in &new_atoms {
    *formula_counts.entry(atom.element.clone()).or_insert(0) += 1;
  }
  // Sort elements alphabetically for consistent formula string
  let mut formula_parts: Vec<_> = formula_counts.into_iter().collect();
  formula_parts.sort_by(|a, b| a.0.cmp(&b.0));

  let formula = formula_parts
    .iter()
    .map(|(el, count)| {
      if *count > 1 {
        format!("{}{}", el, count)
      } else {
        el.clone()
      }
    })
    .collect::<Vec<String>>()
    .join("");

  Ok(Structure {
    lattice: new_lattice,
    atoms: new_atoms,
    formula, // FIXED: Added missing field
  })
}
