use serde::{Deserialize, Serialize}; // Assuming you use these for saving/loading
// If you don't use serde yet, you can remove the #[derive(...)] parts below.

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
}

impl Structure {
    /// Creates a new Structure expanded by factors nx, ny, nz
    pub fn make_supercell(&self, nx: u32, ny: u32, nz: u32) -> Structure {
        let mut new_atoms = Vec::new();

        // 1. Extract Lattice Vectors
        // We assume the standard format: lattice[0] = a, lattice[1] = b, lattice[2] = c
        let vec_a = self.lattice[0];
        let vec_b = self.lattice[1];
        let vec_c = self.lattice[2];

        // 2. Loop through the multipliers
        let mut atom_counter = 0;

        for x in 0..nx {
            for y in 0..ny {
                for z in 0..nz {
                    // Calculate translation vector T = (x * a) + (y * b) + (z * c)
                    let translation = [
                        (vec_a[0] * x as f64) + (vec_b[0] * y as f64) + (vec_c[0] * z as f64),
                        (vec_a[1] * x as f64) + (vec_b[1] * y as f64) + (vec_c[1] * z as f64),
                        (vec_a[2] * x as f64) + (vec_b[2] * y as f64) + (vec_c[2] * z as f64),
                    ];

                    // 3. Clone every existing atom and shift it
                    for atom in &self.atoms {
                        let mut new_atom = atom.clone();

                        // Shift position: New = Old + Translation
                        new_atom.position[0] += translation[0];
                        new_atom.position[1] += translation[1];
                        new_atom.position[2] += translation[2];

                        // Update index (essential for selection to work on the new structure)
                        new_atom.original_index = atom_counter;

                        new_atoms.push(new_atom);
                        atom_counter += 1;
                    }
                }
            }
        }

        // 4. Scale the Lattice Matrix
        // The new unit cell is larger
        let new_lattice = [
            [vec_a[0] * nx as f64, vec_a[1] * nx as f64, vec_a[2] * nx as f64],
            [vec_b[0] * ny as f64, vec_b[1] * ny as f64, vec_b[2] * ny as f64],
            [vec_c[0] * nz as f64, vec_c[1] * nz as f64, vec_c[2] * nz as f64],
        ];

        // 5. Update Formula (Optional)
        // Usually, a supercell has the same chemical formula as the unit cell (ratio-wise),
        // or you can append "x N" to it. For now, we keep the original.
        let new_formula = format!("{} ({}x{}x{} Supercell)", self.formula, nx, ny, nz);

        Structure {
            lattice: new_lattice,
            atoms: new_atoms,
            formula: new_formula,
        }
    }
}
