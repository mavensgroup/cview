use crate::model::structure::{Atom, Structure};
use nalgebra::{Matrix3, Vector3};
use spglib::cell::Cell;
use spglib::dataset::Dataset;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, PartialEq)]
pub enum CellType {
  Primitive,
  Conventional,
}

pub fn convert_structure(structure: &Structure, cell_type: CellType) -> Result<Structure, String> {
  // 1. Map Elements to Integers
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

  // 2. Prepare Data for Spglib
  let lat = structure.lattice;
  let spg_lattice = lat;

  // Basis Matrix (Columns)
  let basis_cols = Matrix3::new(
    lat[0][0], lat[1][0], lat[2][0], lat[0][1], lat[1][1], lat[2][1], lat[0][2], lat[1][2],
    lat[2][2],
  );
  let inv_basis = basis_cols
    .try_inverse()
    .ok_or("Invalid lattice: singular matrix")?;

  let positions: Vec<[f64; 3]> = structure
    .atoms
    .iter()
    .map(|a| {
      let p = a.position;
      let vec = Vector3::new(p[0], p[1], p[2]);
      let frac = inv_basis * vec;
      [frac[0], frac[1], frac[2]]
    })
    .collect();

  let types: Vec<i32> = structure
    .atoms
    .iter()
    .map(|a| *element_map.get(&a.element).unwrap_or(&0))
    .collect();

  // 3. Create Spglib Cell and Dataset
  let mut cell = Cell::new(&spg_lattice, &positions, &types);
  let dataset = Dataset::new(&mut cell, 1e-5); // Returns Dataset directly in 1.15.1

  // 4. Extract New Structure Data based on CellType
  let (final_lattice, new_atoms) = match cell_type {
    CellType::Conventional => {
      // Standardized Cell (std_lattice, std_positions)
      let lat_rows = dataset.std_lattice;

      // Reconstruct Basis (Cols)
      let new_basis = Matrix3::new(
        lat_rows[0][0],
        lat_rows[1][0],
        lat_rows[2][0],
        lat_rows[0][1],
        lat_rows[1][1],
        lat_rows[2][1],
        lat_rows[0][2],
        lat_rows[1][2],
        lat_rows[2][2],
      );

      let mut atoms_list = Vec::new();
      for (i, &type_id) in dataset.std_types.iter().enumerate() {
        let pf = dataset.std_positions[i];
        let pos_frac = Vector3::new(pf[0], pf[1], pf[2]);
        let pos_cart = new_basis * pos_frac;

        let element = reverse_map
          .get(&type_id)
          .cloned()
          .unwrap_or_else(|| "X".to_string());

        atoms_list.push(Atom {
          element,
          position: [pos_cart.x, pos_cart.y, pos_cart.z],
          original_index: 0,
        });
      }
      (lat_rows, atoms_list)
    }
    CellType::Primitive => {
      // Primitive Lattice
      let prim_lat_rows = dataset.primitive_lattice;

      let prim_basis_cols = Matrix3::new(
        prim_lat_rows[0][0],
        prim_lat_rows[1][0],
        prim_lat_rows[2][0],
        prim_lat_rows[0][1],
        prim_lat_rows[1][1],
        prim_lat_rows[2][1],
        prim_lat_rows[0][2],
        prim_lat_rows[1][2],
        prim_lat_rows[2][2],
      );
      let inv_prim_basis = prim_basis_cols
        .try_inverse()
        .ok_or("Invalid primitive lattice")?;

      let mut atoms_list = Vec::new();

      // Map: Primitive Slot Index -> Original Atom Index
      // We want to find ONE representative original atom for each primitive slot.
      // BTreeMap ensures we process primitive slots (0, 1, 2, 3...) in order.
      let mut primitive_slot_representatives: BTreeMap<usize, usize> = BTreeMap::new();

      // dataset.mapping_to_primitive maps: Original Index [i] -> Primitive Slot [p]
      for (original_idx, &prim_slot) in dataset.mapping_to_primitive.iter().enumerate() {
        let prim_slot = prim_slot as usize;
        // If we haven't found a representative for this slot yet, take this atom
        primitive_slot_representatives
          .entry(prim_slot)
          .or_insert(original_idx);
      }

      // Now reconstruct the atoms
      for (_, &original_idx) in primitive_slot_representatives.iter() {
        let original_atom = &structure.atoms[original_idx];

        let p = original_atom.position;
        let pos_cart_orig = Vector3::new(p[0], p[1], p[2]);

        // Convert Original Cartesian -> New Primitive Fractional
        let mut pos_frac_prim = inv_prim_basis * pos_cart_orig;

        // Wrap to [0, 1) to keep atoms inside the primitive box
        let tol = 1e-4; // tolerance for wrapping
        pos_frac_prim.x = pos_frac_prim.x - (pos_frac_prim.x - tol).floor();
        pos_frac_prim.y = pos_frac_prim.y - (pos_frac_prim.y - tol).floor();
        pos_frac_prim.z = pos_frac_prim.z - (pos_frac_prim.z - tol).floor();

        let pos_cart_final = prim_basis_cols * pos_frac_prim;

        atoms_list.push(Atom {
          element: original_atom.element.clone(),
          position: [pos_cart_final.x, pos_cart_final.y, pos_cart_final.z],
          original_index: 0,
        });
      }
      (prim_lat_rows, atoms_list)
    }
  };

  // 5. Generate Formula
  let mut formula_map: HashMap<String, usize> = HashMap::new();
  for atom in &new_atoms {
    *formula_map.entry(atom.element.clone()).or_insert(0) += 1;
  }
  let mut formula_parts: Vec<_> = formula_map.into_iter().collect();
  formula_parts.sort_by(|a, b| a.0.cmp(&b.0));
  let formula = formula_parts
    .into_iter()
    .map(|(el, count)| {
      if count > 1 {
        format!("{}{}", el, count)
      } else {
        el
      }
    })
    .collect::<Vec<_>>()
    .join("");

  Ok(Structure {
    atoms: new_atoms,
    lattice: final_lattice,
    formula,
  })
}
