// src/physics/operations/conversion.rs
use crate::model::structure::{Atom, Structure};
use crate::utils::linalg::{cart_to_frac, frac_to_cart};
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
    // Row-major lattice matrix (rows = lattice vectors)
    let lat_mat = Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    );

    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let frac =
            cart_to_frac(atom.position, lat).ok_or("Lattice is singular (volume is zero)")?;
        positions.push(Vector3::from(frac));
        numbers.push(*element_map.get(&atom.element).unwrap());
    }

    // Moyo Lattice::new takes vectors as ROWS — lat_mat is already row-major
    let moyo_cell = Cell::new(Lattice::new(lat_mat), positions, numbers);

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

    // 5. Convert Moyo Cell Back to Structure
    // Moyo returns lattice as rows — that matches our [[f64;3];3] convention directly
    let m = result_cell.lattice.basis; // row-major
    let new_lattice = [
        [m.m11, m.m12, m.m13],
        [m.m21, m.m22, m.m23],
        [m.m31, m.m32, m.m33],
    ];

    let mut new_atoms = Vec::new();
    for (i, pos_frac) in result_cell.positions.iter().enumerate() {
        let frac = [pos_frac.x, pos_frac.y, pos_frac.z];
        let position = frac_to_cart(frac, new_lattice);

        let type_id = result_cell.numbers[i];
        let element_str = reverse_map
            .get(&type_id)
            .cloned()
            .unwrap_or_else(|| "X".to_string());

        new_atoms.push(Atom {
            element: element_str,
            position,
            original_index: 0,
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
        is_periodic: structure.is_periodic,
    })
}
