// src/physics/operations/basis.rs

use crate::model::structure::{Atom, Structure};
use std::collections::HashSet;

/// Replaces all instances of an element (Global)
pub fn substitute_element(structure: &Structure, target_el: &str, new_el: &str) -> Structure {
    let mut new_atoms = structure.atoms.clone();
    for atom in &mut new_atoms {
        if atom.element == target_el {
            atom.element = new_el.to_string();
        }
    }
    Structure {
        atoms: new_atoms,
        lattice: structure.lattice,
        formula: structure.formula.clone(),
    }
}

/// Changes the element of specifically selected atoms (Selection)
pub fn modify_selection(structure: &Structure, indices: &[usize], new_el: &str) -> Structure {
    let mut new_atoms = structure.atoms.clone();

    for &idx in indices {
        if idx < new_atoms.len() {
            new_atoms[idx].element = new_el.to_string();
        }
    }

    Structure {
        atoms: new_atoms,
        lattice: structure.lattice,
        formula: structure.formula.clone(),
    }
}

/// Removes specifically selected atoms
pub fn remove_selection(structure: &Structure, indices: &[usize]) -> Structure {
    // Use a HashSet for fast lookup
    let idx_set: HashSet<usize> = indices.iter().cloned().collect();

    // Keep atoms whose index is NOT in the set
    let new_atoms: Vec<Atom> = structure
        .atoms
        .iter()
        .enumerate()
        .filter(|(i, _)| !idx_set.contains(i))
        .map(|(_, atom)| atom.clone())
        .collect();

    // Note: This re-indexes atoms. The UI selection must be cleared after this op.
    Structure {
        atoms: new_atoms,
        lattice: structure.lattice,
        formula: structure.formula.clone(),
    }
}

pub fn standardize_positions(structure: &Structure) -> Structure {
    let mut new_atoms = structure.atoms.clone();
    for atom in &mut new_atoms {
        let mut frac = crate::utils::linalg::cart_to_frac(atom.position, structure.lattice)
            .unwrap_or([0.0; 3]);
        frac.iter_mut().for_each(|x| *x = x.rem_euclid(1.0));
        atom.position = crate::utils::linalg::frac_to_cart(frac, structure.lattice);
    }
    Structure {
        atoms: new_atoms,
        lattice: structure.lattice,
        formula: structure.formula.clone(),
    }
}
