// src/physics/analysis/voids.rs

use crate::model::elements::{get_atom_cov, get_atom_ionic_radius, get_atom_vdw};
use crate::model::structure::Structure;
use crate::utils::linalg::cart_to_frac;
use crate::utils::task::CancelToken;
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

// --- 1. PUBLIC CONSTANTS (Single Source of Truth) ---

/// Standard molecular probes for void/porosity calculation
/// Based on kinetic diameters from gas adsorption experiments
pub const PRESET_PROBES: &[(&str, f64)] = &[
    ("He", 1.30),        // Kinetic diameter 2.60 Å (consistent with N2/CO2/CH4 rows)
    ("H₂", 1.45),        // Hydrogen storage
    ("H₂O", 1.32),       // Water accessibility
    ("CO₂", 1.65),       // Carbon capture
    ("N₂", 1.82),        // BET surface area (77K)
    ("O₂", 1.73),        // Oxygen transport
    ("Ar", 1.70),        // Alternative inert probe
    ("Kr", 1.80),        // Larger probe
    ("CH₄", 1.90),       // Methane storage
    ("C₂H₆", 2.20),      // Ethane separation
    ("Geometric", 0.00), // Pure geometric void (no probe)
];

/// Ceiling on grid points, and so on memory: samples are `f32`, so 30M
/// points is 120 MB for the field plus the cell list over atom images.
///
/// Sized by memory rather than time. Since the sampling kernel became a
/// cell-list scan (~44 ns per point regardless of atom count) and moved to a
/// cancellable worker thread, 30M points is about a second of wall clock --
/// a 310^3 grid, or a 30 A cell at 0.1 A resolution.
pub const DEFAULT_MAX_GRID_POINTS: usize = 30_000_000;

/// Common ions for intercalation analysis
/// Ionic radii from Shannon (1976) - coordination-dependent values
pub const CANDIDATE_IONS: &[(&str, f64)] = &[
    // Small cations (battery materials)
    ("Li⁺", 0.76),  // CN=6
    ("Mg²⁺", 0.72), // CN=6
    ("Zn²⁺", 0.74), // CN=6
    ("Al³⁺", 0.54), // CN=6
    // Medium cations
    ("Na⁺", 1.02),  // CN=6
    ("Ca²⁺", 1.00), // CN=6
    ("Fe²⁺", 0.78), // CN=6, high-spin
    ("Co²⁺", 0.75), // CN=6, high-spin
    ("Ni²⁺", 0.69), // CN=6
    // Large cations
    ("K⁺", 1.38),  // CN=6
    ("Rb⁺", 1.52), // CN=6
    ("Cs⁺", 1.67), // CN=6
    // Anions
    ("F⁻", 1.33),  // CN=6
    ("Cl⁻", 1.81), // CN=6
    ("O²⁻", 1.40), // CN=6
    ("S²⁻", 1.84), // CN=6
];

// --- 2. ERROR HANDLING ---

#[derive(Debug, Clone)]
pub enum VoidError {
    SingularLattice,
    InvalidProbeRadius(f64),
    InvalidRadiiScale(f64),
    InvalidGridResolution(f64),
    GridTooLarge { requested: usize, max: usize },
    NoAtoms,
}

impl fmt::Display for VoidError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VoidError::SingularLattice => write!(f, "Lattice matrix is singular (non-invertible)"),
            VoidError::InvalidProbeRadius(r) => {
                write!(f, "Probe radius must be non-negative, got {}", r)
            }
            VoidError::InvalidRadiiScale(s) => write!(f, "Radii scale must be positive, got {}", s),
            VoidError::InvalidGridResolution(r) => {
                write!(f, "Grid resolution must be positive, got {}", r)
            }
            VoidError::GridTooLarge { requested, max } => write!(
                f,
                "Grid too large: {} points requested, max {} allowed",
                requested, max
            ),
            VoidError::NoAtoms => write!(f, "Structure contains no atoms"),
        }
    }
}

impl std::error::Error for VoidError {}

// --- 3. CONFIGURATION ---

/// Radius type for void calculation
///
/// Scientific guidance:
/// - Ionic: Best for ionic/ceramic crystals (oxides, halides, perovskites)
/// - VanDerWaals: For molecular crystals and MOFs
/// - Covalent: Generally not recommended (underestimates atomic size)
#[derive(Clone, Copy, PartialEq, Debug)]
#[derive(Default)]
pub enum RadiusType {
    /// Ionic radii (Shannon 1976) - DEFAULT for crystalline solids
    #[default]
    Ionic,
    /// Van der Waals radii - use for molecular crystals
    VanDerWaals,
    /// Covalent radii - typically too small, use with caution
    Covalent,
}


#[derive(Clone, Copy, Debug)]
pub struct VoidConfig {
    /// Grid spacing in Angstroms (typical: 0.2-0.5 Å)
    pub grid_resolution: f64,

    /// Probe radius in Angstroms (0.0 = geometric void)
    pub probe_radius: f64,

    /// Scaling factor for atomic radii (typically 1.0)
    pub radii_scale: f64,

    /// Which atomic radius set to use
    pub radius_type: RadiusType,

    /// Maximum grid points to prevent memory issues
    pub max_grid_points: usize,
}

impl Default for VoidConfig {
    fn default() -> Self {
        Self {
            grid_resolution: 0.25,          // 0.25 Å - good balance
            probe_radius: 1.30,             // Helium probe (kinetic diameter 2.60 Å)
            radii_scale: 1.0,               // No scaling
            radius_type: RadiusType::Ionic, // Best for most crystals
            max_grid_points: DEFAULT_MAX_GRID_POINTS,
        }
    }
}

impl VoidConfig {
    /// Configuration for helium pycnometry (standard density measurement)
    pub fn helium_probe() -> Self {
        Self {
            probe_radius: 1.30, // He kinetic diameter 2.60 Å
            radius_type: RadiusType::Ionic,
            ..Default::default()
        }
    }

    /// Configuration for nitrogen porosimetry (BET surface area)
    pub fn nitrogen_probe() -> Self {
        Self {
            probe_radius: 1.82,
            radius_type: RadiusType::Ionic,
            ..Default::default()
        }
    }

    /// Geometric void analysis (no probe, pure geometry)
    pub fn geometric() -> Self {
        Self {
            probe_radius: 0.0,
            radius_type: RadiusType::Ionic,
            ..Default::default()
        }
    }

    /// For molecular crystals (use vdW radii)
    pub fn molecular_crystal() -> Self {
        Self {
            probe_radius: 1.30, // He probe (kinetic diameter 2.60 Å)
            radius_type: RadiusType::VanDerWaals,
            ..Default::default()
        }
    }

    /// Custom probe for ion intercalation studies
    pub fn ion_probe(ionic_radius: f64) -> Self {
        Self {
            probe_radius: ionic_radius,
            radius_type: RadiusType::Ionic,
            ..Default::default()
        }
    }

    /// Validate configuration
    fn validate(&self) -> Result<(), VoidError> {
        if self.probe_radius < 0.0 {
            return Err(VoidError::InvalidProbeRadius(self.probe_radius));
        }
        if self.radii_scale <= 0.0 {
            return Err(VoidError::InvalidRadiiScale(self.radii_scale));
        }
        if self.grid_resolution <= 0.0 {
            return Err(VoidError::InvalidGridResolution(self.grid_resolution));
        }
        Ok(())
    }
}

// --- 4. RESULTS ---

#[derive(Clone, Debug)]
pub struct VoidResult {
    /// Radius of largest sphere that fits in the structure (Å)
    pub max_sphere_radius: f64,

    /// Cartesian coordinates of largest sphere center (Å)
    pub max_sphere_center: [f64; 3],

    /// Percentage of volume accessible to the probe (%)
    pub void_fraction: f64,

    /// Configuration used for this calculation
    pub config: VoidConfig,

    /// Grid statistics
    pub grid_info: GridInfo,
}

#[derive(Clone, Debug)]
pub struct GridInfo {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    pub total_points: usize,
    pub void_points: usize,
}

impl VoidResult {
    /// Get ions from CANDIDATE_IONS that could fit in the largest void
    pub fn fitting_ions(&self) -> Vec<(&'static str, f64)> {
        CANDIDATE_IONS
            .iter()
            .filter(|(_, r)| *r <= self.max_sphere_radius)
            .copied()
            .collect()
    }

    /// Check if a specific ion can fit
    pub fn can_fit_ion(&self, ion_radius: f64) -> bool {
        ion_radius <= self.max_sphere_radius
    }
}

// --- 5. DISTANCE FIELD ---

/// One atom prepared for distance queries: fractional position plus the
/// scaled radius selected by [`RadiusType`].
#[derive(Clone, Copy, Debug)]
struct ProbeAtom {
    frac: Vector3<f64>,
    radius: f64,
}

/// Distance from `point` (fractional) to the nearest atom *surface*, in Å.
///
/// Brute-force reference implementation: scans every atom, and for an oblique
/// cell every one of the 27 images around the component-wise-rounded offset.
/// [`ImageGrid`] is what the sampling loops actually use; this is kept as the
/// definition the fast path is tested against.
#[cfg(test)]
fn min_surface_distance(
    point: Vector3<f64>,
    atoms: &[ProbeAtom],
    basis: &Matrix3<f64>,
    oblique: bool,
) -> f64 {
    let mut min_dist = f64::MAX;

    for atom in atoms {
        // Minimum image convention (periodic boundaries).
        let mut df = point - atom.frac;
        df.x -= df.x.round();
        df.y -= df.y.round();
        df.z -= df.z.round();

        let center_dist = if oblique {
            let mut best_sq = f64::MAX;
            for ox in -1..=1 {
                for oy in -1..=1 {
                    for oz in -1..=1 {
                        let d = basis * (df + Vector3::new(ox as f64, oy as f64, oz as f64));
                        best_sq = best_sq.min(d.norm_squared());
                    }
                }
            }
            best_sq.sqrt()
        } else {
            (basis * df).norm()
        };

        min_dist = min_dist.min(center_dist - atom.radius);
    }

    min_dist
}

/// Uniform cell list over the periodic images of the framework atoms.
///
/// The naive kernel is O(grid points x atoms), and for an oblique cell every
/// one of those pairs pays a further 27-image rescan to find the true minimum
/// image — 6 s for a 216-atom cell at 0.15 Å, all of it on one user action.
///
/// Expanding each atom into its 27 images once, up front, does two things:
/// the minimum-image search disappears (the images are explicit, so a plain
/// Cartesian distance is exact), and a spatial hash over them turns the inner
/// loop into a local neighbourhood scan whose cost does not grow with the
/// size of the cell.
struct ImageGrid {
    /// Flat cell array, indexed `z * dims[0] * dims[1] + y * dims[0] + x`.
    /// Each entry indexes into `positions`/`radii`.
    cells: Vec<Vec<u32>>,
    dims: [usize; 3],
    origin: Vector3<f64>,
    cell_size: f64,
    positions: Vec<Vector3<f64>>,
    radii: Vec<f64>,
    /// Largest radius present, needed by the shell-termination bound.
    max_radius: f64,
}

impl ImageGrid {
    fn build(atoms: &[ProbeAtom], basis: &Matrix3<f64>) -> Self {
        // Every atom in its own cell plus the 26 surrounding ones. A grid
        // point lies in the central cell, so its true nearest image is always
        // among these — the same guarantee the brute-force path relies on.
        let mut positions = Vec::with_capacity(atoms.len() * 27);
        let mut radii = Vec::with_capacity(atoms.len() * 27);

        for atom in atoms {
            for ox in -1..=1 {
                for oy in -1..=1 {
                    for oz in -1..=1 {
                        let shifted =
                            atom.frac + Vector3::new(ox as f64, oy as f64, oz as f64);
                        positions.push(basis * shifted);
                        radii.push(atom.radius);
                    }
                }
            }
        }

        let max_radius = radii.iter().cloned().fold(0.0f64, f64::max);

        // One cell per atom on average keeps both the per-cell scan and the
        // number of shells small.
        let volume = basis.determinant().abs();
        let mean_spacing = if atoms.is_empty() {
            2.0
        } else {
            (volume / atoms.len() as f64).cbrt()
        };
        let cell_size = mean_spacing.clamp(1.0, 8.0);

        let mut lo = Vector3::repeat(f64::MAX);
        let mut hi = Vector3::repeat(f64::MIN);
        for p in &positions {
            lo = lo.inf(p);
            hi = hi.sup(p);
        }
        // Guard against a degenerate span in any direction.
        let origin = lo - Vector3::repeat(cell_size);
        let span = (hi - lo) + Vector3::repeat(2.0 * cell_size);

        let dims = [
            ((span.x / cell_size).ceil() as usize).max(1),
            ((span.y / cell_size).ceil() as usize).max(1),
            ((span.z / cell_size).ceil() as usize).max(1),
        ];

        let mut cells = vec![Vec::new(); dims[0] * dims[1] * dims[2]];

        for (idx, p) in positions.iter().enumerate() {
            let c = Self::cell_of(*p, origin, cell_size, dims);
            cells[c[2] * dims[0] * dims[1] + c[1] * dims[0] + c[0]].push(idx as u32);
        }

        Self {
            cells,
            dims,
            origin,
            cell_size,
            positions,
            radii,
            max_radius,
        }
    }

    fn cell_of(
        p: Vector3<f64>,
        origin: Vector3<f64>,
        cell_size: f64,
        dims: [usize; 3],
    ) -> [usize; 3] {
        let raw = (p - origin) / cell_size;
        [
            (raw.x.floor() as isize).clamp(0, dims[0] as isize - 1) as usize,
            (raw.y.floor() as isize).clamp(0, dims[1] as isize - 1) as usize,
            (raw.z.floor() as isize).clamp(0, dims[2] as isize - 1) as usize,
        ]
    }

    /// Distance from a Cartesian point to the nearest atom surface.
    ///
    /// Searches cells in expanding Chebyshev shells. After shell `s` is done,
    /// nothing unsearched can be nearer than `s * cell_size` (the query point
    /// may sit anywhere within its own cell, which costs one shell), so the
    /// best possible remaining surface distance is `s * cell_size - r_max`.
    /// Once that cannot beat the best found, the answer is settled.
    fn nearest_surface_distance(&self, p: Vector3<f64>) -> f64 {
        let base = Self::cell_of(p, self.origin, self.cell_size, self.dims);
        let base = [base[0] as isize, base[1] as isize, base[2] as isize];

        let max_shell = self.dims[0].max(self.dims[1]).max(self.dims[2]) as isize;
        let mut best = f64::MAX;

        for s in 0..=max_shell {
            for dz in -s..=s {
                let z = base[2] + dz;
                if z < 0 || z >= self.dims[2] as isize {
                    continue;
                }
                let z_face = dz.abs() == s;

                for dy in -s..=s {
                    let y = base[1] + dy;
                    if y < 0 || y >= self.dims[1] as isize {
                        continue;
                    }
                    let y_face = dy.abs() == s;

                    // Only the surface of the cube is new at shell s; the
                    // interior was covered by earlier shells.
                    let step = if z_face || y_face { 1 } else { 2 * s.max(1) };

                    let mut dx = -s;
                    while dx <= s {
                        let x = base[0] + dx;
                        if x >= 0 && x < self.dims[0] as isize {
                            let cell = &self.cells[z as usize * self.dims[0] * self.dims[1]
                                + y as usize * self.dims[0]
                                + x as usize];
                            for &i in cell {
                                let i = i as usize;
                                let d = (self.positions[i] - p).norm() - self.radii[i];
                                if d < best {
                                    best = d;
                                }
                            }
                        }
                        dx += step;
                    }
                }
            }

            if s as f64 * self.cell_size - self.max_radius >= best {
                break;
            }
        }

        best
    }
}

/// The distance function sampled on a regular fractional grid over the cell.
///
/// [`calculate_voids`] reduces this to a single maximum and a void count;
/// `physics::analysis::interstitial` walks it for distinct interstitial
/// sites. Keeping the samples is what separates "how big is the biggest
/// hole" from "where are all the holes, and do they connect".
///
/// Samples are stored as `f32` — 4 bytes per point, so even the 10M-point
/// default cap costs 40 MB, and ordinary unit cells are orders of magnitude
/// below it. All arithmetic is done in `f64`; only storage is narrowed.
pub struct DistanceField {
    nx: usize,
    ny: usize,
    nz: usize,
    /// Indexed `k * (nx * ny) + j * nx + i` — `i` fastest, matching the
    /// sampling loops and the z-slice parallelism.
    data: Vec<f32>,
    /// Columns are the lattice vectors: maps fractional → Cartesian.
    basis: Matrix3<f64>,
    /// Cartesian → fractional.
    inv_basis: Matrix3<f64>,
    images: ImageGrid,
    oblique: bool,
    config: VoidConfig,
}

impl DistanceField {
    pub fn dims(&self) -> (usize, usize, usize) {
        (self.nx, self.ny, self.nz)
    }

    pub fn config(&self) -> VoidConfig {
        self.config
    }

    /// Sampled distance at a grid index. Indices wrap periodically, so
    /// neighbor scans do not need to special-case cell edges.
    pub fn at(&self, i: isize, j: isize, k: isize) -> f64 {
        let i = i.rem_euclid(self.nx as isize) as usize;
        let j = j.rem_euclid(self.ny as isize) as usize;
        let k = k.rem_euclid(self.nz as isize) as usize;
        self.data[k * self.nx * self.ny + j * self.nx + i] as f64
    }

    /// Fractional coordinates of a grid index.
    pub fn frac_of(&self, i: usize, j: usize, k: usize) -> Vector3<f64> {
        Vector3::new(
            i as f64 / self.nx as f64,
            j as f64 / self.ny as f64,
            k as f64 / self.nz as f64,
        )
    }

    /// The same distance function the grid samples, evaluated off-lattice.
    /// Sub-grid refinement of a site hill-climbs on this.
    pub fn distance_at_frac(&self, frac: Vector3<f64>) -> f64 {
        self.images.nearest_surface_distance(self.basis * frac)
    }

    pub fn to_cart(&self, frac: Vector3<f64>) -> Vector3<f64> {
        self.basis * frac
    }

    pub fn to_frac(&self, cart: Vector3<f64>) -> Vector3<f64> {
        self.inv_basis * cart
    }

    /// Shortest Cartesian distance between two fractional points under PBC.
    pub fn min_image_distance(&self, a: Vector3<f64>, b: Vector3<f64>) -> f64 {
        let mut df = a - b;
        df.x -= df.x.round();
        df.y -= df.y.round();
        df.z -= df.z.round();

        if self.oblique {
            let mut best_sq = f64::MAX;
            for ox in -1..=1 {
                for oy in -1..=1 {
                    for oz in -1..=1 {
                        let d = self.basis * (df + Vector3::new(ox as f64, oy as f64, oz as f64));
                        best_sq = best_sq.min(d.norm_squared());
                    }
                }
            }
            best_sq.sqrt()
        } else {
            (self.basis * df).norm()
        }
    }

    /// Smallest perpendicular spacing between adjacent sampling planes, in Å.
    /// The natural starting step for a sub-grid search.
    pub fn spacing_hint(&self) -> f64 {
        let (da, db, dc) = interplanar_spacings(&self.basis);
        (da / self.nx as f64)
            .min(db / self.ny as f64)
            .min(dc / self.nz as f64)
    }

    /// Walk a grid maximum to the true continuous maximum nearby.
    ///
    /// A compass (pattern) search rather than a gradient step: at a maximum of
    /// the distance function several atoms are equidistant, so the gradient is
    /// discontinuous there and a descent method stalls or oscillates. Halving
    /// the step on failure converges cleanly to well below the grid spacing.
    ///
    /// Steps are taken in Cartesian space and converted to fractional, so the
    /// search is isotropic in real space even in a sheared cell.
    pub fn refine_maximum(&self, start: Vector3<f64>) -> (Vector3<f64>, f64) {
        let dirs = compass_directions();

        let mut best = start;
        let mut best_d = self.distance_at_frac(best);

        // Start at the grid spacing — the maximum is by construction within one
        // grid step of where we started — and stop well under any radius
        // difference that could matter chemically.
        let mut step = self.spacing_hint();
        const MIN_STEP: f64 = 1e-4;

        while step > MIN_STEP {
            let mut improved = false;

            for dir in &dirs {
                let trial = best + self.to_frac(dir * step);
                let d = self.distance_at_frac(trial);
                if d > best_d {
                    best_d = d;
                    best = trial;
                    improved = true;
                }
            }

            if !improved {
                step *= 0.5;
            }
        }

        (wrap_frac(best), best_d)
    }

    /// Largest sampled clearance and the fractional point where it occurs.
    /// Grid-quantized — callers that report the value should refine it.
    fn global_max(&self) -> (f64, Vector3<f64>) {
        let mut best = f64::NEG_INFINITY;
        let mut best_idx = 0usize;

        for (idx, &v) in self.data.iter().enumerate() {
            if v as f64 > best {
                best = v as f64;
                best_idx = idx;
            }
        }

        if best == f64::NEG_INFINITY {
            return (0.0, Vector3::zeros());
        }

        let i = best_idx % self.nx;
        let j = (best_idx / self.nx) % self.ny;
        let k = best_idx / (self.nx * self.ny);

        (best, self.frac_of(i, j, k))
    }

    /// Number of samples with more clearance than `threshold`.
    fn count_above(&self, threshold: f64) -> usize {
        self.data.iter().filter(|&&v| v as f64 > threshold).count()
    }
}

/// 26 unit vectors covering the face, edge and corner directions of a cube.
/// The step set for the pattern search in [`refine_site`].
fn compass_directions() -> Vec<Vector3<f64>> {
    let mut dirs = Vec::with_capacity(26);
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                dirs.push(Vector3::new(dx as f64, dy as f64, dz as f64).normalize());
            }
        }
    }
    dirs
}

fn wrap_frac(v: Vector3<f64>) -> Vector3<f64> {
    Vector3::new(
        v.x.rem_euclid(1.0),
        v.y.rem_euclid(1.0),
        v.z.rem_euclid(1.0),
    )
}

/// Perpendicular spacings of the (100), (010) and (001) planes, in Å.
///
/// `d_a = V / |b × c|` is what actually limits how finely a sphere is
/// resolved along `a` — not `|a|`. The two coincide for an orthogonal cell;
/// for a sheared cell `d_a < |a|`, so sizing the grid off vector lengths
/// oversamples triclinic cells relative to the requested resolution.
fn interplanar_spacings(basis: &Matrix3<f64>) -> (f64, f64, f64) {
    let a = basis.column(0).into_owned();
    let b = basis.column(1).into_owned();
    let c = basis.column(2).into_owned();

    let volume = a.dot(&b.cross(&c)).abs();

    (
        volume / b.cross(&c).norm(),
        volume / c.cross(&a).norm(),
        volume / a.cross(&b).norm(),
    )
}

/// Sample the distance function over the unit cell.
///
/// # Algorithm
/// 1. Size a regular fractional grid from the requested resolution.
/// 2. For each grid point, find the distance to the nearest atom surface
///    under the minimum image convention.
///
/// # Returns
/// - `Ok(DistanceField)` with one sample per grid point
/// - `Err(VoidError)` if inputs are invalid or the grid would be too large
pub fn calculate_distance_field(
    structure: &Structure,
    config: VoidConfig,
) -> Result<DistanceField, VoidError> {
    // Cannot be cancelled, so the Option is always Some.
    Ok(calculate_distance_field_cancellable(structure, config, &CancelToken::never())?
        .expect("an uncancellable run cannot be cancelled"))
}

/// Sample the distance function, abandoning the work if `cancel` is tripped.
///
/// Returns `Ok(None)` when the run was cancelled. Superseded runs are the
/// normal case in the UI: every spin-button change starts a new one, and the
/// previous is dropped mid-sweep.
pub fn calculate_distance_field_cancellable(
    structure: &Structure,
    config: VoidConfig,
    cancel: &CancelToken,
) -> Result<Option<DistanceField>, VoidError> {
    config.validate()?;

    if structure.atoms.is_empty() {
        return Err(VoidError::NoAtoms);
    }

    let lat = structure.lattice;

    // Basis: columns = lattice vectors — maps fractional → Cartesian
    let basis = Matrix3::from_columns(&[
        Vector3::from(lat[0]),
        Vector3::from(lat[1]),
        Vector3::from(lat[2]),
    ]);
    let inv_basis = basis.try_inverse().ok_or(VoidError::SingularLattice)?;

    // Grid dimensions from the interplanar spacings, so `grid_resolution`
    // means the same perpendicular sample spacing in every cell shape.
    let (d_a, d_b, d_c) = interplanar_spacings(&basis);

    let nx = ((d_a / config.grid_resolution).ceil() as usize).max(1);
    let ny = ((d_b / config.grid_resolution).ceil() as usize).max(1);
    let nz = ((d_c / config.grid_resolution).ceil() as usize).max(1);

    let total_points = nx * ny * nz;

    if total_points > config.max_grid_points {
        return Err(VoidError::GridTooLarge {
            requested: total_points,
            max: config.max_grid_points,
        });
    }

    // --- Preprocess Atoms ---
    let atoms: Vec<ProbeAtom> = structure
        .atoms
        .iter()
        .filter_map(|a| {
            let frac = cart_to_frac(a.position, lat)?;
            let raw_radius = match config.radius_type {
                RadiusType::Ionic => get_atom_ionic_radius(&a.element),
                RadiusType::VanDerWaals => get_atom_vdw(&a.element),
                RadiusType::Covalent => get_atom_cov(&a.element),
            };
            Some(ProbeAtom {
                frac: Vector3::from(frac),
                radius: raw_radius * config.radii_scale,
            })
        })
        .collect();

    // Detect obliqueness once so the 27-offset scan is paid for only where
    // component-wise rounding is not already the exact minimum image.
    let a_col = basis.column(0);
    let b_col = basis.column(1);
    let c_col = basis.column(2);
    let oblique = a_col.dot(&b_col).abs() > 1e-9
        || a_col.dot(&c_col).abs() > 1e-9
        || b_col.dot(&c_col).abs() > 1e-9;

    let images = ImageGrid::build(&atoms, &basis);

    // --- Parallel Grid Sampling ---
    // Parallelize over z-slices for good load balancing. Grid points are
    // stepped in Cartesian space directly — `p = i*da + j*db + k*dc` — so the
    // inner loop is one vector add rather than a fractional-to-Cartesian
    // matrix multiply.
    let da = basis.column(0) / nx as f64;
    let db = basis.column(1) / ny as f64;
    let dc = basis.column(2) / nz as f64;

    let mut data = vec![0f32; total_points];
    let slice_len = nx * ny;
    let cancelled = AtomicBool::new(false);

    data.par_chunks_mut(slice_len)
        .enumerate()
        .for_each(|(k, slice)| {
            // Checked once per slice: fine-grained enough to abandon a stale
            // run promptly, coarse enough to stay off the hot path.
            if cancel.is_cancelled() {
                cancelled.store(true, Ordering::Relaxed);
                return;
            }

            let base_k = dc * k as f64;

            for j in 0..ny {
                let base_jk = base_k + db * j as f64;

                for i in 0..nx {
                    let p = base_jk + da * i as f64;
                    slice[j * nx + i] = images.nearest_surface_distance(p) as f32;
                }
            }
        });

    if cancelled.load(Ordering::Relaxed) || cancel.is_cancelled() {
        return Ok(None);
    }

    Ok(Some(DistanceField {
        nx,
        ny,
        nz,
        data,
        basis,
        inv_basis,
        images,
        oblique,
        config,
    }))
}

/// Calculate void space in a crystal structure.
///
/// A reduction over [`calculate_distance_field`]: the largest inscribed
/// sphere, and the fraction of samples the probe fits into.
///
/// # Returns
/// - `Ok(VoidResult)` with void analysis
/// - `Err(VoidError)` if inputs are invalid
pub fn calculate_voids(structure: &Structure, config: VoidConfig) -> Result<VoidResult, VoidError> {
    // Cannot be cancelled, so the Option is always Some.
    Ok(
        calculate_voids_cancellable(structure, config, &CancelToken::never())?
            .expect("an uncancellable run cannot be cancelled"),
    )
}

/// Calculate void space, abandoning the work if `cancel` is tripped.
///
/// Returns `Ok(None)` when the run was cancelled.
pub fn calculate_voids_cancellable(
    structure: &Structure,
    config: VoidConfig,
    cancel: &CancelToken,
) -> Result<Option<VoidResult>, VoidError> {
    let field = match calculate_distance_field_cancellable(structure, config, cancel)? {
        Some(field) => field,
        None => return Ok(None),
    };

    Ok(Some(summarize(&field)))
}

/// Reduce a sampled field to the headline void numbers.
///
/// Split out so a caller that needs the field for something else -- the
/// interstitial site search -- can have both from one sweep instead of
/// sampling the cell twice.
pub fn summarize(field: &DistanceField) -> VoidResult {
    let config = field.config();


    // The sampled maximum is quantized by the grid; refine it off-lattice
    // before reporting. This number decides which ions are said to fit, and
    // at a 0.25 A grid the quantization error straddles Li+ and Mg2+.
    let (grid_max, grid_frac) = field.global_max();
    let (centre_frac, max_sphere_radius) = if grid_max > f64::NEG_INFINITY {
        field.refine_maximum(grid_frac)
    } else {
        (grid_frac, grid_max)
    };

    let centre = field.to_cart(centre_frac);
    let max_sphere_center = [centre.x, centre.y, centre.z];

    let void_points = field.count_above(config.probe_radius);

    let (nx, ny, nz) = field.dims();
    let total_points = nx * ny * nz;

    let void_fraction = if total_points > 0 {
        (void_points as f64 / total_points as f64) * 100.0
    } else {
        0.0
    };

    VoidResult {
        max_sphere_radius,
        max_sphere_center,
        void_fraction,
        config,
        grid_info: GridInfo {
            nx,
            ny,
            nz,
            total_points,
            void_points,
        },
    }
}

// --- 6. CONVENIENCE FUNCTIONS ---

/// Quick void analysis with default settings (He probe, ionic radii)
pub fn quick_void_analysis(structure: &Structure) -> Result<VoidResult, VoidError> {
    calculate_voids(structure, VoidConfig::default())
}

/// Analyze which ions could fit in the structure
pub fn analyze_ion_intercalation(
    structure: &Structure,
) -> Result<Vec<(&'static str, bool)>, VoidError> {
    let result = calculate_voids(structure, VoidConfig::geometric())?;

    Ok(CANDIDATE_IONS
        .iter()
        .map(|(name, radius)| (*name, *radius <= result.max_sphere_radius))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    fn structure(lattice: [[f64; 3]; 3], fracs: &[[f64; 3]]) -> Structure {
        let basis = Matrix3::from_columns(&[
            Vector3::from(lattice[0]),
            Vector3::from(lattice[1]),
            Vector3::from(lattice[2]),
        ]);
        Structure {
            lattice,
            atoms: fracs
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let c = basis * Vector3::from(*f);
                    Atom {
                        element: if i % 2 == 0 { "Ti".into() } else { "O".into() },
                        position: [c.x, c.y, c.z],
                        original_index: i,
                        oxidation: None,
                        occupancy: 1.0,
                    }
                })
                .collect(),
            formula: "test".into(),
            is_periodic: true,
        }
    }

    /// Deterministic pseudo-random fractional coordinates — a fixed set beats
    /// a seeded RNG dependency, and a failure is reproducible.
    fn sample_points(n: usize) -> Vec<Vector3<f64>> {
        (0..n)
            .map(|i| {
                let a = (i as f64 * 0.6180339887).fract();
                let b = (i as f64 * 0.4142135624).fract();
                let c = (i as f64 * 0.7320508076).fract();
                Vector3::new(a, b, c)
            })
            .collect()
    }

    /// The cell list must agree with the brute-force definition everywhere,
    /// not just on average — it is the only thing standing between a fast
    /// kernel and quietly wrong void radii.
    fn assert_matches_brute_force(lattice: [[f64; 3]; 3], fracs: &[[f64; 3]]) {
        let s = structure(lattice, fracs);
        let field = calculate_distance_field(&s, VoidConfig::geometric()).unwrap();

        let atoms: Vec<ProbeAtom> = s
            .atoms
            .iter()
            .map(|a| ProbeAtom {
                frac: Vector3::from(cart_to_frac(a.position, lattice).unwrap()),
                radius: get_atom_ionic_radius(&a.element),
            })
            .collect();

        for p in sample_points(400) {
            let fast = field.distance_at_frac(p);
            let slow = min_surface_distance(p, &atoms, &field.basis, field.oblique);
            assert!(
                (fast - slow).abs() < 1e-9,
                "cell list {} != brute force {} at {:?}",
                fast,
                slow,
                p
            );
        }
    }

    #[test]
    fn cell_list_matches_brute_force_orthogonal() {
        assert_matches_brute_force(
            [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]],
            &[
                [0.0, 0.0, 0.0],
                [0.5, 0.5, 0.5],
                [0.25, 0.75, 0.1],
                [0.9, 0.2, 0.6],
            ],
        );
    }

    #[test]
    fn cell_list_matches_brute_force_oblique() {
        // Heavily sheared: component-wise rounding is nowhere near the true
        // minimum image here, which is exactly where a cell list can drift.
        assert_matches_brute_force(
            [[7.0, 0.0, 0.0], [3.4, 6.2, 0.0], [2.1, 2.8, 5.5]],
            &[
                [0.0, 0.0, 0.0],
                [0.5, 0.5, 0.5],
                [0.3, 0.1, 0.8],
                [0.7, 0.65, 0.2],
            ],
        );
    }

    /// A single atom in a large cell leaves most of the box empty, so the
    /// shell search has to walk several rings out before it can terminate.
    #[test]
    fn cell_list_matches_brute_force_sparse() {
        assert_matches_brute_force(
            [[14.0, 0.0, 0.0], [0.0, 14.0, 0.0], [0.0, 0.0, 14.0]],
            &[[0.1, 0.1, 0.1]],
        );
    }

    /// `grid_resolution` is a perpendicular spacing, so a sheared cell gets
    /// the divisions its interplanar spacings call for, not its edge lengths.
    #[test]
    fn grid_divisions_follow_interplanar_spacing() {
        let cubic = structure(
            [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            &[[0.0, 0.0, 0.0]],
        );
        let field = calculate_distance_field(&cubic, VoidConfig::geometric()).unwrap();
        // 10 A / 0.25 A
        assert_eq!(field.dims(), (40, 40, 40));

        // Shear b within the ab plane. The (100) planes are spanned by b and
        // c, so tilting b tilts them: d_a = V / |b x c| = 1000 / sqrt(100^2 +
        // 60^2) = 8.575 A, needing 35 divisions rather than the 40 that |a| =
        // 10 A would ask for. d_b and d_c are untouched at 10 A.
        let sheared = structure(
            [[10.0, 0.0, 0.0], [6.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            &[[0.0, 0.0, 0.0]],
        );
        let field = calculate_distance_field(&sheared, VoidConfig::geometric()).unwrap();
        assert_eq!(
            field.dims(),
            (35, 40, 40),
            "divisions must follow interplanar spacings, not edge lengths"
        );
    }

    /// A cancelled sweep must report cancellation rather than a half-filled
    /// field, or the UI would render garbage from an abandoned run.
    #[test]
    fn a_cancelled_run_yields_no_field() {
        let s = structure(
            [[8.0, 0.0, 0.0], [0.0, 8.0, 0.0], [0.0, 0.0, 8.0]],
            &[[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]],
        );

        let cancel = CancelToken::new();
        cancel.cancel();

        let result =
            calculate_distance_field_cancellable(&s, VoidConfig::geometric(), &cancel).unwrap();
        assert!(result.is_none());
    }
}
