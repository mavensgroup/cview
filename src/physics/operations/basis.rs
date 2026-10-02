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

/// Turns each atom in `indices` into a mixed site: it keeps its element at
/// `(1 − fraction)` of its occupancy, and `new_el` takes `fraction` of it, at
/// the same position (the representation of a disordered site: two atoms at one
/// position, each partially occupied). A site that already holds `new_el` has
/// that species' occupancy increased instead of gaining a duplicate.
///
/// Atoms are re-indexed, so anything keyed by atom index (overrides,
/// selection) no longer applies; the caller clears it.
pub fn mix_sites(
    structure: &Structure,
    indices: &[usize],
    new_el: &str,
    fraction: f64,
) -> Structure {
    let x = fraction.clamp(0.0, 1.0);
    let targets: HashSet<usize> = indices
        .iter()
        .copied()
        .filter(|&i| i < structure.atoms.len() && structure.atoms[i].element != new_el)
        .collect();
    if x <= 0.0 || targets.is_empty() {
        return structure.clone();
    }

    let same_position = |a: &Atom, b: &Atom| {
        (0..3).all(|k| (a.position[k] - b.position[k]).abs() < 1e-6)
    };

    // First pass: lower each target's occupancy and note what each site gains.
    let mut atoms: Vec<Atom> = structure.atoms.clone();
    let mut gains: Vec<(usize, f64)> = Vec::new(); // (target index, occupancy for new_el)
    for &i in &targets {
        let old = atoms[i].occupancy;
        atoms[i].occupancy = old * (1.0 - x);
        gains.push((i, old * x));
    }
    gains.sort_by_key(|g| g.0);

    // Second pass: give the share to an existing `new_el` atom on that site if
    // there is one, else insert a new atom right after the target.
    let mut inserts: Vec<(usize, Atom)> = Vec::new();
    for (i, share) in gains {
        let host = atoms[i].clone();
        match atoms
            .iter_mut()
            .find(|a| a.element == new_el && same_position(a, &host))
        {
            Some(existing) => existing.occupancy += share,
            None => {
                let mut a = host;
                a.element = new_el.to_string();
                a.occupancy = share;
                a.oxidation = None; // the new species' charge is not known
                inserts.push((i, a));
            }
        }
    }

    let mut out: Vec<Atom> = Vec::with_capacity(atoms.len() + inserts.len());
    for (i, a) in atoms.into_iter().enumerate() {
        out.push(a);
        for (at, new_atom) in &inserts {
            if *at == i {
                out.push(new_atom.clone());
            }
        }
    }
    for (n, a) in out.iter_mut().enumerate() {
        a.original_index = n;
    }

    let formula = build_formula(&out);
    Structure {
        atoms: out,
        lattice: structure.lattice,
        formula,
        is_periodic: structure.is_periodic,
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
    fn mixing_one_oxygen_gives_a_two_species_site() {
        let s = batio3();
        let out = mix_sites(&s, &[2], "N", 0.25);
        assert_eq!(out.atoms.len(), 6, "one atom added");
        let (o, n) = (&out.atoms[2], &out.atoms[3]);
        assert_eq!((o.element.as_str(), n.element.as_str()), ("O", "N"));
        assert!((o.occupancy - 0.75).abs() < 1e-12 && (n.occupancy - 0.25).abs() < 1e-12);
        assert_eq!(o.position, n.position, "same site");
        // The site is still fully occupied, and the rest are untouched.
        assert!((o.occupancy + n.occupancy - 1.0).abs() < 1e-12);
        assert_eq!(out.atoms[4].element, "O");
        assert_eq!(out.atoms[4].occupancy, 1.0);
        // Indices are consecutive, and the formula carries the fractions.
        assert!(out.atoms.iter().enumerate().all(|(i, a)| a.original_index == i));
        assert_eq!(out.formula, "BaN0.25O2.75Ti");
    }

    #[test]
    fn mixing_every_oxygen_to_a_third_gives_the_same_composition_per_site() {
        let s = batio3();
        let out = mix_sites(&s, &[2, 3, 4], "N", 1.0 / 3.0);
        assert_eq!(out.atoms.len(), 8);
        let n_total: f64 = out.atoms.iter().filter(|a| a.element == "N").map(|a| a.occupancy).sum();
        assert!((n_total - 1.0).abs() < 1e-12, "3 sites x 1/3 = one N in total");
        assert_eq!(out.formula, "BaNO2Ti");
    }

    #[test]
    fn mixing_in_the_same_element_again_adds_to_it_without_duplicating() {
        let s = mix_sites(&batio3(), &[2], "N", 0.25);
        // Mix 20% more N into the O half of that site (index 2).
        let out = mix_sites(&s, &[2], "N", 0.2);
        assert_eq!(out.atoms.len(), s.atoms.len(), "no extra atom");
        let n: f64 = out.atoms.iter().filter(|a| a.element == "N").map(|a| a.occupancy).sum();
        assert!((n - (0.25 + 0.75 * 0.2)).abs() < 1e-12);
    }

    #[test]
    fn mixing_with_the_same_element_or_zero_fraction_changes_nothing() {
        let s = batio3();
        assert_eq!(mix_sites(&s, &[2], "O", 0.5).atoms.len(), s.atoms.len());
        assert_eq!(mix_sites(&s, &[2], "N", 0.0).atoms.len(), s.atoms.len());
        assert_eq!(mix_sites(&s, &[], "N", 0.5).atoms.len(), s.atoms.len());
    }

    #[test]
    fn asking_for_all_or_more_returns_every_atom_of_the_element() {
        let s = batio3();
        assert_eq!(pick_atoms(&s, "O", 3, Some(1)), vec![2, 3, 4]);
        assert_eq!(pick_atoms(&s, "O", 10, None), vec![2, 3, 4]);
        assert!(pick_atoms(&s, "Zr", 1, None).is_empty());
    }
}

#[cfg(test)]
mod mixed_site_guard_tests {
    use super::*;
    use crate::physics::operations::conversion::{convert_structure, CellType};
    use crate::physics::operations::slab::generate_slab;
    use crate::physics::operations::supercell;

    fn mixed() -> Structure {
        let atom = |el: &str, pos: [f64; 3], occ: f64| Atom {
            element: el.to_string(),
            position: pos,
            original_index: 0,
            oxidation: None,
            occupancy: occ,
        };
        Structure {
            atoms: vec![
                atom("Ba", [0.0, 0.0, 0.0], 1.0),
                atom("O", [2.0, 0.0, 0.0], 0.5),
                atom("N", [2.0, 0.0, 0.0], 0.5),
            ],
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            formula: String::new(),
            is_periodic: true,
        }
    }

    #[test]
    fn slab_and_cell_conversion_refuse_mixed_sites_instead_of_corrupting_them() {
        let s = mixed();
        let e = generate_slab(&s, 1, 0, 0, 1, 10.0).unwrap_err();
        assert!(e.contains("partially occupied"), "{e}");
        let e = convert_structure(&s, CellType::Primitive).unwrap_err();
        assert!(e.contains("partially occupied"), "{e}");
    }

    #[test]
    fn supercell_keeps_mixed_sites_intact() {
        let out = supercell::transform(&mixed(), [[2, 0, 0], [0, 1, 0], [0, 0, 1]]);
        assert_eq!(out.atoms.len(), 6);
        let (o, n): (Vec<_>, Vec<_>) = out
            .atoms
            .iter()
            .filter(|a| a.element != "Ba")
            .partition(|a| a.element == "O");
        assert_eq!((o.len(), n.len()), (2, 2));
        assert!(out.atoms.iter().filter(|a| a.element != "Ba").all(|a| (a.occupancy - 0.5).abs() < 1e-12));
        assert_eq!(out.formula, "Ba2NO");
    }
}
