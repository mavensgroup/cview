// src/physics/analysis/voids.rs

use crate::model::elements::{get_atom_cov, get_atom_ionic_radius, get_atom_vdw};
use crate::model::structure::Structure;
use crate::utils::linalg::cart_to_frac;
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;
use std::fmt;

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
            max_grid_points: 10_000_000,    // ~10M points limit
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
/// Positive in open space, negative inside an atom. This is the single
/// quantity the whole void/interstitial stack is built on:
/// `d(x) = min_i (|x - r_i|_pbc - R_i)`.
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

        // Component-wise fractional rounding is an exact minimum image only
        // for orthogonal cells; for oblique cells the true nearest image can
        // be a neighboring offset, so scan the 27 around it.
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
    atoms: Vec<ProbeAtom>,
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
        min_surface_distance(frac, &self.atoms, &self.basis, self.oblique)
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

    // --- Parallel Grid Sampling ---
    // Parallelize over z-slices for good load balancing.
    let mut data = vec![0f32; total_points];
    let slice_len = nx * ny;

    data.par_chunks_mut(slice_len)
        .enumerate()
        .for_each(|(k, slice)| {
            let frac_k = k as f64 / nz as f64;

            for j in 0..ny {
                let frac_j = j as f64 / ny as f64;

                for i in 0..nx {
                    let frac_i = i as f64 / nx as f64;
                    let pt = Vector3::new(frac_i, frac_j, frac_k);
                    slice[j * nx + i] =
                        min_surface_distance(pt, &atoms, &basis, oblique) as f32;
                }
            }
        });

    Ok(DistanceField {
        nx,
        ny,
        nz,
        data,
        basis,
        inv_basis,
        atoms,
        oblique,
        config,
    })
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
    let field = calculate_distance_field(structure, config)?;

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

    Ok(VoidResult {
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
    })
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
