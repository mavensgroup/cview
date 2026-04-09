// src/physics/analysis/charge_density.rs
// 2D slice extraction + marching squares isoline algorithm
// Zero-copy for Total channel via Cow; atom projection for overlays

use crate::io::chgcar::ChgcarData;
use nalgebra::{Matrix3, Vector3};
use std::borrow::Cow;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which density channel to visualize
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DensityChannel {
    Total,
    SpinUp,
    SpinDown,
    Magnetization,
}

impl DensityChannel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Total => "Total",
            Self::SpinUp => "Spin-up",
            Self::SpinDown => "Spin-down",
            Self::Magnetization => "Magnetization",
        }
    }

    pub fn is_diverging(&self) -> bool {
        *self == Self::Magnetization
    }
}

/// Which crystallographic plane to slice along
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SlicePlane {
    /// Slice perpendicular to c-axis (constant fractional z)
    XY,
    /// Slice perpendicular to b-axis (constant fractional y)
    XZ,
    /// Slice perpendicular to a-axis (constant fractional x)
    YZ,
}

/// A 2D slice of charge density ready for rendering
#[derive(Clone, Debug)]
pub struct DensitySlice {
    /// The 2D grid data in row-major order [row][col]
    pub data: Vec<Vec<f64>>,
    /// Number of rows (first axis of the slice)
    pub n_rows: usize,
    /// Number of columns (second axis of the slice)
    pub n_cols: usize,
    /// Minimum value in this slice
    pub data_min: f64,
    /// Maximum value in this slice
    pub data_max: f64,
    /// Which plane this slice came from
    pub plane: SlicePlane,
    /// Fractional position of the slice along the perpendicular axis
    pub position: f64,
    /// Label describing the axes (e.g. "a (Å)" / "b (Å)")
    pub x_label: String,
    /// Label for the vertical axis
    pub y_label: String,
    /// Physical extent in Å: (x_range, y_range) for axis ticks
    pub x_extent_ang: f64,
    pub y_extent_ang: f64,
    /// Plane description for annotation (e.g. "(001) z=0.50")
    pub plane_annotation: String,
    /// Cell boundary polygon in normalised [0,1] plot coordinates.
    /// Non-empty only for HKL slices — used to clip the heatmap to the
    /// actual plane–cell intersection shape (triangle, hexagon, etc.).
    pub cell_boundary_uv: Vec<(f64, f64)>,
}

/// A projected atom position for overlay on a 2D slice
#[derive(Clone, Debug)]
pub struct ProjectedAtom {
    /// Normalised position [0, 1] in the slice coordinate system
    pub x: f64,
    pub y: f64,
    /// Element symbol
    pub element: String,
    /// Distance from the slice plane in Å (for filtering/transparency)
    pub distance_ang: f64,
}

/// A single isoline (constant-density contour line)
#[derive(Clone, Debug)]
pub struct Isoline {
    /// Threshold value (charge density at this contour)
    pub threshold: f64,
    /// Line segments: pairs of (x1,y1) → (x2,y2) in normalised [0,1] coordinates
    pub segments: Vec<((f64, f64), (f64, f64))>,
}

/// Threshold spacing mode
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ThresholdMode {
    Linear,
    Logarithmic,
}

// ---------------------------------------------------------------------------
// Channel data selection — zero-copy for Total via Cow
// ---------------------------------------------------------------------------

/// Get the density data for a given channel.
/// Returns `Cow::Borrowed` for Total (zero-copy), `Cow::Owned` for derived channels.
fn get_channel_data<'a>(
    chgcar: &'a ChgcarData,
    channel: DensityChannel,
    normalize: bool,
) -> Option<Cow<'a, [f64]>> {
    match channel {
        DensityChannel::Total => {
            if normalize {
                Some(Cow::Owned(chgcar.normalized_total()))
            } else {
                Some(Cow::Borrowed(&chgcar.charge_total))
            }
        }
        DensityChannel::Magnetization => {
            if normalize {
                chgcar.normalized_mag().map(Cow::Owned)
            } else {
                chgcar.charge_mag.as_deref().map(Cow::Borrowed)
            }
        }
        DensityChannel::SpinUp => {
            if normalize {
                let vol = chgcar.cell_volume();
                chgcar.charge_up().map(|d| {
                    if vol.abs() > 1e-30 {
                        Cow::Owned(d.iter().map(|&v| v / vol).collect())
                    } else {
                        Cow::Owned(d)
                    }
                })
            } else {
                chgcar.charge_up().map(Cow::Owned)
            }
        }
        DensityChannel::SpinDown => {
            if normalize {
                let vol = chgcar.cell_volume();
                chgcar.charge_down().map(|d| {
                    if vol.abs() > 1e-30 {
                        Cow::Owned(d.iter().map(|&v| v / vol).collect())
                    } else {
                        Cow::Owned(d)
                    }
                })
            } else {
                chgcar.charge_down().map(Cow::Owned)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lattice helpers — use nalgebra consistently
// ---------------------------------------------------------------------------

fn lattice_to_mat3(lat: &[[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    )
}

fn lattice_vec(lat: &[[f64; 3]; 3], i: usize) -> Vector3<f64> {
    Vector3::new(lat[i][0], lat[i][1], lat[i][2])
}

/// Compute physical length of a lattice vector
fn lattice_length(lat: &[[f64; 3]; 3], i: usize) -> f64 {
    lattice_vec(lat, i).norm()
}

// ---------------------------------------------------------------------------
// Fractional axis slice extraction
// ---------------------------------------------------------------------------

/// Extract a 2D density slice from volumetric data.
pub fn extract_slice(
    chgcar: &ChgcarData,
    channel: DensityChannel,
    plane: SlicePlane,
    position: f64,
    normalize: bool,
) -> Option<DensitySlice> {
    let density = get_channel_data(chgcar, channel, normalize)?;
    let [nx, ny, nz] = chgcar.grid;
    let lat = &chgcar.lattice;

    let position = position.clamp(0.0, 1.0 - 1e-9);

    let (n_rows, n_cols, data, x_label, y_label, x_ext, y_ext, annotation) = match plane {
        SlicePlane::XY => {
            let iz = ((position * nz as f64) as usize).min(nz - 1);
            let mut grid = vec![vec![0.0f64; nx]; ny];
            for iy in 0..ny {
                for ix in 0..nx {
                    grid[iy][ix] = density[chgcar.index(ix, iy, iz)];
                }
            }
            let ann = format!("(001) z = {:.3}", position);
            (
                ny,
                nx,
                grid,
                "a (Å)".to_string(),
                "b (Å)".to_string(),
                lattice_length(lat, 0),
                lattice_length(lat, 1),
                ann,
            )
        }
        SlicePlane::XZ => {
            let iy = ((position * ny as f64) as usize).min(ny - 1);
            let mut grid = vec![vec![0.0f64; nx]; nz];
            for iz in 0..nz {
                for ix in 0..nx {
                    grid[iz][ix] = density[chgcar.index(ix, iy, iz)];
                }
            }
            let ann = format!("(010) y = {:.3}", position);
            (
                nz,
                nx,
                grid,
                "a (Å)".to_string(),
                "c (Å)".to_string(),
                lattice_length(lat, 0),
                lattice_length(lat, 2),
                ann,
            )
        }
        SlicePlane::YZ => {
            let ix = ((position * nx as f64) as usize).min(nx - 1);
            let mut grid = vec![vec![0.0f64; ny]; nz];
            for iz in 0..nz {
                for iy in 0..ny {
                    grid[iz][iy] = density[chgcar.index(ix, iy, iz)];
                }
            }
            let ann = format!("(100) x = {:.3}", position);
            (
                nz,
                ny,
                grid,
                "b (Å)".to_string(),
                "c (Å)".to_string(),
                lattice_length(lat, 1),
                lattice_length(lat, 2),
                ann,
            )
        }
    };

    let (data_min, data_max) = compute_min_max(&data);

    Some(DensitySlice {
        data,
        n_rows,
        n_cols,
        data_min,
        data_max,
        plane,
        position,
        x_label,
        y_label,
        x_extent_ang: x_ext,
        y_extent_ang: y_ext,
        plane_annotation: annotation,
        cell_boundary_uv: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Atom projection onto a fractional-axis slice
// ---------------------------------------------------------------------------

/// Project atoms onto a fractional-axis slice, returning those within `tolerance`
/// (in fractional units along the perpendicular axis) of the slice position.
pub fn project_atoms_fractional(
    chgcar: &ChgcarData,
    plane: SlicePlane,
    position: f64,
    tolerance: f64,
) -> Vec<ProjectedAtom> {
    let lat = &chgcar.lattice;
    let perp_length = match plane {
        SlicePlane::XY => lattice_length(lat, 2),
        SlicePlane::XZ => lattice_length(lat, 1),
        SlicePlane::YZ => lattice_length(lat, 0),
    };

    chgcar
        .atoms
        .iter()
        .filter_map(|atom| {
            let f = atom.frac_coords;
            let (perp_frac, x_frac, y_frac) = match plane {
                SlicePlane::XY => (f[2], f[0], f[1]),
                SlicePlane::XZ => (f[1], f[0], f[2]),
                SlicePlane::YZ => (f[0], f[1], f[2]),
            };

            // Minimum-image distance in fractional units along perpendicular
            let mut df = (perp_frac - position).rem_euclid(1.0);
            if df > 0.5 {
                df = 1.0 - df;
            }

            if df <= tolerance {
                Some(ProjectedAtom {
                    x: x_frac.rem_euclid(1.0),
                    y: y_frac.rem_euclid(1.0),
                    element: atom.element.clone(),
                    distance_ang: df * perp_length,
                })
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// HKL plane slice — aspect-aware grid
// ---------------------------------------------------------------------------

/// Extract a 2D density slice through an arbitrary (h k l) Miller plane.
///
/// `offset` ∈ [0,1] sweeps the plane along its normal across the unit cell.
/// Uses aspect-aware grid dimensions proportional to the in-plane bounding box.
pub fn extract_slice_hkl(
    chgcar: &ChgcarData,
    channel: DensityChannel,
    hkl: [i32; 3],
    offset: f64,
    normalize: bool,
) -> Option<DensitySlice> {
    let density = get_channel_data(chgcar, channel, normalize)?;
    let [nx, ny, nz] = chgcar.grid;
    let lat = &chgcar.lattice;

    let lat_mat = lattice_to_mat3(lat);
    let inv_lat = lat_mat.try_inverse()?;

    // Reciprocal lattice vectors = columns of inv_lat
    let a_star = Vector3::new(inv_lat[(0, 0)], inv_lat[(1, 0)], inv_lat[(2, 0)]);
    let b_star = Vector3::new(inv_lat[(0, 1)], inv_lat[(1, 1)], inv_lat[(2, 1)]);
    let c_star = Vector3::new(inv_lat[(0, 2)], inv_lat[(1, 2)], inv_lat[(2, 2)]);

    let h = hkl[0] as f64;
    let k = hkl[1] as f64;
    let l = hkl[2] as f64;
    if h == 0.0 && k == 0.0 && l == 0.0 {
        return None;
    }

    // Plane normal in Cartesian space
    let n_raw = h * a_star + k * b_star + l * c_star;
    let n = n_raw.normalize();

    // Two orthonormal in-plane vectors
    let seed = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if n.y.abs() <= n.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let u = n.cross(&seed).normalize();
    let v = n.cross(&u);

    // Plane origin: sweep across cell along n
    let a_vec = lattice_vec(lat, 0);
    let b_vec = lattice_vec(lat, 1);
    let c_vec = lattice_vec(lat, 2);
    let cell_centre = (a_vec + b_vec + c_vec) * 0.5;

    let corners = [
        Vector3::zeros(),
        a_vec,
        b_vec,
        c_vec,
        a_vec + b_vec,
        a_vec + c_vec,
        b_vec + c_vec,
        a_vec + b_vec + c_vec,
    ];
    let projs: Vec<f64> = corners.iter().map(|p| p.dot(&n)).collect();
    let pmin = projs.iter().cloned().fold(f64::INFINITY, f64::min);
    let pmax = projs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let plane_d = pmin + offset.clamp(0.0, 1.0) * (pmax - pmin);
    let origin = cell_centre + n * (plane_d - cell_centre.dot(&n));

    // In-plane bounding box
    let uv: Vec<(f64, f64)> = corners.iter().map(|p| (p.dot(&u), p.dot(&v))).collect();
    let umin = uv.iter().map(|&(x, _)| x).fold(f64::INFINITY, f64::min);
    let umax = uv.iter().map(|&(x, _)| x).fold(f64::NEG_INFINITY, f64::max);
    let vmin = uv.iter().map(|&(_, y)| y).fold(f64::INFINITY, f64::min);
    let vmax = uv.iter().map(|&(_, y)| y).fold(f64::NEG_INFINITY, f64::max);

    let u_range = umax - umin;
    let v_range = vmax - vmin;

    // Aspect-aware grid: scale dimensions proportionally to physical extents
    let base_samples = nx.max(ny).max(nz).max(64).min(512);
    let max_dim = u_range.max(v_range);
    let (n_cols, n_rows) = if max_dim.abs() < 1e-12 {
        (base_samples, base_samples)
    } else {
        let nc = ((u_range / max_dim) * base_samples as f64)
            .round()
            .max(32.0) as usize;
        let nr = ((v_range / max_dim) * base_samples as f64)
            .round()
            .max(32.0) as usize;
        (nc.min(512), nr.min(512))
    };

    let mut grid = vec![vec![0.0f64; n_cols]; n_rows];

    for row in 0..n_rows {
        let vf = vmin + (row as f64 / (n_rows - 1).max(1) as f64) * v_range;
        for col in 0..n_cols {
            let uf = umin + (col as f64 / (n_cols - 1).max(1) as f64) * u_range;
            let cart = origin + u * uf + v * vf;
            // Cartesian → fractional with PBC
            let frac = inv_lat * cart;
            let fx = frac.x.rem_euclid(1.0);
            let fy = frac.y.rem_euclid(1.0);
            let fz = frac.z.rem_euclid(1.0);
            grid[row][col] = trilinear(&density, nx, ny, nz, fx, fy, fz, chgcar);
        }
    }

    let (data_min, data_max) = compute_min_max(&grid);
    let annotation = format!("({}{}{}) offset = {:.3}", hkl[0], hkl[1], hkl[2], offset);

    // ── Cell boundary polygon: intersect the plane with the 12 cell edges ──
    let cell_boundary_uv = {
        let cell_edges: [(usize, usize); 12] = [
            (0, 1),
            (0, 2),
            (0, 3), // from origin
            (1, 4),
            (1, 5), // from a
            (2, 4),
            (2, 6), // from b
            (3, 5),
            (3, 6), // from c
            (4, 7),
            (5, 7),
            (6, 7), // to a+b+c
        ];
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for &(i0, i1) in &cell_edges {
            let p0 = corners[i0];
            let p1 = corners[i1];
            let d = p1 - p0;
            let dn = d.dot(&n);
            if dn.abs() < 1e-12 {
                continue;
            }
            let t = (plane_d - p0.dot(&n)) / dn;
            if t >= -1e-6 && t <= 1.0 + 1e-6 {
                let pt = p0 + d * t.clamp(0.0, 1.0);
                let pu = (pt.dot(&u) - umin) / u_range;
                let pv = (pt.dot(&v) - vmin) / v_range;
                pts.push((pu, pv));
            }
        }
        // Remove near-duplicates
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4);
        // Sort angularly around centroid
        if pts.len() >= 3 {
            let n_pts = pts.len() as f64;
            let cx = pts.iter().map(|p| p.0).sum::<f64>() / n_pts;
            let cy = pts.iter().map(|p| p.1).sum::<f64>() / n_pts;
            pts.sort_by(|a, b| {
                let aa = (a.1 - cy).atan2(a.0 - cx);
                let ab = (b.1 - cy).atan2(b.0 - cx);
                aa.partial_cmp(&ab).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        pts
    };

    Some(DensitySlice {
        data: grid,
        n_rows,
        n_cols,
        data_min,
        data_max,
        plane: SlicePlane::XY, // placeholder for HKL
        position: offset,
        x_label: "u (Å)".to_string(),
        y_label: "v (Å)".to_string(),
        x_extent_ang: u_range,
        y_extent_ang: v_range,
        plane_annotation: annotation,
        cell_boundary_uv,
    })
}

/// Project atoms onto an arbitrary HKL plane.
/// Returns atoms within `tolerance_ang` Å of the plane.
pub fn project_atoms_hkl(
    chgcar: &ChgcarData,
    hkl: [i32; 3],
    offset: f64,
    tolerance_ang: f64,
) -> Vec<ProjectedAtom> {
    let lat = &chgcar.lattice;
    let lat_mat = lattice_to_mat3(lat);
    let inv_lat = match lat_mat.try_inverse() {
        Some(m) => m,
        None => return Vec::new(),
    };

    let a_star = Vector3::new(inv_lat[(0, 0)], inv_lat[(1, 0)], inv_lat[(2, 0)]);
    let b_star = Vector3::new(inv_lat[(0, 1)], inv_lat[(1, 1)], inv_lat[(2, 1)]);
    let c_star = Vector3::new(inv_lat[(0, 2)], inv_lat[(1, 2)], inv_lat[(2, 2)]);

    let h = hkl[0] as f64;
    let k = hkl[1] as f64;
    let l = hkl[2] as f64;
    if h == 0.0 && k == 0.0 && l == 0.0 {
        return Vec::new();
    }

    let n_raw = h * a_star + k * b_star + l * c_star;
    let n = n_raw.normalize();

    let seed = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if n.y.abs() <= n.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let u = n.cross(&seed).normalize();
    let v = n.cross(&u);

    let a_vec = lattice_vec(lat, 0);
    let b_vec = lattice_vec(lat, 1);
    let c_vec = lattice_vec(lat, 2);

    let corners = [
        Vector3::zeros(),
        a_vec,
        b_vec,
        c_vec,
        a_vec + b_vec,
        a_vec + c_vec,
        b_vec + c_vec,
        a_vec + b_vec + c_vec,
    ];
    let projs: Vec<f64> = corners.iter().map(|p| p.dot(&n)).collect();
    let pmin = projs.iter().cloned().fold(f64::INFINITY, f64::min);
    let pmax = projs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let plane_d = pmin + offset.clamp(0.0, 1.0) * (pmax - pmin);

    // In-plane bounding box for normalisation
    let uv: Vec<(f64, f64)> = corners.iter().map(|p| (p.dot(&u), p.dot(&v))).collect();
    let umin = uv.iter().map(|&(x, _)| x).fold(f64::INFINITY, f64::min);
    let umax = uv.iter().map(|&(x, _)| x).fold(f64::NEG_INFINITY, f64::max);
    let vmin = uv.iter().map(|&(_, y)| y).fold(f64::INFINITY, f64::min);
    let vmax = uv.iter().map(|&(_, y)| y).fold(f64::NEG_INFINITY, f64::max);
    let u_range = umax - umin;
    let v_range = vmax - vmin;

    chgcar
        .atoms
        .iter()
        .filter_map(|atom| {
            let f = atom.frac_coords;
            let cart = lat_mat * Vector3::new(f[0], f[1], f[2]);
            let dist = (cart.dot(&n) - plane_d).abs();
            if dist <= tolerance_ang {
                let pu = cart.dot(&u);
                let pv = cart.dot(&v);
                let x_norm = if u_range.abs() > 1e-12 {
                    (pu - umin) / u_range
                } else {
                    0.5
                };
                let y_norm = if v_range.abs() > 1e-12 {
                    (pv - vmin) / v_range
                } else {
                    0.5
                };
                Some(ProjectedAtom {
                    x: x_norm,
                    y: y_norm,
                    element: atom.element.clone(),
                    distance_ang: dist,
                })
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Marching squares isoline extraction
// ---------------------------------------------------------------------------

/// Extract isolines at the given thresholds from a 2D density slice.
pub fn extract_isolines(slice: &DensitySlice, thresholds: &[f64]) -> Vec<Isoline> {
    thresholds
        .iter()
        .map(|&t| Isoline {
            threshold: t,
            segments: marching_squares(&slice.data, slice.n_rows, slice.n_cols, t),
        })
        .collect()
}

/// Generate a set of evenly-spaced threshold values across the data range.
pub fn auto_thresholds(slice: &DensitySlice, n_levels: usize, mode: ThresholdMode) -> Vec<f64> {
    if n_levels == 0 || slice.data_min >= slice.data_max {
        return Vec::new();
    }

    match mode {
        ThresholdMode::Linear => {
            let step = (slice.data_max - slice.data_min) / (n_levels + 1) as f64;
            (1..=n_levels)
                .map(|i| slice.data_min + step * i as f64)
                .collect()
        }
        ThresholdMode::Logarithmic => {
            // For log spacing, handle cases where data includes zero or negative values
            let lo = if slice.data_min > 0.0 {
                slice.data_min
            } else {
                // Find a small positive floor based on max value
                slice.data_max * 1e-6
            };
            let hi = slice.data_max;
            if lo >= hi || lo <= 0.0 {
                // Fall back to linear if log is not meaningful
                let step = (slice.data_max - slice.data_min) / (n_levels + 1) as f64;
                return (1..=n_levels)
                    .map(|i| slice.data_min + step * i as f64)
                    .collect();
            }
            let log_lo = lo.ln();
            let log_hi = hi.ln();
            let step = (log_hi - log_lo) / (n_levels + 1) as f64;
            (1..=n_levels)
                .map(|i| (log_lo + step * i as f64).exp())
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Marching squares implementation
// ---------------------------------------------------------------------------

fn marching_squares(
    data: &[Vec<f64>],
    n_rows: usize,
    n_cols: usize,
    threshold: f64,
) -> Vec<((f64, f64), (f64, f64))> {
    if n_rows < 2 || n_cols < 2 {
        return Vec::new();
    }

    let mut segments = Vec::new();

    let cell_w = 1.0 / (n_cols - 1) as f64;
    let cell_h = 1.0 / (n_rows - 1) as f64;

    for row in 0..(n_rows - 1) {
        for col in 0..(n_cols - 1) {
            let v00 = data[row][col];
            let v01 = data[row][col + 1];
            let v10 = data[row + 1][col];
            let v11 = data[row + 1][col + 1];

            let mut case = 0u8;
            if v00 > threshold {
                case |= 1;
            }
            if v01 > threshold {
                case |= 2;
            }
            if v11 > threshold {
                case |= 4;
            }
            if v10 > threshold {
                case |= 8;
            }

            if case == 0 || case == 15 {
                continue;
            }

            let x_left = col as f64 * cell_w;
            let x_right = (col + 1) as f64 * cell_w;
            let y_top = row as f64 * cell_h;
            let y_bottom = (row + 1) as f64 * cell_h;

            let lerp_x_top = lerp_x(x_left, x_right, v00, v01, threshold);
            let lerp_x_bottom = lerp_x(x_left, x_right, v10, v11, threshold);
            let lerp_y_left = lerp_y(y_top, y_bottom, v00, v10, threshold);
            let lerp_y_right = lerp_y(y_top, y_bottom, v01, v11, threshold);

            let top = (lerp_x_top, y_top);
            let bottom = (lerp_x_bottom, y_bottom);
            let left = (x_left, lerp_y_left);
            let right = (x_right, lerp_y_right);

            match case {
                1 | 14 => segments.push((top, left)),
                2 | 13 => segments.push((top, right)),
                3 | 12 => segments.push((left, right)),
                4 | 11 => segments.push((bottom, right)),
                6 | 9 => segments.push((top, bottom)),
                7 | 8 => segments.push((bottom, left)),
                // Saddle cases — disambiguate by average at cell centre
                5 => {
                    let avg = (v00 + v01 + v10 + v11) * 0.25;
                    if avg > threshold {
                        segments.push((top, right));
                        segments.push((bottom, left));
                    } else {
                        segments.push((top, left));
                        segments.push((bottom, right));
                    }
                }
                10 => {
                    let avg = (v00 + v01 + v10 + v11) * 0.25;
                    if avg > threshold {
                        segments.push((top, left));
                        segments.push((bottom, right));
                    } else {
                        segments.push((top, right));
                        segments.push((bottom, left));
                    }
                }
                _ => {}
            }
        }
    }

    segments
}

fn lerp_x(x0: f64, x1: f64, v0: f64, v1: f64, t: f64) -> f64 {
    let dv = v1 - v0;
    if dv.abs() < 1e-12 {
        return (x0 + x1) * 0.5;
    }
    x0 + (t - v0) / dv * (x1 - x0)
}

fn lerp_y(y0: f64, y1: f64, v0: f64, v1: f64, t: f64) -> f64 {
    let dv = v1 - v0;
    if dv.abs() < 1e-12 {
        return (y0 + y1) * 0.5;
    }
    y0 + (t - v0) / dv * (y1 - y0)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compute_min_max(data: &[Vec<f64>]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for row in data {
        for &v in row {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }
    }
    if !min.is_finite() {
        min = 0.0;
    }
    if !max.is_finite() {
        max = 1.0;
    }
    (min, max)
}

/// Trilinear interpolation of 3D volumetric data with PBC
fn trilinear(
    density: &[f64],
    nx: usize,
    ny: usize,
    nz: usize,
    fx: f64,
    fy: f64,
    fz: f64,
    chgcar: &ChgcarData,
) -> f64 {
    let x = fx * nx as f64;
    let y = fy * ny as f64;
    let z = fz * nz as f64;
    let ix0 = (x.floor() as usize) % nx;
    let iy0 = (y.floor() as usize) % ny;
    let iz0 = (z.floor() as usize) % nz;
    let ix1 = (ix0 + 1) % nx;
    let iy1 = (iy0 + 1) % ny;
    let iz1 = (iz0 + 1) % nz;
    let tx = x - x.floor();
    let ty = y - y.floor();
    let tz = z - z.floor();

    let c000 = density[chgcar.index(ix0, iy0, iz0)];
    let c100 = density[chgcar.index(ix1, iy0, iz0)];
    let c010 = density[chgcar.index(ix0, iy1, iz0)];
    let c110 = density[chgcar.index(ix1, iy1, iz0)];
    let c001 = density[chgcar.index(ix0, iy0, iz1)];
    let c101 = density[chgcar.index(ix1, iy0, iz1)];
    let c011 = density[chgcar.index(ix0, iy1, iz1)];
    let c111 = density[chgcar.index(ix1, iy1, iz1)];

    let c00 = c000 * (1.0 - tx) + c100 * tx;
    let c01 = c001 * (1.0 - tx) + c101 * tx;
    let c10 = c010 * (1.0 - tx) + c110 * tx;
    let c11 = c011 * (1.0 - tx) + c111 * tx;
    let c0 = c00 * (1.0 - ty) + c10 * ty;
    let c1 = c01 * (1.0 - ty) + c11 * ty;
    c0 * (1.0 - tz) + c1 * tz
}
