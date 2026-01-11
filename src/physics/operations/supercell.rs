use crate::model::structure::Structure;

pub fn generate(structure: &Structure, nx: u32, ny: u32, nz: u32) -> Structure {
    let mut new_atoms = Vec::new();

    let vec_a = structure.lattice[0];
    let vec_b = structure.lattice[1];
    let vec_c = structure.lattice[2];

    let mut atom_counter = 0;

    for x in 0..nx {
        for y in 0..ny {
            for z in 0..nz {
                let translation = [
                    vec_a[0] * x as f64 + vec_b[0] * y as f64 + vec_c[0] * z as f64,
                    vec_a[1] * x as f64 + vec_b[1] * y as f64 + vec_c[1] * z as f64,
                    vec_a[2] * x as f64 + vec_b[2] * y as f64 + vec_c[2] * z as f64,
                ];

                for atom in &structure.atoms {
                    let mut new_atom = atom.clone();
                    new_atom.position[0] += translation[0];
                    new_atom.position[1] += translation[1];
                    new_atom.position[2] += translation[2];
                    new_atom.original_index = atom_counter;
                    new_atoms.push(new_atom);
                    atom_counter += 1;
                }
            }
        }
    }

    let new_lattice = [
        [vec_a[0] * nx as f64, vec_a[1] * nx as f64, vec_a[2] * nx as f64],
        [vec_b[0] * ny as f64, vec_b[1] * ny as f64, vec_b[2] * ny as f64],
        [vec_c[0] * nz as f64, vec_c[1] * nz as f64, vec_c[2] * nz as f64],
    ];

    let new_formula = format!("{} ({}x{}x{} Supercell)", structure.formula, nx, ny, nz);

    Structure {
        lattice: new_lattice,
        atoms: new_atoms,
        formula: new_formula,
    }
}
