// src/physics/operations/conversion.rs
//
// Converts a Structure between Primitive and Conventional standard cells
// using the Moyo symmetry library (IUCr/Spglib conventions).

use crate::model::elements::get_atomic_number;
use crate::model::structure::{Atom, Structure};
use crate::utils::linalg::{cart_to_frac, frac_to_cart, lattice_to_matrix3, matrix3_to_arr};
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;
use nalgebra::Vector3;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
pub enum CellType {
    Primitive,
    Conventional,
}

pub fn convert_structure(structure: &Structure, cell_type: CellType) -> Result<Structure, String> {
    // 1. Build element ↔ integer mapping using atomic numbers.
    //    This is scientifically correct (unique per element) and avoids
    //    fragile alphabetical-sort or insertion-order schemes.
    let mut element_to_z: HashMap<String, i32> = HashMap::new();
    let mut z_to_element: HashMap<i32, String> = HashMap::new();

    for atom in &structure.atoms {
        if !element_to_z.contains_key(&atom.element) {
            let z = get_atomic_number(&atom.element);
            // Fallback: if element is unknown, assign a unique negative id
            let id = if z > 0 {
                z
            } else {
                -(element_to_z.len() as i32 + 1)
            };
            element_to_z.insert(atom.element.clone(), id);
            z_to_element.insert(id, atom.element.clone());
        }
    }

    // 2. Prepare Moyo input: lattice matrix (rows = lattice vectors) + fractional positions
    let lat_mat = lattice_to_matrix3(structure.lattice);

    let mut positions = Vec::with_capacity(structure.atoms.len());
    let mut numbers = Vec::with_capacity(structure.atoms.len());

    for atom in &structure.atoms {
        let frac = cart_to_frac(atom.position, structure.lattice)
            .ok_or("Lattice is singular (volume is zero)")?;
        positions.push(Vector3::from(frac));
        numbers.push(element_to_z[&atom.element]);
    }

    let moyo_cell = Cell::new(Lattice::new(lat_mat), positions, numbers);

    // 3. Symmetry detection + standardization
    let dataset = MoyoDataset::new(
        &moyo_cell,
        1e-4,
        AngleTolerance::Default,
        Setting::Spglib,
        true,
    )
    .map_err(|e| format!("Symmetry search failed: {:?}", e))?;

    // 4. Select the output cell
    let result_cell = match cell_type {
        CellType::Primitive => &dataset.prim_std_cell,
        CellType::Conventional => &dataset.std_cell,
    };

    // 5. Convert Moyo result back to Structure
    let new_lattice = matrix3_to_arr(result_cell.lattice.basis);

    let mut new_atoms = Vec::with_capacity(result_cell.positions.len());
    for (i, pos_frac) in result_cell.positions.iter().enumerate() {
        // Wrap fractional coordinates to [0, 1) — Moyo can return values
        // slightly outside this range due to floating-point standardization
        let frac = [
            pos_frac.x.rem_euclid(1.0),
            pos_frac.y.rem_euclid(1.0),
            pos_frac.z.rem_euclid(1.0),
        ];
        let position = frac_to_cart(frac, new_lattice);

        let type_id = result_cell.numbers[i];
        let element = z_to_element
            .get(&type_id)
            .cloned()
            .unwrap_or_else(|| "X".to_string());

        new_atoms.push(Atom {
            element,
            position,
            original_index: i,
        });
    }

    // 6. Build formula string
    let formula = build_formula(&new_atoms);

    Ok(Structure {
        lattice: new_lattice,
        atoms: new_atoms,
        formula,
        is_periodic: structure.is_periodic,
    })
}

/// Build a chemical formula string sorted alphabetically (e.g. "Cl6Cs2Mo").
fn build_formula(atoms: &[Atom]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for atom in atoms {
        *counts.entry(&atom.element).or_insert(0) += 1;
    }
    let mut parts: Vec<_> = counts.into_iter().collect();
    parts.sort_by(|a, b| a.0.cmp(b.0));

    parts
        .iter()
        .map(|(el, count)| {
            if *count > 1 {
                format!("{}{}", el, count)
            } else {
                (*el).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("")
}
