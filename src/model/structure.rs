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
    /// Magnetic moment, μB, as a Cartesian vector in the same frame as
    /// `position`. A collinear moment points along +z or −z.
    ///
    /// `None` — the file gave no moment (most formats). Read from
    /// INCAR `MAGMOM` / OUTCAR beside a POSCAR, vasprun.xml, pw.x output
    /// and extended XYZ `magmoms`; drawn as an arrow through the atom.
    /// Edits that keep the atom keep the moment (supercell, move); edits
    /// that change what sits on the site drop it (element substitution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magmom: Option<[f64; 3]>,
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

impl Structure {
    /// Whether every site is an OVF magnetization mesh cell.
    pub fn is_ovf_mesh(&self) -> bool {
        !self.atoms.is_empty() && self.atoms.iter().all(|a| a.element == crate::io::ovf::SITE_NAME)
    }

    /// The magnetic axis: the line of the largest moment, oriented towards
    /// +z (or +y, then +x, when it lies perpendicular), so that "up" does not
    /// depend on which of two equal antiparallel moments comes first. A
    /// collinear (anti/ferri)magnet splits cleanly into up and down along
    /// it. `None` without moments.
    pub fn magnetic_axis(&self) -> Option<[f64; 3]> {
        moment_axis(self.atoms.iter().filter_map(|a| a.magmom))
    }

    /// Whether every moment is parallel or antiparallel to the magnetic
    /// axis (true when there are none).
    pub fn moments_collinear(&self) -> bool {
        let Some(n) = self.magnetic_axis() else { return true };
        collinear_with(self.atoms.iter().filter_map(|a| a.magmom), n)
    }
}

/// Axis of the largest moment in `moments`, signed so its leading non-zero
/// component (z, then y, then x) is positive; `None` with no moment.
pub fn moment_axis(moments: impl Iterator<Item = [f64; 3]>) -> Option<[f64; 3]> {
    let norm = |m: &[f64; 3]| (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    let m = moments.max_by(|a, b| norm(a).total_cmp(&norm(b)))?;
    let n = norm(&m);
    if n <= 1e-9 {
        return None;
    }
    let mut axis = m.map(|x| x / n);
    let lead = [axis[2], axis[1], axis[0]].into_iter().find(|c| c.abs() > 1e-6).unwrap_or(1.0);
    if lead < 0.0 {
        axis = axis.map(|x| -x);
    }
    Some(axis)
}

/// Whether every moment is parallel or antiparallel to the unit vector `n`.
pub fn collinear_with(mut moments: impl Iterator<Item = [f64; 3]>, n: [f64; 3]) -> bool {
    moments.all(|m| {
        let c = [m[1] * n[2] - m[2] * n[1], m[2] * n[0] - m[0] * n[2], m[0] * n[1] - m[1] * n[0]];
        (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() < 1e-4
    })
}
