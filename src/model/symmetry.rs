use crate::model::{Structure, Atom};
use spglib::cell::Cell;
use spglib::dataset::Dataset;

pub fn to_conventional_cell(structure: &Structure) -> Result<Structure, String> {
    let lattice = structure.lattice;
    let inv_lattice = invert_matrix(lattice).ok_or("Invalid lattice")?;

    let mut positions = Vec::new();
    let mut types = Vec::new();
    let mut unique_elements = Vec::new();

    for atom in &structure.atoms {
        let p = atom.position;
        // Cartesian -> Fractional
        let fx = p[0]*inv_lattice[0][0] + p[1]*inv_lattice[1][0] + p[2]*inv_lattice[2][0];
        let fy = p[0]*inv_lattice[0][1] + p[1]*inv_lattice[1][1] + p[2]*inv_lattice[2][1];
        let fz = p[0]*inv_lattice[0][2] + p[1]*inv_lattice[1][2] + p[2]*inv_lattice[2][2];

        positions.push([fx, fy, fz]);

        if !unique_elements.contains(&atom.element) {
            unique_elements.push(atom.element.clone());
        }
        let id = unique_elements.iter().position(|e| *e == atom.element).unwrap() as i32;
        types.push(id);
    }

    // FIX 1: Make 'cell' mutable
    let mut cell = Cell::new(&lattice, &positions, &types);

    // FIX 2: Pass &mut cell
    let dataset = Dataset::new(&mut cell, 1e-5);

    // Extract Standardized Cell data (Conventional Cell)
    let new_lattice = dataset.std_lattice;
    let new_positions = dataset.std_positions; // Fractional positions
    let new_types = dataset.std_types;

    // Safety check: if spglib failed, std_types might be empty or 0
    if new_types.is_empty() {
        return Err("Symmetry detection failed (empty result)".to_string());
    }

    let mut new_atoms = Vec::new();

    for (i, &t_id) in new_types.iter().enumerate() {
        // Safety: Ensure we don't go out of bounds if positions match types
        if i >= new_positions.len() { break; }

        let frac = new_positions[i];

        // Fractional -> Cartesian
        let cx = frac[0]*new_lattice[0][0] + frac[1]*new_lattice[1][0] + frac[2]*new_lattice[2][0];
        let cy = frac[0]*new_lattice[0][1] + frac[1]*new_lattice[1][1] + frac[2]*new_lattice[2][1];
        let cz = frac[0]*new_lattice[0][2] + frac[1]*new_lattice[1][2] + frac[2]*new_lattice[2][2];

        let element = if (t_id as usize) < unique_elements.len() {
            unique_elements[t_id as usize].clone()
        } else {
            "X".to_string()
        };

        // --- FIX: Added original_index ---
        let idx = new_atoms.len();
        new_atoms.push(Atom {
            element,
            position: [cx, cy, cz],
            original_index: idx,
        });
    }

    Ok(Structure {
        lattice: new_lattice,
        atoms: new_atoms,
        // --- FIX: Added formula (preserving original) ---
        formula: structure.formula.clone(),
    })
}

fn invert_matrix(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) -
              m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) +
              m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-6 { return None; }
    let inv = 1.0 / det;
    Some([
        [(m[1][1]*m[2][2]-m[1][2]*m[2][1])*inv, (m[0][2]*m[2][1]-m[0][1]*m[2][2])*inv, (m[0][1]*m[1][2]-m[0][2]*m[1][1])*inv],
        [(m[1][2]*m[2][0]-m[1][0]*m[2][2])*inv, (m[0][0]*m[2][2]-m[0][2]*m[2][0])*inv, (m[1][0]*m[0][2]-m[0][0]*m[1][2])*inv],
        [(m[1][0]*m[2][1]-m[1][1]*m[2][0])*inv, (m[2][0]*m[0][1]-m[0][0]*m[2][1])*inv, (m[0][0]*m[1][1]-m[1][0]*m[0][1])*inv],
    ])
}
