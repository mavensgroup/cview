// src/physics/operations/atom_edit.rs
//
// Adding and moving single atoms: the edits behind the Structure window's
// Atoms tab. Positions are Cartesian (Å) throughout; the tab converts from
// fractional input. Nothing is wrapped into the cell — the atoms land where
// they are put, as typed (Basis → Standardize positions wraps on request).

use crate::model::structure::{Atom, Structure};
use crate::physics::operations::conversion::build_formula;
use crate::utils::geometry::minimum_image;

/// `structure` with one more atom of `element` at `cart`, appended at the
/// end so every existing atom keeps its index.
pub fn add_atom(structure: &Structure, element: &str, cart: [f64; 3]) -> Structure {
    let mut atoms = structure.atoms.clone();
    atoms.push(Atom {
        element: element.to_string(),
        position: cart,
        original_index: atoms.len(),
        oxidation: None,
        occupancy: 1.0,
    });
    rebuilt(structure, atoms)
}

/// `structure` with the atoms at `indices` shifted by the Cartesian `delta`.
/// Indices out of range are ignored.
pub fn translate_atoms(structure: &Structure, indices: &[usize], delta: [f64; 3]) -> Structure {
    let mut atoms = structure.atoms.clone();
    for &i in indices {
        if let Some(a) = atoms.get_mut(i) {
            for (p, d) in a.position.iter_mut().zip(delta) {
                *p += d;
            }
        }
    }
    rebuilt(structure, atoms)
}

/// `structure` with atom `index` placed at `cart`.
pub fn place_atom(structure: &Structure, index: usize, cart: [f64; 3]) -> Structure {
    let mut atoms = structure.atoms.clone();
    if let Some(a) = atoms.get_mut(index) {
        a.position = cart;
    }
    rebuilt(structure, atoms)
}

/// The atom nearest to `cart` (its index and distance in Å), skipping the
/// indices in `exclude`. Distances are to the nearest periodic image for a
/// periodic structure. `None` when no atom is left to compare with.
pub fn nearest_atom(structure: &Structure, cart: [f64; 3], exclude: &[usize]) -> Option<(usize, f64)> {
    structure
        .atoms
        .iter()
        .enumerate()
        .filter(|(i, _)| !exclude.contains(i))
        .map(|(i, a)| {
            let mut d = [0.0; 3];
            for k in 0..3 {
                d[k] = a.position[k] - cart[k];
            }
            if structure.is_periodic {
                if let Some((v, _)) = minimum_image(d, &structure.lattice) {
                    d = v;
                }
            }
            (i, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn rebuilt(structure: &Structure, atoms: Vec<Atom>) -> Structure {
    Structure {
        formula: build_formula(&atoms),
        atoms,
        lattice: structure.lattice,
        is_periodic: structure.is_periodic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cubic(atoms: &[(&str, [f64; 3])]) -> Structure {
        let atoms: Vec<Atom> = atoms
            .iter()
            .enumerate()
            .map(|(i, (el, p))| Atom {
                element: el.to_string(),
                position: *p,
                original_index: i,
                oxidation: None,
                occupancy: 1.0,
            })
            .collect();
        Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            formula: build_formula(&atoms),
            atoms,
            is_periodic: true,
        }
    }

    #[test]
    fn add_appends_and_updates_the_formula() {
        let s = cubic(&[("Na", [0.0; 3])]);
        let t = add_atom(&s, "Cl", [2.0, 2.0, 2.0]);
        assert_eq!(t.atoms.len(), 2);
        assert_eq!(t.atoms[0].element, "Na");
        assert_eq!(t.atoms[1].element, "Cl");
        assert_eq!(t.atoms[1].position, [2.0, 2.0, 2.0]);
        assert_eq!(t.formula, build_formula(&t.atoms));
        assert_ne!(t.formula, s.formula);
    }

    #[test]
    fn translate_moves_only_the_chosen_atoms() {
        let s = cubic(&[("Fe", [0.0; 3]), ("O", [1.0, 0.0, 0.0]), ("O", [2.0, 0.0, 0.0])]);
        let t = translate_atoms(&s, &[1, 2, 99], [0.5, -0.5, 0.0]);
        assert_eq!(t.atoms[0].position, [0.0; 3]);
        assert_eq!(t.atoms[1].position, [1.5, -0.5, 0.0]);
        assert_eq!(t.atoms[2].position, [2.5, -0.5, 0.0]);
    }

    #[test]
    fn place_sets_one_position() {
        let s = cubic(&[("Fe", [0.0; 3]), ("O", [1.0, 0.0, 0.0])]);
        let t = place_atom(&s, 1, [3.0, 3.0, 3.0]);
        assert_eq!(t.atoms[1].position, [3.0, 3.0, 3.0]);
        assert_eq!(t.atoms[0].position, [0.0; 3]);
    }

    #[test]
    fn nearest_uses_the_periodic_image() {
        // 3.8 Å apart directly, 0.2 Å through the cell boundary.
        let s = cubic(&[("Fe", [0.1, 0.0, 0.0]), ("O", [2.0, 3.5, 3.5])]);
        let (i, d) = nearest_atom(&s, [3.9, 0.0, 0.0], &[]).unwrap();
        assert_eq!(i, 0);
        assert!((d - 0.2).abs() < 1e-9);

        let mut m = s.clone();
        m.is_periodic = false;
        let (_, d) = nearest_atom(&m, [3.9, 0.0, 0.0], &[]).unwrap();
        assert!((d - 3.8).abs() < 1e-9);
    }

    #[test]
    fn nearest_skips_excluded_atoms() {
        let s = cubic(&[("Fe", [0.0; 3]), ("O", [2.0, 0.0, 0.0])]);
        let (i, _) = nearest_atom(&s, [0.1, 0.0, 0.0], &[0]).unwrap();
        assert_eq!(i, 1);
        assert!(nearest_atom(&s, [0.0; 3], &[0, 1]).is_none());
    }
}
