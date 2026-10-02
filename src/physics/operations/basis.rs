// src/physics/operations/basis.rs

use crate::model::structure::{Atom, Structure};
use crate::physics::operations::conversion::build_formula;
use std::collections::HashSet;

/// Replaces all instances of an element (Global)
pub fn substitute_element(structure: &Structure, target_el: &str, new_el: &str) -> Structure {
    let mut new_atoms = structure.atoms.clone();
    for atom in &mut new_atoms {
        if atom.element == target_el {
            atom.element = new_el.to_string();
        }
    }
    // The composition changed, so the formula must too.
    let formula = build_formula(&new_atoms);
    Structure {
        atoms: new_atoms,
        lattice: structure.lattice,
        formula,
        is_periodic: structure.is_periodic,
    }
}

/// Which `count` of the atoms of `element` to change, for a partial
/// substitution (e.g. one O of three, or a random 25% of a site in a
/// supercell to model disorder). Returns indices into `structure.atoms`,
/// sorted. `seed: None` takes the first `count` in atom order; `Some(seed)`
/// picks at random, reproducibly for a given seed. Asking for at least as many
/// as exist returns them all.
pub fn pick_atoms(
    structure: &Structure,
    element: &str,
    count: usize,
    seed: Option<u64>,
) -> Vec<usize> {
    let mut idx: Vec<usize> = structure
        .atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| a.element == element)
        .map(|(i, _)| i)
        .collect();
    if count >= idx.len() {
        return idx;
    }
    if let Some(seed) = seed {
        // Fisher–Yates with SplitMix64: small, reproducible, no extra crate.
        let mut state = seed;
        let mut next = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        for i in (1..idx.len()).rev() {
            let j = (next() % (i as u64 + 1)) as usize;
            idx.swap(i, j);
        }
    }
    idx.truncate(count);
    idx.sort_unstable();
    idx
}

/// Changes the element of specifically selected atoms (Selection)
pub fn modify_selection(structure: &Structure, indices: &[usize], new_el: &str) -> Structure {
    let mut new_atoms = structure.atoms.clone();

    for &idx in indices {
        if idx < new_atoms.len() {
            new_atoms[idx].element = new_el.to_string();
        }
    }

    let formula = build_formula(&new_atoms);
    Structure {
        atoms: new_atoms,
        lattice: structure.lattice,
        formula,
        is_periodic: structure.is_periodic,
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
        is_periodic: structure.is_periodic,
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
        is_periodic: structure.is_periodic,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn batio3() -> Structure {
        let atom = |el: &str| Atom {
            element: el.to_string(),
            position: [0.0; 3],
            original_index: 0,
            oxidation: None,
            occupancy: 1.0,
        };
        Structure {
            atoms: vec![atom("Ba"), atom("Ti"), atom("O"), atom("O"), atom("O")],
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            formula: "BaO3Ti".to_string(),
            is_periodic: true,
        }
    }

    #[test]
    fn replacing_one_of_three_oxygens_changes_exactly_one() {
        let s = batio3();
        let picks = pick_atoms(&s, "O", 1, None);
        assert_eq!(picks, vec![2], "first O in atom order");
        let out = modify_selection(&s, &picks, "N");
        let n = out.atoms.iter().filter(|a| a.element == "N").count();
        let o = out.atoms.iter().filter(|a| a.element == "O").count();
        assert_eq!((n, o), (1, 2));
        assert_eq!(out.formula, "BaNO2Ti", "formula follows the composition");
    }

    #[test]
    fn random_pick_is_reproducible_and_only_touches_the_element() {
        let s = batio3();
        let a = pick_atoms(&s, "O", 2, Some(7));
        let b = pick_atoms(&s, "O", 2, Some(7));
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|&i| s.atoms[i].element == "O"));
        // Different seeds reach different choices across a few tries.
        let differs = (0..16).any(|k| pick_atoms(&s, "O", 1, Some(k)) != pick_atoms(&s, "O", 1, Some(0)));
        assert!(differs);
    }

    #[test]
    fn asking_for_all_or_more_returns_every_atom_of_the_element() {
        let s = batio3();
        assert_eq!(pick_atoms(&s, "O", 3, Some(1)), vec![2, 3, 4]);
        assert_eq!(pick_atoms(&s, "O", 10, None), vec![2, 3, 4]);
        assert!(pick_atoms(&s, "Zr", 1, None).is_empty());
    }
}
