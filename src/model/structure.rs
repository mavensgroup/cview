use serde::{Deserialize, Serialize}; // Assuming you use these for saving/loading

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Atom {
    pub element: String,
    pub position: [f64; 3],
    // We track the original index if needed for selection logic,
    // but for the supercell we essentially create "new" atoms.
    #[serde(skip)]
    pub original_index: usize,
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
