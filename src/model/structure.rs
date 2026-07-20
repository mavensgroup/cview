use serde::{Deserialize, Serialize}; // Assuming you use these for saving/loading

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Atom {
    pub element: String,
    pub position: [f64; 3],
    // We track the original index if needed for selection logic,
    // but for the supercell we essentially create "new" atoms.
    #[serde(skip)]
    pub original_index: usize,
    /// Formal oxidation state when known from the source file.
    ///
    /// `Some(n)` — the parser found an explicit charge (e.g. CIF
    /// `_atom_site_type_symbol = "Fe3+"`, `"O2-"`). Consumers like the
    /// Bond-Valence-Sum calculator use this directly instead of guessing.
    ///
    /// `None` — unknown; downstream code falls back to a priority-list of
    /// plausible valences. Set by parsers that have no native field for
    /// oxidation state (POSCAR, QE, XYZ).
    #[serde(default)]
    pub oxidation: Option<i32>,
    /// Site occupancy in [0, 1]. 1.0 (full) for formats without an
    /// occupancy field (POSCAR, QE, XYZ); CIF fills it from
    /// `_atom_site_occupancy`. Consumers weight per-site contributions by
    /// it: XRD scales the atomic form factor (virtual-crystal
    /// approximation), BVS scales each neighbor's bond valence.
    #[serde(default = "default_occupancy")]
    pub occupancy: f64,
}

fn default_occupancy() -> f64 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Structure {
    // Lattice vectors: [a_vec, b_vec, c_vec]
    pub lattice: [[f64; 3]; 3],
    pub atoms: Vec<Atom>,
    // Optional: Chemical formula string (e.g. "SiO2")
    #[serde(skip)]
    pub formula: String,
    /// Whether this structure has periodic boundary conditions.
    /// Set by parsers: CIF/POSCAR/QE/SPRKKR → true, XYZ → false (unless extended XYZ with Lattice=).
    /// Operations that transform a structure (supercell, slab, etc.) inherit from the parent.
    #[serde(default = "default_periodic")]
    pub is_periodic: bool,
}

fn default_periodic() -> bool {
    true
}
