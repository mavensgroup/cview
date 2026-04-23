// src/utils/spatial_grid.rs
//
// Uniform-cell spatial hash for O(1)-amortized neighbor queries on
// RenderAtom slices. Used to replace the O(N²) nested loops in the
// per-frame hot paths (bond detection in the painter, coordination
// neighbor search in the polyhedra builder).
//
// The grid is built once per frame against a filter (e.g. "only anions"
// for polyhedra, or "not a coord-only ghost" for bonds). Queries return
// atom indices into the ORIGINAL slice — cheap to combine with the
// existing per-atom logic downstream.
//
// Cell size should be ≥ the maximum query radius you intend to use;
// otherwise the grid still returns the correct answer but has to scan
// more cells per query.

use crate::rendering::scene::RenderAtom;
use std::collections::HashMap;

pub struct SpatialGrid {
    /// Integer-coordinate cell → list of atom indices included at build time.
    cells: HashMap<(i32, i32, i32), Vec<usize>>,
    /// Copy of cart_pos for every atom in the original slice. Indexed by
    /// the caller's atom index, so `query()` can resolve distances without
    /// borrowing the atoms slice back from the caller.
    positions: Vec<[f64; 3]>,
    cell_size: f64,
}

impl SpatialGrid {
    /// Build a grid over `atoms`. Only atoms where `include(atom)` is true
    /// are inserted into the grid; the rest are still tracked in `positions`
    /// so indices returned from `query` refer to the same slice the caller
    /// sees.
    ///
    /// `cell_size` should be at least as large as the biggest radius you
    /// intend to query with. For the bond loop that's the hard 4 Å cap;
    /// for polyhedra it's the user's `max_bond_dist`.
    pub fn build<F>(atoms: &[RenderAtom], cell_size: f64, include: F) -> Self
    where
        F: Fn(&RenderAtom) -> bool,
    {
        let cs = if cell_size > 1e-6 { cell_size } else { 1.0 };
        let mut cells: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
        let mut positions = Vec::with_capacity(atoms.len());

        for (i, atom) in atoms.iter().enumerate() {
            positions.push(atom.cart_pos);
            if !include(atom) {
                continue;
            }
            let key = cell_key(atom.cart_pos, cs);
            cells.entry(key).or_default().push(i);
        }

        Self {
            cells,
            positions,
            cell_size: cs,
        }
    }

    /// Append indices of atoms within `radius` of `pos` to `out`. Only atoms
    /// that passed the `include` filter at build time are reported. `out` is
    /// NOT cleared — the caller is expected to reuse a scratch buffer.
    pub fn query(&self, pos: [f64; 3], radius: f64, out: &mut Vec<usize>) {
        let span = (radius / self.cell_size).ceil() as i32;
        let cx = (pos[0] / self.cell_size).floor() as i32;
        let cy = (pos[1] / self.cell_size).floor() as i32;
        let cz = (pos[2] / self.cell_size).floor() as i32;
        let r2 = radius * radius;

        for dx in -span..=span {
            for dy in -span..=span {
                for dz in -span..=span {
                    let key = (cx + dx, cy + dy, cz + dz);
                    if let Some(bucket) = self.cells.get(&key) {
                        for &idx in bucket {
                            let p = self.positions[idx];
                            let ex = p[0] - pos[0];
                            let ey = p[1] - pos[1];
                            let ez = p[2] - pos[2];
                            if ex * ex + ey * ey + ez * ez <= r2 {
                                out.push(idx);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn cell_key(p: [f64; 3], size: f64) -> (i32, i32, i32) {
    (
        (p[0] / size).floor() as i32,
        (p[1] / size).floor() as i32,
        (p[2] / size).floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_atom(x: f64, y: f64, z: f64) -> RenderAtom {
        RenderAtom {
            screen_pos: [0.0; 3],
            cart_pos: [x, y, z],
            element: "X".to_string(),
            original_index: 0,
            unique_id: 0,
            is_ghost: false,
            is_coord_only: false,
            screen_radius: 0.0,
        }
    }

    #[test]
    fn grid_finds_atoms_within_radius() {
        let atoms = vec![
            mk_atom(0.0, 0.0, 0.0),
            mk_atom(1.0, 0.0, 0.0),
            mk_atom(5.0, 0.0, 0.0),
            mk_atom(10.0, 10.0, 10.0),
        ];
        let grid = SpatialGrid::build(&atoms, 2.0, |_| true);
        let mut out = Vec::new();
        grid.query([0.0, 0.0, 0.0], 2.0, &mut out);
        out.sort();
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn grid_respects_include_filter() {
        let atoms = vec![mk_atom(0.0, 0.0, 0.0), mk_atom(0.5, 0.0, 0.0)];
        // Include only index 1
        let grid = SpatialGrid::build(&atoms, 2.0, |a| a.cart_pos[0] > 0.1);
        let mut out = Vec::new();
        grid.query([0.0, 0.0, 0.0], 5.0, &mut out);
        assert_eq!(out, vec![1]);
    }

    #[test]
    fn query_radius_larger_than_cell_still_correct() {
        let atoms: Vec<RenderAtom> = (0..10)
            .map(|i| mk_atom(i as f64 * 0.5, 0.0, 0.0))
            .collect();
        let grid = SpatialGrid::build(&atoms, 1.0, |_| true);
        let mut out = Vec::new();
        grid.query([0.0, 0.0, 0.0], 3.0, &mut out);
        out.sort();
        // Positions 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0 are within 3.0
        assert_eq!(out, vec![0, 1, 2, 3, 4, 5, 6]);
    }
}
