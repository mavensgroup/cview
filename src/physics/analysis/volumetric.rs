// src/physics/analysis/volumetric.rs
//
// Quantities on a periodic density grid that the 3D charge-density view
// reports and uses to pick isovalues:
//
// - `trilinear`: periodic interpolation at a fractional coordinate.
// - `log_histogram`: |ρ| over log-spaced bins, for the isovalue histogram.
// - `iso_for_charge_fraction`: the isovalue whose surface encloses a given
//   fraction of the (positive or negative) charge — comparable across
//   systems in a way an absolute isovalue is not.
// - `enclosed`: electrons and volume inside an isosurface.
// - `ambient_occlusion`: per-vertex occlusion of a mesh, from rays marched
//   through the density field itself (inside a lobe = occluded) plus atom
//   spheres. No ray/triangle structure is needed, and it is exact with
//   respect to the surface being shaded.
//
// Densities are in e/Å³ and lattice vectors in Å (rows).

use super::isosurface::Grid;
use crate::utils::task::CancelToken;
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;

/// Which sign of the field a quantity refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Positive,
    Negative,
}

fn lattice_matrix(l: &[[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(l[0][0], l[1][0], l[2][0], l[0][1], l[1][1], l[2][1], l[0][2], l[1][2], l[2][2])
}

pub fn cell_volume(lattice: &[[f64; 3]; 3]) -> f64 {
    lattice_matrix(lattice).determinant().abs()
}

/// Trilinear interpolation at fractional coordinate `f` (periodic).
pub fn trilinear(grid: &Grid, f: Vector3<f64>) -> f64 {
    let n = grid.dims;
    let mut i0 = [0usize; 3];
    let mut i1 = [0usize; 3];
    let mut t = [0f64; 3];
    for a in 0..3 {
        let u = f[a] * n[a] as f64;
        let fl = u.floor();
        t[a] = u - fl;
        let i = (fl as i64).rem_euclid(n[a] as i64) as usize;
        i0[a] = i;
        i1[a] = (i + 1) % n[a];
    }
    let at = |x: usize, y: usize, z: usize| grid.data[x + n[0] * (y + n[1] * z)] as f64;
    let (tx, ty, tz) = (t[0], t[1], t[2]);
    let c00 = at(i0[0], i0[1], i0[2]) * (1.0 - tx) + at(i1[0], i0[1], i0[2]) * tx;
    let c10 = at(i0[0], i1[1], i0[2]) * (1.0 - tx) + at(i1[0], i1[1], i0[2]) * tx;
    let c01 = at(i0[0], i0[1], i1[2]) * (1.0 - tx) + at(i1[0], i0[1], i1[2]) * tx;
    let c11 = at(i0[0], i1[1], i1[2]) * (1.0 - tx) + at(i1[0], i1[1], i1[2]) * tx;
    let c0 = c00 * (1.0 - ty) + c10 * ty;
    let c1 = c01 * (1.0 - ty) + c11 * ty;
    c0 * (1.0 - tz) + c1 * tz
}

/// Counts of |ρ| in `bins` log-spaced bins over [lo, hi]. Values outside
/// the range are not counted.
pub fn log_histogram(data: &[f32], lo: f64, hi: f64, bins: usize) -> Vec<u32> {
    let mut h = vec![0u32; bins.max(1)];
    if !(lo > 0.0 && hi > lo) {
        return h;
    }
    let (l0, span) = (lo.ln(), (hi / lo).ln());
    for &v in data {
        // f32 data sits a rounding error off the f64 range ends.
        let a = (v as f64).abs();
        let a = if a < lo && a >= lo * (1.0 - 1e-6) { lo } else if a > hi && a <= hi * (1.0 + 1e-6) { hi } else { a };
        if a >= lo && a <= hi {
            let b = (((a.ln() - l0) / span) * bins as f64) as usize;
            h[b.min(bins - 1)] += 1;
        }
    }
    h
}

/// Isovalue (> 0) whose surface encloses `fraction` of the charge of the
/// given sign: Σ_{±ρ > iso} ±ρ = fraction × Σ_{±ρ > 0} ±ρ.
///
/// Uses a charge-weighted histogram with 16384 log bins over the full
/// range, interpolated inside the crossing bin: ~0.1% in the isovalue at
/// any grid size, in one pass.
pub fn iso_for_charge_fraction(data: &[f32], fraction: f64, sign: Sign) -> Option<f64> {
    const BINS: usize = 16384;
    let s = if sign == Sign::Positive { 1.0 } else { -1.0 };
    let vals = || data.iter().map(move |&v| v as f64 * s).filter(|&v| v > 0.0);
    let hi = vals().fold(0.0, f64::max);
    if hi <= 0.0 {
        return None;
    }
    let lo = (hi * 1e-9).max(vals().fold(f64::INFINITY, f64::min));
    let (l0, span) = (lo.ln(), (hi / lo).ln().max(1e-12));
    let mut w = vec![0f64; BINS];
    let mut total = 0.0;
    for v in vals() {
        total += v;
        if v >= lo {
            let b = (((v.ln() - l0) / span) * BINS as f64) as usize;
            w[b.min(BINS - 1)] += v;
        }
    }
    let target = fraction.clamp(0.0, 1.0) * total;
    let mut acc = 0.0;
    for b in (0..BINS).rev() {
        if acc + w[b] >= target {
            // Charge in this bin is spread evenly in log space.
            let need = if w[b] > 0.0 { (target - acc) / w[b] } else { 0.0 };
            let edge = |k: f64| (l0 + span * k / BINS as f64).exp();
            let upper = edge((b + 1) as f64);
            let lower = edge(b as f64);
            return Some(upper * (lower / upper).powf(need));
        }
        acc += w[b];
    }
    Some(lo)
}

/// Charge and volume inside an isosurface.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Enclosed {
    /// Electrons (|charge|) inside the surface.
    pub electrons: f64,
    /// Å³.
    pub volume: f64,
    /// Of the cell volume.
    pub volume_fraction: f64,
    /// Of all charge of the same sign.
    pub charge_fraction: f64,
}

/// Electrons and volume where ±ρ > iso (voxel sum; exact in the grid limit).
pub fn enclosed(data: &[f32], lattice: &[[f64; 3]; 3], iso: f64, sign: Sign) -> Enclosed {
    let s = if sign == Sign::Positive { 1.0 } else { -1.0 };
    let n = data.len().max(1) as f64;
    let dv = cell_volume(lattice) / n;
    let (mut q_in, mut q_all, mut count) = (0.0, 0.0, 0usize);
    for &v in data {
        let v = v as f64 * s;
        if v > 0.0 {
            q_all += v;
            if v > iso {
                q_in += v;
                count += 1;
            }
        }
    }
    Enclosed {
        electrons: q_in * dv,
        volume: count as f64 * dv,
        volume_fraction: count as f64 / n,
        charge_fraction: if q_all > 0.0 { q_in / q_all } else { 0.0 },
    }
}

/// Unit directions spread evenly over a sphere (Fibonacci lattice).
fn sphere_directions(n: usize) -> Vec<Vector3<f64>> {
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
            let r = (1.0 - y * y).sqrt();
            let phi = golden * i as f64;
            Vector3::new(r * phi.cos(), y, r * phi.sin())
        })
        .collect()
}

/// What blocks ambient light: lobes of the density (ρ > `iso_pos` or
/// ρ < −`iso_neg`, either optional) and spheres (centre, radius), Å.
pub struct Occluders<'a> {
    pub iso_pos: Option<f64>,
    pub iso_neg: Option<f64>,
    pub spheres: &'a [([f64; 3], f64)],
}

/// Ambient occlusion per vertex, 1 = fully open, 0 = buried. `reach` is how
/// far (Å) occluders still count; nearer ones count more. Returns `None` if
/// cancelled.
pub fn ambient_occlusion(
    grid: &Grid,
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    occ: &Occluders,
    reach: f64,
    rays: usize,
    cancel: &CancelToken,
) -> Option<Vec<f32>> {
    let m = lattice_matrix(&grid.lattice);
    let inv = m.try_inverse()?;
    // Twice the rays over the full sphere; each vertex keeps its hemisphere.
    let dirs = sphere_directions(rays * 2);
    const STEPS: usize = 6;
    let start = 0.12_f64.min(reach * 0.1);
    let blocked = |p: Vector3<f64>| -> bool {
        if occ.iso_pos.is_some() || occ.iso_neg.is_some() {
            let v = trilinear(grid, inv * p);
            if occ.iso_pos.is_some_and(|i| v > i) || occ.iso_neg.is_some_and(|i| v < -i) {
                return true;
            }
        }
        occ.spheres.iter().any(|(c, r)| {
            let d = Vector3::new(p.x - c[0], p.y - c[1], p.z - c[2]);
            d.norm_squared() < r * r
        })
    };
    let chunk = 4096;
    let out: Vec<Option<Vec<f32>>> = positions
        .par_chunks(chunk)
        .zip(normals.par_chunks(chunk))
        .map(|(ps, ns)| {
            if cancel.is_cancelled() {
                return None;
            }
            Some(
                ps.iter()
                    .zip(ns)
                    .map(|(p, n)| {
                        let p = Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64);
                        let n = Vector3::new(n[0] as f64, n[1] as f64, n[2] as f64);
                        let (mut occl, mut weight) = (0.0, 0.0);
                        for d in &dirs {
                            let c = d.dot(&n);
                            if c <= 0.05 {
                                continue;
                            }
                            // Cosine-weighted: rays near the normal matter most.
                            weight += c;
                            for s in 0..STEPS {
                                let t = start + (reach - start) * (s as f64 + 0.5) / STEPS as f64;
                                if blocked(p + d * t) {
                                    occl += c * (1.0 - t / reach);
                                    break;
                                }
                            }
                        }
                        if weight > 0.0 {
                            (1.0 - occl / weight) as f32
                        } else {
                            1.0
                        }
                    })
                    .collect(),
            )
        })
        .collect();
    let mut ao = Vec::with_capacity(positions.len());
    for c in out {
        ao.extend(c?);
    }
    Some(ao)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(a: f64) -> [[f64; 3]; 3] {
        [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]]
    }

    fn sample(n: usize, a: f64, f: impl Fn(Vector3<f64>) -> f64) -> Vec<f32> {
        let mut out = vec![0f32; n * n * n];
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let p = Vector3::new(i as f64, j as f64, k as f64) * (a / n as f64);
                    out[i + n * (j + n * k)] = f(p) as f32;
                }
            }
        }
        out
    }

    #[test]
    fn trilinear_is_exact_on_linear_fields_and_periodic() {
        let a = 4.0;
        let n = 8;
        let data = sample(n, a, |p| p.x + 2.0 * p.y);
        let g = Grid { data: &data, dims: [n; 3], lattice: cube(a) };
        let v = trilinear(&g, Vector3::new(0.3, 0.2, 0.7));
        assert!((v - (1.2 + 2.0 * 0.8)).abs() < 1e-5, "{v}");
        // Grid point values repeat with the lattice.
        let w = trilinear(&g, Vector3::new(1.25, -0.75, 2.0));
        assert!((w - (1.0 + 2.0 * 1.0)).abs() < 1e-5, "{w}");
    }

    #[test]
    fn charge_fraction_round_trips_with_enclosed() {
        // A Gaussian "atom": enclosing half its charge has a definite iso.
        let a = 10.0;
        let c = Vector3::new(5.0, 5.0, 5.0);
        let data = sample(40, a, |p| (-(p - c).norm_squared() / 2.0).exp());
        for frac in [0.25, 0.5, 0.9] {
            let iso = iso_for_charge_fraction(&data, frac, Sign::Positive).unwrap();
            let e = enclosed(&data, &cube(a), iso, Sign::Positive);
            assert!((e.charge_fraction - frac).abs() < 0.01, "frac {frac}: got {}", e.charge_fraction);
        }
        // Total electrons: ∫ exp(-r²/2) d³r = (2π)^{3/2}.
        let all = enclosed(&data, &cube(a), 0.0, Sign::Positive);
        let exact = (2.0 * std::f64::consts::PI).powf(1.5);
        assert!((all.electrons - exact).abs() / exact < 0.01, "{} vs {exact}", all.electrons);
    }

    #[test]
    fn negative_side_mirrors_positive() {
        let data: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.37).sin()).collect();
        let neg: Vec<f32> = data.iter().map(|v| -v).collect();
        let a = iso_for_charge_fraction(&data, 0.5, Sign::Positive).unwrap();
        let b = iso_for_charge_fraction(&neg, 0.5, Sign::Negative).unwrap();
        assert!((a - b).abs() < 1e-12);
    }

    #[test]
    fn histogram_counts_magnitudes() {
        let h = log_histogram(&[0.01, -0.01, 0.1, 1.0, 5.0], 0.01, 1.0, 2);
        assert_eq!(h, vec![2, 2]); // 0.01, 0.01 | 0.1, 1.0 (5.0 out of range)
    }

    #[test]
    fn occlusion_is_high_in_a_crevice_and_low_on_an_open_face() {
        // Field: a slab (ρ > iso for |z - 5| < 1) in a 10 Å cell.
        let a = 10.0;
        let data = sample(40, a, |p| 1.0 - (p.z - 5.0).abs());
        let g = Grid { data: &data, dims: [40; 3], lattice: cube(a) };
        let occ = Occluders { iso_pos: Some(0.0), iso_neg: None, spheres: &[] };
        // A point on the slab's upper face looking up is open...
        let open = ambient_occlusion(&g, &[[5.0, 5.0, 6.0]], &[[0.0, 0.0, 1.0]], &occ, 2.0, 32, &CancelToken::never()).unwrap();
        assert!(open[0] > 0.95, "{}", open[0]);
        // ...a point just above it with a sphere sitting right over it is not.
        let s = [([5.0, 5.0, 6.8], 0.6)];
        let occ = Occluders { iso_pos: Some(0.0), iso_neg: None, spheres: &s };
        let shut = ambient_occlusion(&g, &[[5.0, 5.0, 6.0]], &[[0.0, 0.0, 1.0]], &occ, 2.0, 32, &CancelToken::never()).unwrap();
        assert!(shut[0] < open[0] - 0.2, "{} vs {}", shut[0], open[0]);
    }
}
