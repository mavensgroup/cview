// src/rendering/occupancy.rs
//
// Groups partially occupied atoms into crystallographic sites so the painter
// can draw each site once, as a sphere split into pie sectors by occupancy
// (the VESTA convention). A CIF split site such as Fe0.5/Cr0.5 arrives as two
// `Atom`s at the same position; without grouping both spheres are drawn on
// top of each other and whichever is painted last hides the other.

use crate::model::structure::Structure;
use crate::utils::geometry;
use std::collections::{HashMap, HashSet};

/// Occupancies at or above this count as full and never form a pie.
const FULL_OCCUPANCY: f64 = 0.999;
/// Atoms closer than this (Å, nearest periodic image) share a site. The CIF
/// reader merges coincident positions at 1e-3 fractional, i.e. ~0.01 Å in a
/// typical cell, so this keeps its split sites together without merging
/// genuinely distinct positions.
const SITE_TOLERANCE: f64 = 0.02;

#[derive(Default)]
pub struct PartialSites {
    /// Representative atom index → every atom on that site, highest
    /// occupancy first. The representative is the first entry.
    sites: HashMap<usize, Vec<usize>>,
    /// Non-representative atoms. They are drawn as part of their site's pie
    /// and must not be drawn, bonded or picked on their own.
    hidden: HashSet<usize>,
}

impl PartialSites {
    pub fn build(structure: &Structure) -> Self {
        let partial: Vec<usize> = structure
            .atoms
            .iter()
            .enumerate()
            .filter(|(_, a)| a.occupancy < FULL_OCCUPANCY)
            .map(|(i, _)| i)
            .collect();
        // Fully ordered structures, i.e. almost every file, pay nothing more.
        if partial.is_empty() {
            return Self::default();
        }

        let separation = |i: usize, j: usize| -> f64 {
            let (a, b) = (structure.atoms[i].position, structure.atoms[j].position);
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = if structure.is_periodic {
                geometry::minimum_image(d, &structure.lattice).map_or(d, |(v, _)| v)
            } else {
                d
            };
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
        };

        // Partially occupied atoms are few, so a pairwise scan is fine.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &i in &partial {
            match groups
                .iter_mut()
                .find(|g| separation(g[0], i) < SITE_TOLERANCE)
            {
                Some(g) => g.push(i),
                None => groups.push(vec![i]),
            }
        }

        let mut out = Self::default();
        for mut members in groups {
            members.sort_by(|&a, &b| {
                structure.atoms[b]
                    .occupancy
                    .partial_cmp(&structure.atoms[a].occupancy)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.cmp(&b))
            });
            out.hidden.extend(members[1..].iter().copied());
            out.sites.insert(members[0], members);
        }
        out
    }

    /// True for an atom drawn as part of another atom's site.
    pub fn is_hidden(&self, index: usize) -> bool {
        self.hidden.contains(&index)
    }

    /// Every atom on the site `index` represents, or `None` when `index` is
    /// fully occupied (or hidden).
    pub fn members(&self, index: usize) -> Option<&[usize]> {
        self.sites.get(&index).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    fn atom(element: &str, position: [f64; 3], occupancy: f64) -> Atom {
        Atom {
            element: element.to_string(),
            position,
            original_index: 0,
            oxidation: None,
            occupancy,
        }
    }

    fn cubic(atoms: Vec<Atom>) -> Structure {
        Structure {
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            atoms,
            formula: String::new(),
            is_periodic: true,
        }
    }

    #[test]
    fn ordered_structure_has_no_sites() {
        let s = cubic(vec![atom("Na", [0.0; 3], 1.0), atom("Cl", [2.0; 3], 1.0)]);
        let p = PartialSites::build(&s);
        assert!(p.members(0).is_none() && p.members(1).is_none());
        assert!(!p.is_hidden(0) && !p.is_hidden(1));
    }

    #[test]
    fn split_site_groups_under_majority_species() {
        let s = cubic(vec![
            atom("Fe", [1.0, 1.0, 1.0], 0.3),
            atom("O", [2.0, 2.0, 2.0], 1.0),
            atom("Cr", [1.0, 1.0, 1.0], 0.7),
        ]);
        let p = PartialSites::build(&s);
        assert_eq!(p.members(2), Some(&[2, 0][..]));
        assert!(p.is_hidden(0));
        assert!(!p.is_hidden(2) && !p.is_hidden(1));
    }

    #[test]
    fn split_site_found_across_cell_boundary() {
        // Wrapped fractional coordinates can land one site at 0 and its
        // partner just under 1; they are still the same site.
        let s = cubic(vec![
            atom("Sr", [0.0, 0.0, 0.0], 0.5),
            atom("Ba", [3.999, 0.0, 0.0], 0.5),
        ]);
        let p = PartialSites::build(&s);
        assert_eq!(p.members(0), Some(&[0, 1][..]));
        assert!(p.is_hidden(1));
    }

    #[test]
    fn lone_partial_atom_is_its_own_site() {
        let s = cubic(vec![atom("Li", [1.0, 0.0, 0.0], 0.6)]);
        let p = PartialSites::build(&s);
        assert_eq!(p.members(0), Some(&[0][..]));
    }
}
