use crate::model::{Structure, Atom};
use moyo::base::{Cell, Lattice, AngleTolerance};
use moyo::MoyoDataset;
use moyo::data::Setting;
use nalgebra::{Matrix3, Vector3};

pub fn to_conventional_cell(structure: &Structure) -> Result<Structure, String> {
    let l = structure.lattice;

    // 1. Setup Lattice
    let lattice_mat = Matrix3::new(
        l[0][0], l[0][1], l[0][2],
        l[1][0], l[1][1], l[1][2],
        l[2][0], l[2][1], l[2][2],
    );
    let lattice = Lattice::new(lattice_mat);

    // 2. Calculate Inverse Lattice for Coordinate Conversion
    // We need this to convert Cartesian (structure) -> Fractional (moyo)
    let inv_mat = lattice_mat.try_inverse()
        .ok_or("Invalid lattice (determinant is zero)")?;

    // 3. Convert Atoms (Cartesian -> Fractional)
    let mut positions = Vec::new();
    let mut numbers = Vec::new();
    let mut unique_elements = Vec::new();

    for atom in &structure.atoms {
        let v_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
        // Matrix multiplication: frac = (L^T)^-1 * cart
        // Since lattice_mat rows are basis vectors, we transpose before inversion in standard math,
        // or transpose the inverse. nalgebra vector is column.
        let v_frac = inv_mat.transpose() * v_cart;

        positions.push(v_frac);

        // Map element string to ID
        if !unique_elements.contains(&atom.element) {
            unique_elements.push(atom.element.clone());
        }
        let id = unique_elements.iter().position(|e| *e == atom.element).unwrap() as i32;
        numbers.push(id + 1); // 1-based ID for Moyo
    }

    let cell = Cell::new(lattice, positions, numbers);

    // 4. Run Moyo (Refine = true for Standardization)
    let dataset = MoyoDataset::new(
        &cell,
        1e-4,
        AngleTolerance::Default,
        Setting::Spglib,
        true
    ).map_err(|e| format!("Moyo symmetry search failed: {:?}", e))?;

    // 5. Convert Result Back (Fractional -> Cartesian)
    let std_cell = dataset.std_cell;
    let m = std_cell.lattice.basis;
    let new_lattice = [
        [m.m11, m.m12, m.m13],
        [m.m21, m.m22, m.m23],
        [m.m31, m.m32, m.m33],
    ];

    // Need conversion matrix for the NEW standardized lattice
    // Moyo returns standardized positions as fractional relative to std_lattice.
    // So we just multiply by std_lattice to get Cartesian.
    let std_lat_mat = std_cell.lattice.basis;

    let mut new_atoms = Vec::new();

    for (i, pos_frac) in std_cell.positions.iter().enumerate() {
        // Look up element
        let type_id = std_cell.numbers[i];
        let element = if (type_id - 1) < unique_elements.len() as i32 {
            unique_elements[(type_id - 1) as usize].clone()
        } else {
            "X".to_string()
        };

        // Fractional -> Cartesian (for CView Structure)
        // cart = L^T * frac
        let v_frac_vec = Vector3::new(pos_frac.x, pos_frac.y, pos_frac.z);
        let v_cart = std_lat_mat.transpose() * v_frac_vec;

        new_atoms.push(Atom {
            element,
            position: [v_cart.x, v_cart.y, v_cart.z],
            original_index: i,
        });
    }

    Ok(Structure {
        lattice: new_lattice,
        atoms: new_atoms,
        formula: structure.formula.clone(),
    })
}
