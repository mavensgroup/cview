use crate::model::elements::{get_atom_cov, get_atom_ionic_radius, get_atom_vdw};
use crate::model::structure::Structure;
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;
use std::fmt;

// --- 1. PUBLIC CONSTANTS (Single Source of Truth) ---

/// Standard molecular probes for void/porosity calculation
/// Based on kinetic diameters from gas adsorption experiments
pub const PRESET_PROBES: &[(&str, f64)] = &[
    ("He", 1.20),        // Helium pycnometry standard
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
pub enum RadiusType {
    /// Ionic radii (Shannon 1976) - DEFAULT for crystalline solids
    Ionic,
    /// Van der Waals radii - use for molecular crystals
    VanDerWaals,
    /// Covalent radii - typically too small, use with caution
    Covalent,
}

impl Default for RadiusType {
    fn default() -> Self {
        RadiusType::Ionic // Scientifically appropriate for most crystals
    }
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
            probe_radius: 1.20,             // Helium probe (standard)
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
            probe_radius: 1.20,
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
            probe_radius: 1.20,
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

// --- 5. MAIN CALCULATION ---

/// Calculate void space in a crystal structure
///
/// # Algorithm
/// 1. Create 3D grid in fractional coordinates
/// 2. For each grid point, find distance to nearest atom surface
/// 3. Points farther than probe_radius from all atoms = voids
/// 4. Track largest inscribed sphere
///
/// # Returns
/// - `Ok(VoidResult)` with void analysis
/// - `Err(VoidError)` if inputs are invalid
pub fn calculate_voids(structure: &Structure, config: VoidConfig) -> Result<VoidResult, VoidError> {
    // --- Validation ---
    config.validate()?;

    if structure.atoms.is_empty() {
        return Err(VoidError::NoAtoms);
    }

    let lat = structure.lattice;

    // --- Basis Matrix Setup ---
    // Columns = lattice vectors (a, b, c)
    // Maps fractional -> Cartesian coordinates
    let basis = Matrix3::new(
        lat[0][0], lat[1][0], lat[2][0], lat[0][1], lat[1][1], lat[2][1], lat[0][2], lat[1][2],
        lat[2][2],
    );

    let inv_basis = basis.try_inverse().ok_or(VoidError::SingularLattice)?;

    // --- Grid Dimensions ---
    let a_len = (lat[0][0].powi(2) + lat[0][1].powi(2) + lat[0][2].powi(2)).sqrt();
    let b_len = (lat[1][0].powi(2) + lat[1][1].powi(2) + lat[1][2].powi(2)).sqrt();
    let c_len = (lat[2][0].powi(2) + lat[2][1].powi(2) + lat[2][2].powi(2)).sqrt();

    let nx = ((a_len / config.grid_resolution).ceil() as usize).max(1);
    let ny = ((b_len / config.grid_resolution).ceil() as usize).max(1);
    let nz = ((c_len / config.grid_resolution).ceil() as usize).max(1);

    let total_points = nx * ny * nz;

    if total_points > config.max_grid_points {
        return Err(VoidError::GridTooLarge {
            requested: total_points,
            max: config.max_grid_points,
        });
    }

    // --- Preprocess Atoms ---
    struct ProcessedAtom {
        frac: Vector3<f64>,
        radius: f64,
    }

    let atoms_data: Vec<ProcessedAtom> = structure
        .atoms
        .iter()
        .map(|a| {
            let cart = Vector3::new(a.position[0], a.position[1], a.position[2]);
            let frac = inv_basis * cart;

            // Get appropriate radius
            let raw_radius = match config.radius_type {
                RadiusType::Ionic => get_atom_ionic_radius(&a.element),
                RadiusType::VanDerWaals => get_atom_vdw(&a.element),
                RadiusType::Covalent => get_atom_cov(&a.element),
            };

            ProcessedAtom {
                frac,
                radius: raw_radius * config.radii_scale,
            }
        })
        .collect();

    // --- Parallel Grid Sampling ---
    // Parallelize over z-slices for good load balancing
    let results: Vec<(f64, [f64; 3], usize)> = (0..nz)
        .into_par_iter()
        .map(|k| {
            let mut local_max_dist = f64::NEG_INFINITY;
            let mut local_best_point = [0.0; 3];
            let mut local_void_count = 0;

            let frac_k = k as f64 / nz as f64;

            for j in 0..ny {
                let frac_j = j as f64 / ny as f64;

                for i in 0..nx {
                    let frac_i = i as f64 / nx as f64;

                    // Current grid point in fractional coordinates
                    let pt_frac = Vector3::new(frac_i, frac_j, frac_k);

                    // Find minimum distance to any atom surface
                    let mut min_dist_to_surface = f64::MAX;

                    for atom in &atoms_data {
                        // Apply minimum image convention (periodic boundaries)
                        let mut df = pt_frac - atom.frac;
                        df.x -= df.x.round();
                        df.y -= df.y.round();
                        df.z -= df.z.round();

                        // Convert to Cartesian for distance measurement
                        let d_cart = basis * df;
                        let center_dist = d_cart.norm();

                        // Distance to atom surface = distance to center - radius
                        let surface_dist = center_dist - atom.radius;

                        min_dist_to_surface = min_dist_to_surface.min(surface_dist);
                    }

                    // Track largest sphere
                    if min_dist_to_surface > local_max_dist {
                        local_max_dist = min_dist_to_surface;
                        let cart = basis * pt_frac;
                        local_best_point = [cart.x, cart.y, cart.z];
                    }

                    // Count as void if probe can fit
                    if min_dist_to_surface > config.probe_radius {
                        local_void_count += 1;
                    }
                }
            }

            (local_max_dist, local_best_point, local_void_count)
        })
        .collect();

    // --- Aggregate Results ---
    let mut max_sphere_radius = f64::NEG_INFINITY;
    let mut max_sphere_center = [0.0; 3];
    let mut total_void_points = 0;

    for (max_dist, point, void_count) in results {
        total_void_points += void_count;
        if max_dist > max_sphere_radius {
            max_sphere_radius = max_dist;
            max_sphere_center = point;
        }
    }

    // Handle edge case where all points are inside atoms
    if max_sphere_radius == f64::NEG_INFINITY {
        max_sphere_radius = 0.0;
    }

    let void_fraction = if total_points > 0 {
        (total_void_points as f64 / total_points as f64) * 100.0
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
            void_points: total_void_points,
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
