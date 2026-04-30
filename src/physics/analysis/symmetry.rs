// src/physics/analysis/symmetry.rs
//
// Space group detection via the Moyo symmetry library.
// Read-only analysis — does NOT modify the Structure.
//
// Cell conversion (Primitive ↔ Conventional) lives in
// physics::operations::conversion to avoid duplication.

use crate::model::elements::get_atomic_number;
use crate::model::Structure;
use crate::utils::linalg::{cart_to_frac, lattice_to_matrix3};
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;

pub struct SymmetryInfo {
    pub number: i32,
    pub symbol: String,
    pub system: String,
}

// =========================================================================
// ANALYSIS: Read-only check of the Space Group (Used by UI)
// =========================================================================
pub fn analyze(structure: &Structure) -> Result<SymmetryInfo, String> {
    let lattice_mat = lattice_to_matrix3(structure.lattice);

    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let frac = cart_to_frac(atom.position, structure.lattice).ok_or("Invalid lattice")?;
        positions.push(nalgebra::Vector3::from(frac));
        let z = get_atomic_number(&atom.element);
        numbers.push(if z == 0 { 1 } else { z });
    }

    let cell = Cell::new(Lattice::new(lattice_mat), positions, numbers);
    let dataset = MoyoDataset::new(&cell, 1e-4, AngleTolerance::Default, Setting::Spglib, true)
        .map_err(|_| "Symmetry search failed".to_string())?;

    let sys_name = match dataset.number {
        1..=2 => "Triclinic",
        3..=15 => "Monoclinic",
        16..=74 => "Orthorhombic",
        75..=142 => "Tetragonal",
        143..=167 => "Trigonal",
        168..=194 => "Hexagonal",
        195..=230 => "Cubic",
        _ => "Unknown",
    };

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
// DATA: Space Group Symbols
// =========================================================================
const SG_SYMBOLS: [&str; 232] = [
    "",
    "P1",
    "P-1",
    "P121",
    "P12_11",
    "C121",
    "P1m1",
    "P1c1",
    "C1m1",
    "C1c1",
    "P12/m1",
    "P12_1/m1",
    "C12/m1",
    "P12/c1",
    "P12_1/c1",
    "C12/c1",
    "P222",
    "P222_1",
    "P2_12_12",
    "P2_12_12_1",
    "C222_1",
    "C222",
    "F222",
    "I222",
    "I2_12_12_1",
    "Pmm2",
    "Pmc2_1",
    "Pcc2",
    "Pma2",
    "Pca2_1",
    "Pnc2",
    "Pmn2_1",
    "Pba2",
    "Pna2_1",
    "Pnn2",
    "Cmm2",
    "Cmc2_1",
    "Ccc2",
    "Amm2",
    "Aem2",
    "Ama2",
    "Aea2",
    "Fmm2",
    "Fdd2",
    "Imm2",
    "Iba2",
    "Ima2",
    "Pmmm",
    "Pnnn",
    "Pccm",
    "Pban",
    "Pmma",
    "Pnna",
    "Pmna",
    "Pcca",
    "Pbam",
    "Pccn",
    "Pbcm",
    "Pnnm",
    "Pmmn",
    "Pbcn",
    "Pbca",
    "Pnma",
    "Cmcm",
    "Cmce",
    "Cmmm",
    "Cccm",
    "Cmme",
    "Ccce",
    "Fmmm",
    "Fddd",
    "Immm",
    "Ibam",
    "Ibca",
    "Imma",
    "P4",
    "P4_1",
    "P4_2",
    "P4_3",
    "I4",
    "I4_1",
    "P-4",
    "I-4",
    "P4/m",
    "P4_2/m",
    "P4/n",
    "P4_2/n",
    "I4/m",
    "I4_1/a",
    "P422",
    "P42_12",
    "P4_122",
    "P4_12_12",
    "P4_222",
    "P4_22_12",
    "P4_322",
    "P4_32_12",
    "I422",
    "I4_122",
    "P4mm",
    "P4bm",
    "P4_2cm",
    "P4_2nm",
    "P4cc",
    "P4nc",
    "P4_2mc",
    "P4_2bc",
    "I4mm",
    "I4cm",
    "I4_1md",
    "I4_1cd",
    "P-42m",
    "P42c",
    "P-42_1m",
    "P-42_1c",
    "P-4m2",
    "P-4c2",
    "P-4b2",
    "P-4n2",
    "I-4m2",
    "I-4c2",
    "I-42m",
    "I-42d",
    "P4/mmm",
    "P4/mcc",
    "P4/nbm",
    "P4/nnc",
    "P4/mbm",
    "P4/mnc",
    "P4/nmm",
    "P4/ncc",
    "P4_2/mmc",
    "P4_2/mcm",
    "P4_2/nbc",
    "P4_2/nnm",
    "P4_2/mbc",
    "P4_2/mnm",
    "P4_2/nmc",
    "P4_2/ncm",
    "I4/mmm",
    "I4/mcm",
    "I4_1/amd",
    "I4_1/acd",
    "P3",
    "P3_1",
    "P3_2",
    "R3",
    "P-3",
    "R-3",
    "P312",
    "P321",
    "P3_112",
    "P3_121",
    "P3_212",
    "P3_221",
    "R32",
    "P3m1",
    "P31m",
    "P3c1",
    "P31c",
    "R3m",
    "R3c",
    "P-31m",
    "P-31c",
    "P-3m1",
    "P-3c1",
    "R-3m",
    "R-3c",
    "P6",
    "P6_1",
    "P6_5",
    "P6_2",
    "P6_4",
    "P6_3",
    "P-6",
    "P6/m",
    "P6_3/m",
    "P622",
    "P6_122",
    "P6_522",
    "P6_222",
    "P6_422",
    "P6_322",
    "P6mm",
    "P6cc",
    "P6_3cm",
    "P6_3mc",
    "P-6m2",
    "P-6c2",
    "P-62m",
    "P-62c",
    "P6/mmm",
    "P6/mcc",
    "P6_3/mcm",
    "P6_3/mmc",
    "P23",
    "F23",
    "I23",
    "P2_13",
    "I2_13",
    "Pm-3",
    "Pn-3",
    "Fm-3",
    "Fd-3",
    "Im-3",
    "Pa-3",
    "Ia-3",
    "P432",
    "P4_232",
    "F432",
    "F4_132",
    "I432",
    "P4_332",
    "P4_132",
    "I4_132",
    "P4_132",
    "P-43m",
    "F-43m",
    "I-43m",
    "P-43n",
    "F-43c",
    "I-43d",
    "Pm-3m",
    "Pn-3n",
    "Pm-3n",
    "Pn-3m",
    "Fm-3m",
    "Fm-3c",
    "Fd-3m",
    "Fd-3c",
    "Im-3m",
    "Ia-3d",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    fn make_structure(lat: [[f64; 3]; 3], atoms: Vec<(&str, [f64; 3])>) -> Structure {
        // Inputs are FRACTIONAL coords; convert to Cartesian for the Structure.
        let a = nalgebra::Vector3::from(lat[0]);
        let b = nalgebra::Vector3::from(lat[1]);
        let c = nalgebra::Vector3::from(lat[2]);
        Structure {
            lattice: lat,
            atoms: atoms
                .into_iter()
                .enumerate()
                .map(|(i, (e, f))| {
                    let r = a * f[0] + b * f[1] + c * f[2];
                    Atom {
                        element: e.to_string(),
                        position: [r.x, r.y, r.z],
                        original_index: i,
                        oxidation: None,
                    }
                })
                .collect(),
            formula: String::new(),
            is_periodic: true,
        }
    }

    /// NaCl, Fm-3m → space group 225.
    #[test]
    fn test_spacegroup_nacl() {
        let a = 5.6402;
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        let s = make_structure(
            lat,
            vec![
                ("Na", [0.0, 0.0, 0.0]),
                ("Na", [0.5, 0.5, 0.0]),
                ("Na", [0.5, 0.0, 0.5]),
                ("Na", [0.0, 0.5, 0.5]),
                ("Cl", [0.5, 0.5, 0.5]),
                ("Cl", [0.0, 0.0, 0.5]),
                ("Cl", [0.0, 0.5, 0.0]),
                ("Cl", [0.5, 0.0, 0.0]),
            ],
        );
        let info = analyze(&s).expect("symmetry analysis failed");
        assert_eq!(
            info.number, 225,
            "NaCl should be Fm-3m (225), got {}",
            info.number
        );
        assert_eq!(info.system, "Cubic");
    }

    /// Cubic BaTiO₃ (high-T phase), Pm-3m → space group 221.
    #[test]
    fn test_spacegroup_batio3_cubic() {
        let a = 4.0;
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        let s = make_structure(
            lat,
            vec![
                ("Ba", [0.0, 0.0, 0.0]),
                ("Ti", [0.5, 0.5, 0.5]),
                ("O", [0.5, 0.5, 0.0]),
                ("O", [0.5, 0.0, 0.5]),
                ("O", [0.0, 0.5, 0.5]),
            ],
        );
        let info = analyze(&s).expect("symmetry analysis failed");
        assert_eq!(
            info.number, 221,
            "Cubic BaTiO3 should be Pm-3m (221), got {}",
            info.number
        );
        assert_eq!(info.system, "Cubic");
    }

    /// α-quartz, P3₁21 → space group 152 (or its enantiomorph 154 = P3₂21
    /// depending on handedness of the input coordinates).
    #[test]
    fn test_spacegroup_quartz() {
        let a = 4.9133;
        let c = 5.4053;
        let lat = [
            [a, 0.0, 0.0],
            [-a / 2.0, a * (3.0_f64).sqrt() / 2.0, 0.0],
            [0.0, 0.0, c],
        ];
        // Si on 3a Wyckoff (u, 0, 1/3) and equivalents; O on 6c (x, y, z).
        let u = 0.4697;
        let (ox, oy, oz) = (0.4133, 0.2672, 0.2144);
        let s = make_structure(
            lat,
            vec![
                ("Si", [u, 0.0, 1.0 / 3.0]),
                ("Si", [0.0, u, 2.0 / 3.0]),
                ("Si", [-u, -u, 0.0]),
                ("O", [ox, oy, oz]),
                ("O", [-oy, ox - oy, oz + 1.0 / 3.0]),
                ("O", [oy - ox, -ox, oz + 2.0 / 3.0]),
                ("O", [oy, ox, -oz]),
                ("O", [-ox, oy - ox, 1.0 / 3.0 - oz]),
                ("O", [ox - oy, -oy, 2.0 / 3.0 - oz]),
            ],
        );
        let info = analyze(&s).expect("symmetry analysis failed");
        assert!(
            info.number == 152 || info.number == 154,
            "Quartz should be P3_121 (152) or P3_221 (154), got {}",
            info.number
        );
        assert_eq!(info.system, "Trigonal");
    }
}
