use crate::model::{Structure, Atom};
use crate::model::elements::get_atomic_number;
use moyo::base::{Cell, Lattice, AngleTolerance};
use moyo::MoyoDataset;
use moyo::data::Setting;
use nalgebra::{Matrix3, Vector3};

// --- Structs for Analysis Results ---
pub struct SymmetryInfo {
    pub number: i32,
    pub symbol: String,
    pub system: String,
}

// =========================================================================
// 1. ANALYSIS: Read-only check of the Space Group (Used by UI)
// =========================================================================
pub fn analyze(structure: &Structure) -> Result<SymmetryInfo, String> {
    let lat = structure.lattice;

    // Convert Lattice
    let lattice_mat = Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2],
        lat[1][0], lat[1][1], lat[1][2],
        lat[2][0], lat[2][1], lat[2][2],
    );

    // Convert Atoms to Fractional
    let inv_mat = lattice_mat.try_inverse().ok_or("Invalid lattice")?;
    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let v_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
        let v_frac = inv_mat.transpose() * v_cart;
        positions.push(v_frac);

        let z = get_atomic_number(&atom.element);
        numbers.push(if z == 0 { 1 } else { z as i32 });
    }

    // Run Moyo
    let cell = Cell::new(Lattice::new(lattice_mat), positions, numbers);
    let dataset = MoyoDataset::new(&cell, 1e-4, AngleTolerance::Default, Setting::Spglib, true)
        .map_err(|_| "Symmetry search failed".to_string())?;

    // Decode System
    let sys_name = match dataset.number {
        1..=2 => "Triclinic",
        3..=15 => "Monoclinic",
        16..=74 => "Orthorhombic",
        75..=142 => "Tetragonal",
        143..=167 => "Trigonal",
        168..=194 => "Hexagonal",
        195..=230 => "Cubic",
        _ => "Unknown"
    };

    // Decode Symbol
    let symbol = if dataset.number >= 1 && dataset.number <= 230 {
        SG_SYMBOLS[dataset.number as usize].to_string()
    } else {
        "Unknown".to_string()
    };

    Ok(SymmetryInfo {
        number: dataset.number,
        symbol,
        system: sys_name.to_string(),
    })
}

// =========================================================================
// 2. TRANSFORMATION: Returns a NEW Standardized Structure
// =========================================================================
pub fn to_conventional_cell(structure: &Structure) -> Result<Structure, String> {
    let l = structure.lattice;

    // 1. Setup Lattice
    let lattice_mat = Matrix3::new(
        l[0][0], l[0][1], l[0][2],
        l[1][0], l[1][1], l[1][2],
        l[2][0], l[2][1], l[2][2],
    );
    let lattice = Lattice::new(lattice_mat);

    // 2. Calculate Inverse Lattice
    let inv_mat = lattice_mat.try_inverse()
        .ok_or("Invalid lattice (determinant is zero)")?;

    // 3. Convert Atoms (Cartesian -> Fractional)
    let mut positions = Vec::new();
    let mut numbers = Vec::new();
    let mut unique_elements = Vec::new();

    for atom in &structure.atoms {
        let v_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
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

        // Fractional -> Cartesian
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

// =========================================================================
// DATA: Space Group Symbols
// =========================================================================
const SG_SYMBOLS: [&str; 232] = ["","P1", "P-1", "P121", "P12_11", "C121", "P1m1", "P1c1", "C1m1", "C1c1", "P12/m1", "P12_1/m1", "C12/m1", "P12/c1", "P12_1/c1", "C12/c1", "P222", "P222_1", "P2_12_12", "P2_12_12_1", "C222_1", "C222", "F222", "I222", "I2_12_12_1", "Pmm2", "Pmc2_1", "Pcc2", "Pma2", "Pca2_1", "Pnc2", "Pmn2_1", "Pba2", "Pna2_1", "Pnn2", "Cmm2", "Cmc2_1", "Ccc2", "Amm2", "Aem2", "Ama2", "Aea2", "Fmm2", "Fdd2", "Imm2", "Iba2", "Ima2", "Pmmm", "Pnnn", "Pccm", "Pban", "Pmma", "Pnna", "Pmna", "Pcca", "Pbam", "Pccn", "Pbcm", "Pnnm", "Pmmn", "Pbcn", "Pbca", "Pnma", "Cmcm", "Cmce", "Cmmm", "Cccm", "Cmme", "Ccce", "Fmmm", "Fddd", "Immm", "Ibam", "Ibca", "Imma", "P4", "P4_1", "P4_2", "P4_3", "I4", "I4_1", "P-4", "I-4", "P4/m", "P4_2/m", "P4/n", "P4_2/n", "I4/m", "I4_1/a", "P422", "P42_12", "P4_122", "P4_12_12", "P4_222", "P4_22_12", "P4_322", "P4_32_12", "I422", "I4_122", "P4mm", "P4bm", "P4_2cm", "P4_2nm", "P4cc", "P4nc", "P4_2mc", "P4_2bc", "I4mm", "I4cm", "I4_1md", "I4_1cd", "P-42m", "P42c", "P-42_1m", "P-42_1c", "P-4m2", "P-4c2", "P-4b2", "P-4n2", "I-4m2", "I-4c2", "I-42m", "I-42d", "P4/mmm", "P4/mcc", "P4/nbm", "P4/nnc", "P4/mbm", "P4/mnc", "P4/nmm", "P4/ncc", "P4_2/mmc", "P4_2/mcm", "P4_2/nbc", "P4_2/nnm", "P4_2/mbc", "P4_2/mnm", "P4_2/nmc", "P4_2/ncm", "I4/mmm", "I4/mcm", "I4_1/amd", "I4_1/acd", "P3", "P3_1", "P3_2", "R3", "P-3", "R-3", "P312", "P321", "P3_112", "P3_121", "P3_212", "P3_221", "R32", "P3m1", "P31m", "P3c1", "P31c", "R3m", "R3c", "P-31m", "P-31c", "P-3m1", "P-3c1", "R-3m", "R-3c", "P6", "P6_1", "P6_5", "P6_2", "P6_4", "P6_3", "P-6", "P6/m", "P6_3/m", "P622", "P6_122", "P6_522", "P6_222", "P6_422", "P6_322", "P6mm", "P6cc", "P6_3cm", "P6_3mc", "P-6m2", "P-6c2", "P-62m", "P-62c", "P6/mmm", "P6/mcc", "P6_3/mcm", "P6_3/mmc", "P23", "F23", "I23", "P2_13", "I2_13", "Pm-3", "Pn-3", "Fm-3", "Fd-3", "Im-3", "Pa-3", "Ia-3", "P432", "P4_232", "F432", "F4_132", "I432", "P4_332", "P4_132", "I4_132", "P4_132", "P-43m", "F-43m", "I-43m", "P-43n", "F-43c", "I-43d", "Pm-3m", "Pn-3n", "Pm-3n", "Pn-3m", "Fm-3m", "Fm-3c", "Fd-3m", "Fd-3c", "Im-3m", "Ia-3d"];
