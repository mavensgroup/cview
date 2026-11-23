// src/model/mesh_grid.rs
//
// The regular grid an OVF magnetization mesh sits on, recovered from site
// positions: grid index per site, cell edges, the film normal. Shared by the
// main view (colour-map tiles, arrow thinning) and the Trajectory Player.

/// A regular mesh of sites: grid index per site.
#[derive(Debug, Clone)]
pub struct MeshGrid {
    pub idx: Vec<[i64; 3]>,
    /// Smallest cell edge, Å.
    pub spacing: f64,
    /// Cell edge along each axis, Å (0 along an axis with one cell).
    pub step: [f64; 3],
    /// Sites along each axis.
    pub counts: [i64; 3],
    /// The axis with fewest sites (a film's normal).
    pub normal: usize,
    /// The shortest axis is comparable to the others: a 3D bulk mesh, not a
    /// thin film.
    pub bulk: bool,
}

impl MeshGrid {
    /// The grid `positions` sit on. None for fewer than two sites or when
    /// they all coincide.
    pub fn of(positions: &[[f64; 3]]) -> Option<MeshGrid> {
        if positions.len() < 2 {
            return None;
        }
        let mut lo = [0.0; 3];
        let mut step = [0.0; 3];
        let mut spacing = f64::MAX;
        for d in 0..3 {
            let mut v: Vec<f64> = positions.iter().map(|p| p[d]).collect();
            v.sort_by(f64::total_cmp);
            lo[d] = v[0];
            let tol = 1e-6 * (v[v.len() - 1] - v[0]).max(1.0);
            let gap = v.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > tol).fold(f64::MAX, f64::min);
            step[d] = if gap == f64::MAX { 0.0 } else { gap };
            if step[d] > 0.0 {
                spacing = spacing.min(step[d]);
            }
        }
        if spacing == f64::MAX {
            return None;
        }
        let mut counts = [1i64; 3];
        let idx: Vec<[i64; 3]> = positions
            .iter()
            .map(|p| {
                [0, 1, 2].map(|d| {
                    let i = if step[d] > 0.0 { ((p[d] - lo[d]) / step[d]).round() as i64 } else { 0 };
                    counts[d] = counts[d].max(i + 1);
                    i
                })
            })
            .collect();
        let normal = (0..3).min_by_key(|&d| counts[d]).unwrap_or(2);
        let longest = (0..3).filter(|&d| d != normal).map(|d| counts[d]).max().unwrap_or(1);
        let bulk = counts[normal] > 1 && longest <= 4 * counts[normal];
        Some(MeshGrid { idx, spacing, step, counts, normal, bulk })
    }

    /// Cell edge along `d`, the smallest edge where the mesh is one cell
    /// thick.
    fn edge(&self, d: usize) -> f64 {
        if self.step[d] > 0.0 { self.step[d] } else { self.spacing }
    }

    /// In-plane area of one cell, Å².
    pub fn cell_area(&self) -> f64 {
        (0..3).filter(|&d| d != self.normal).map(|d| self.edge(d)).product()
    }

    /// World half-edge vectors of a cell in the film plane, 1% oversize so
    /// neighbouring tiles leave no hairline seam.
    pub fn tile_half_edges(&self) -> [[f64; 3]; 2] {
        let mut axes = [[0.0; 3]; 2];
        for (slot, d) in (0..3).filter(|&d| d != self.normal).enumerate() {
            axes[slot][d] = 0.505 * self.edge(d);
        }
        axes
    }

    /// Arrow pitch that leaves about `n` arrows along the longest in-plane
    /// side.
    pub fn stride_for(&self, n: i64) -> usize {
        let longest = (0..3).filter(|&d| d != self.normal).map(|d| self.counts[d]).max().unwrap_or(1);
        (longest + n - 1).div_euclid(n).max(1) as usize
    }

    /// Whether site `k` carries an arrow at pitch `stride`: every
    /// `stride`-th cell in-plane (and through the thickness of a bulk mesh),
    /// centred so both edges get the same margin.
    pub fn on_pitch(&self, k: usize, stride: usize) -> bool {
        if stride <= 1 {
            return true;
        }
        let s = stride as i64;
        (0..3).all(|d| {
            (!self.bulk && d == self.normal) || (self.idx[k][d] - ((self.counts[d] - 1) % s) / 2).rem_euclid(s) == 0
        })
    }

    /// The layer along the normal that faces a viewer on the `+normal`
    /// side (`towards_plus`) or the other.
    pub fn surface_layer(&self, towards_plus: bool) -> i64 {
        if towards_plus { self.counts[self.normal] - 1 } else { 0 }
    }
}

/// Bits of every coordinate, hashed: a cheap check that a cached grid still
/// belongs to these positions.
pub fn positions_fingerprint(positions: impl Iterator<Item = [f64; 3]>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut n = 0usize;
    for p in positions {
        p.map(f64::to_bits).hash(&mut h);
        n += 1;
    }
    n.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_film_is_recognised_and_thinned_symmetrically() {
        // 9 × 5 × 1 film, 2 Å × 3 Å cells.
        let pos: Vec<[f64; 3]> = (0..45).map(|k| [(k % 9) as f64 * 2.0, (k / 9) as f64 * 3.0, 7.0]).collect();
        let g = MeshGrid::of(&pos).unwrap();
        assert_eq!(g.counts, [9, 5, 1]);
        assert_eq!(g.normal, 2);
        assert!(!g.bulk);
        assert_eq!(g.cell_area(), 6.0);
        let e = g.tile_half_edges();
        assert!((e[0][0] - 1.01).abs() < 1e-12 && (e[1][1] - 1.515).abs() < 1e-12 && e[0][1] == 0.0 && e[1][0] == 0.0);
        // Every 4th of 9 cells: 0, 4, 8 — the same margin on both sides.
        let xs: Vec<i64> = (0..9).filter(|&i| g.on_pitch(i, 4)).map(|i| g.idx[i][0]).collect();
        assert_eq!(xs, [0, 4, 8]);
        assert_eq!(g.surface_layer(true), 0);
    }
}
