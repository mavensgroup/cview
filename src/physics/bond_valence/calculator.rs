// src/physics/bond_valence/calculator.rs

use super::database::get_param;
use crate::model::structure::Structure;
use nalgebra::{Matrix3, Vector3};

/// Bond Valence Sum for a single atom
///
/// **Formula**: BVS = Σ exp((R₀ - R) / B)
///
/// Where:
/// - R₀ = reference bond length (from database or ML)
/// - R = observed bond length (from structure)
/// - B = softness parameter (typically 0.37)
///
/// **Reference**: Brown & Altermatt (1985) Acta Cryst. B41, 244-247
pub fn calculate_bvs(structure: &Structure, atom_idx: usize) -> f64 {
    let atom = &structure.atoms[atom_idx];
    let pos_a = Vector3::from(atom.position);
    let mut bvs = 0.0;

    // Standard cutoff for bond valence interactions
    const CUTOFF: f64 = 4.0; // Å

    for (i, neighbor) in structure.atoms.iter().enumerate() {
        if i == atom_idx {
            continue;
        }

        // Calculate distance with PBC if needed
        let pos_b = Vector3::from(neighbor.position);
        let dist = (pos_a - pos_b).norm();

        // Skip very close atoms (likely errors) and distant atoms
        if dist < 0.2 || dist > CUTOFF {
            continue;
        }

        // Get bond valence parameters
        let param = get_param(&atom.element, &neighbor.element);

        // Only count chemically reasonable bonds (R₀ > 0.5 Å)
        if param.r0 > 0.5 {
            // Brown-Altermatt formula: s = exp((R₀ - R) / B)
            let s = ((param.r0 - dist) / param.b).exp();
            bvs += s;
        }
    }

    bvs
}

/// Calculate BVS with periodic boundary conditions
///
/// For unit cells, atoms near boundaries may bond with images in neighboring cells.
/// This function considers the minimum image convention.

pub fn calculate_bvs_pbc(structure: &Structure, atom_idx: usize) -> f64 {
    let atom = &structure.atoms[atom_idx];
    let pos_a = Vector3::from(atom.position);
    let mut bvs = 0.0;

    // Build lattice matrix
    let lat = structure.lattice;
    let lattice_mat = Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    );

    // Inverse for Cartesian -> Fractional conversion
    let inv_lattice = match lattice_mat.try_inverse() {
        Some(inv) => inv,
        None => return calculate_bvs(structure, atom_idx), // Fallback if singular
    };

    const CUTOFF: f64 = 4.0;

    for (i, neighbor) in structure.atoms.iter().enumerate() {
        let pos_b = Vector3::from(neighbor.position);

        // Find minimum image distance
        let mut min_dist = f64::MAX;

        // === CRITICAL FIX: Check ±2 cells instead of ±1 ===
        for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    // Skip self in central cell
                    if i == atom_idx && dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }

                    let shift = Vector3::new(dx as f64, dy as f64, dz as f64);
                    let shifted_frac = inv_lattice.transpose() * pos_b + shift;
                    let shifted_cart = lattice_mat.transpose() * shifted_frac;

                    let dist = (pos_a - shifted_cart).norm();
                    if dist < min_dist && dist > 0.2 {
                        min_dist = dist;
                    }
                }
            }
        }

        if min_dist > CUTOFF {
            continue;
        }

        let param = get_param(&atom.element, &neighbor.element);

        if param.r0 > 0.5 {
            let s = ((param.r0 - min_dist) / param.b).exp();
            bvs += s;
        }
    }

    bvs * 2.0
}

/// Calculate BVS for all atoms (returns Vec for efficiency)
pub fn calculate_bvs_all(structure: &Structure) -> Vec<f64> {
    (0..structure.atoms.len())
        .map(|i| calculate_bvs(structure, i))
        .collect()
}

/// Calculate BVS for all atoms with PBC
pub fn calculate_bvs_all_pbc(structure: &Structure) -> Vec<f64> {
    (0..structure.atoms.len())
        .map(|i| calculate_bvs_pbc(structure, i))
        .collect()
}

/// Parallel BVS calculation for large structures (>500 atoms)
#[cfg(feature = "parallel")]
pub fn calculate_bvs_all_parallel(structure: &Structure) -> Vec<f64> {
    use rayon::prelude::*;

    (0..structure.atoms.len())
        .into_par_iter()
        .map(|i| calculate_bvs(structure, i))
        .collect()
}

/// Ideal oxidation state lookup
///
/// Returns the most common oxidation state for an element in ionic crystals.
/// Used for coloring and validation.
pub fn get_ideal_oxidation_state(element: &str) -> f64 {
    match element {
        // Anions (negative)
        "O" | "S" | "Se" | "Te" => 2.0,
        "F" | "Cl" | "Br" | "I" => 1.0,
        "N" | "P" | "As" => 3.0,

        // Group 1 (alkali metals)
        "H" | "Li" | "Na" | "K" | "Rb" | "Cs" | "Fr" => 1.0,

        // Group 2 (alkaline earth)
        "Be" | "Mg" | "Ca" | "Sr" | "Ba" | "Ra" => 2.0,

        // Group 13
        "B" | "Al" | "Ga" | "In" | "Tl" => 3.0,

        // Group 14
        "C" => 4.0, // In carbides/carbonates
        "Si" | "Ge" | "Sn" | "Pb" => 4.0,

        // Transition metals (most common states)
        "Sc" | "Y" | "La" => 3.0,
        "Ti" | "Zr" | "Hf" => 4.0,
        "V" | "Nb" | "Ta" => 5.0,
        "Cr" | "Mo" | "W" => 6.0, // High oxidation state in oxides

        // 3d metals (typically +2 in oxides, but Fe/Co/Ni can be +3)
        "Mn" => 2.0, // Can be 2-7
        "Fe" => 3.0, // Can be 2 or 3 (use higher for oxides)
        "Co" => 2.0, // Can be 2 or 3
        "Ni" => 2.0,
        "Cu" => 2.0, // Can be 1 or 2
        "Zn" => 2.0,

        // Lanthanides (rare earth)
        "Ce" | "Pr" | "Nd" | "Pm" | "Sm" | "Eu" | "Gd" | "Tb" | "Dy" | "Ho" | "Er" | "Tm"
        | "Yb" | "Lu" => 3.0,

        // Actinides
        "Th" | "Pa" | "U" | "Np" | "Pu" | "Am" => 4.0,

        // Noble metals
        "Ag" => 1.0,
        "Au" => 3.0,
        "Pt" => 4.0,

        // Post-transition metals
        "Cd" | "Hg" => 2.0,

        // Unknown - return 0 (will show as gray in coloring)
        _ => 0.0,
    }
}

/// Calculate deviation from ideal BVS
pub fn calculate_bvs_deviation(structure: &Structure, atom_idx: usize) -> f64 {
    let bvs_calc = calculate_bvs(structure, atom_idx);
    let bvs_ideal = get_ideal_oxidation_state(&structure.atoms[atom_idx].element);

    if bvs_ideal < 0.1 {
        return 0.0; // Unknown ideal state
    }

    (bvs_calc - bvs_ideal).abs()
}

/// Calculate overall structure quality based on BVS
///
/// Returns (average_deviation, max_deviation, num_validated_atoms)
pub fn calculate_structure_quality(structure: &Structure) -> (f64, f64, usize) {
    let mut sum_dev = 0.0;
    let mut max_dev = 0.0;
    let mut count = 0;

    for (i, atom) in structure.atoms.iter().enumerate() {
        let ideal = get_ideal_oxidation_state(&atom.element);

        // Only count atoms with known oxidation states
        if ideal > 0.1 {
            let bvs = calculate_bvs_pbc(structure, i);
            let dev = (bvs - ideal).abs();

            sum_dev += dev;
            if dev > max_dev {
                max_dev = dev;
            }
            count += 1;
        }
    }

    let avg_dev = if count > 0 {
        sum_dev / count as f64
    } else {
        0.0
    };

    (avg_dev, max_dev, count)
}

/// Quality assessment enum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BVSQuality {
    Excellent,  // Avg deviation < 0.15
    Good,       // < 0.25
    Acceptable, // < 0.40
    Poor,       // >= 0.40
}

impl BVSQuality {
    pub fn from_deviation(deviation: f64) -> Self {
        if deviation < 0.15 {
            BVSQuality::Excellent
        } else if deviation < 0.25 {
            BVSQuality::Good
        } else if deviation < 0.40 {
            BVSQuality::Acceptable
        } else {
            BVSQuality::Poor
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            BVSQuality::Excellent => "Excellent",
            BVSQuality::Good => "Good",
            BVSQuality::Acceptable => "Acceptable",
            BVSQuality::Poor => "Poor",
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            BVSQuality::Excellent => "✓",
            BVSQuality::Good => "✓",
            BVSQuality::Acceptable => "⚠",
            BVSQuality::Poor => "✗",
        }
    }
}

/// Assess structure quality
pub fn assess_structure_quality(structure: &Structure) -> BVSQuality {
    let (avg_dev, _, _) = calculate_structure_quality(structure);
    BVSQuality::from_deviation(avg_dev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    #[test]
    fn test_ideal_oxidation_states() {
        // Common ions
        assert_eq!(get_ideal_oxidation_state("O"), 2.0);
        assert_eq!(get_ideal_oxidation_state("Li"), 1.0);
        assert_eq!(get_ideal_oxidation_state("Fe"), 3.0);
        assert_eq!(get_ideal_oxidation_state("Al"), 3.0);
    }

    #[test]
    fn test_bvs_simple_oxide() {
        // Simple Li2O structure (should have good BVS)
        let structure = Structure {
            lattice: [[4.6, 0.0, 0.0], [0.0, 4.6, 0.0], [0.0, 0.0, 4.6]],
            atoms: vec![
                Atom {
                    element: "Li".into(),
                    position: [0.0, 0.0, 0.0],
                    original_index: 0,
                },
                Atom {
                    element: "Li".into(),
                    position: [2.3, 2.3, 0.0],
                    original_index: 1,
                },
                Atom {
                    element: "O".into(),
                    position: [2.3, 0.0, 2.3],
                    original_index: 2,
                },
            ],
            formula: "Li2O".into(),
        };

        // Li should be close to +1
        let bvs_li = calculate_bvs(&structure, 0);
        assert!(
            bvs_li > 0.5 && bvs_li < 1.5,
            "Li BVS out of range: {}",
            bvs_li
        );

        // O should be close to -2 (but BVS gives magnitude)
        let bvs_o = calculate_bvs(&structure, 2);
        assert!(bvs_o > 1.0 && bvs_o < 3.0, "O BVS out of range: {}", bvs_o);
    }

    #[test]
    fn test_quality_assessment() {
        let excellent = BVSQuality::from_deviation(0.10);
        assert_eq!(excellent, BVSQuality::Excellent);

        let poor = BVSQuality::from_deviation(0.50);
        assert_eq!(poor, BVSQuality::Poor);
    }
}
