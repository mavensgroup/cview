// src/physics/analysis/kpath.rs
//
// Orchestrates k-path calculation:
//   1. Symmetry detection via moyo → space group + standardized cell
//   2. Bravais classification + k-point computation (bravais submodule)
//   3. Brillouin zone wireframe via Voronoi construction (voronoi submodule)
//
// Convention (Setyawan-Curtarolo 2010):
//   - Reciprocal lattice is built from the PRIMITIVE standardized cell
//   - Bravais classification uses the CONVENTIONAL standardized cell parameters
//   - K-point fractional coordinates are in the primitive reciprocal basis

use super::bravais;
use super::voronoi;
use crate::model::elements::get_atomic_number;
use crate::model::structure::Structure;
use crate::utils::linalg::{cart_to_frac, lattice_to_matrix3};
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;
use nalgebra::{Matrix3, Vector3};

const SYMPREC: f64 = 1e-3;

#[derive(Debug, Clone)]
pub struct KPoint {
    pub label: String,
    pub coords_frac: [f64; 3],
    pub coords_cart: [f64; 3],
}

#[derive(Debug, Clone)]
pub struct KPathResult {
    pub spacegroup_str: String,
    pub number: i32,
    pub lattice_type: String,
    pub kpoints: Vec<KPoint>,
    pub path_segments: Vec<Vec<KPoint>>,
    pub bz_lines: Vec<([f64; 3], [f64; 3])>,
    pub rec_lattice: Matrix3<f64>,
}

pub fn calculate_kpath(structure: &Structure) -> Option<KPathResult> {
    // 1. Convert input structure to Moyo cell
    //    lattice_to_matrix3 produces rows = lattice vectors;
    //    Moyo's Lattice::new expects the same convention.
    let lat_mat = lattice_to_matrix3(structure.lattice);

    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let frac = cart_to_frac(atom.position, structure.lattice)?;
        positions.push(Vector3::from(frac));
        numbers.push(get_atomic_number(&atom.element));
    }

    let moyo_cell = Cell::new(Lattice::new(lat_mat), positions, numbers);

    // 2. Symmetry detection
    let dataset = MoyoDataset::new(
        &moyo_cell,
        SYMPREC,
        AngleTolerance::Default,
        Setting::Spglib,
        true,
    )
    .ok()?;

    let sg_num = dataset.number;
    crate::utils::console::log_debug(&format!("[KPATH] Detected Space Group: #{}", sg_num));

    // 3. Reciprocal lattice from the PRIMITIVE standardized cell
    //    Moyo basis has rows = lattice vectors, so transpose gives columns = lattice vectors.
    //    Reciprocal lattice: B = 2π (A^T)^{-1}  where A has columns = direct lattice vectors.
    //    Equivalently: B = 2π (A^{-1})^T
    let std_prim_col = dataset.prim_std_cell.lattice.basis.transpose();
    let two_pi = 2.0 * std::f64::consts::PI;
    let rec_lattice = std_prim_col.try_inverse()?.transpose() * two_pi;

    // 4. Lattice parameters from the CONVENTIONAL standardized cell
    //    Bravais classification requires a, b, c, α, β, γ of the conventional cell.
    let std_conv_col = dataset.std_cell.lattice.basis.transpose();
    let params = bravais::extract_lattice_params(&std_conv_col);

    crate::utils::console::log_debug(&format!(
        "[KPATH] Lattice params: a={:.4}, b={:.4}, c={:.4}, α={:.2}°, β={:.2}°, γ={:.2}°",
        params.a,
        params.b,
        params.c,
        params.alpha.to_degrees(),
        params.beta.to_degrees(),
        params.gamma.to_degrees()
    ));

    // 5. Classify Bravais type and compute k-points at runtime
    let bravais_type = bravais::classify(sg_num, &params);
    let kdata = bravais::compute_kdata(bravais_type, &params);

    crate::utils::console::log_debug(&format!(
        "[KPATH] Bravais type: {:?} ({})",
        bravais_type, kdata.label
    ));

    // 6. Build k-point list and path segments
    //    Fractional coords are in the primitive reciprocal basis;
    //    Cartesian = rec_lattice * frac (columns of rec_lattice are b1, b2, b3).
    let mut linear_kpoints = Vec::new();
    let mut path_segments = Vec::new();

    let make_kp = |label: &str| -> Option<KPoint> {
        let frac = kdata.special_points.get(label)?;
        let c_vec = rec_lattice * Vector3::from(*frac);
        Some(KPoint {
            label: label.to_string(),
            coords_frac: *frac,
            coords_cart: [c_vec.x, c_vec.y, c_vec.z],
        })
    };

    for segment_labels in &kdata.path {
        let mut segment_pts = Vec::new();
        for label in segment_labels {
            if let Some(kp) = make_kp(label) {
                segment_pts.push(kp.clone());
                linear_kpoints.push(kp);
            }
        }
        path_segments.push(segment_pts);
    }

    // 7. Compute BZ wireframe via Voronoi construction
    let bz_lines = voronoi::compute_bz_wireframe(&rec_lattice);

    crate::utils::console::log_debug(&format!(
        "[KPATH] BZ wireframe: {} edges, {} k-points on path",
        bz_lines.len(),
        linear_kpoints.len()
    ));

    Some(KPathResult {
        spacegroup_str: format!("{} ({})", sg_num, dataset.hall_number),
        number: sg_num,
        lattice_type: kdata.label,
        kpoints: linear_kpoints,
        path_segments,
        bz_lines,
        rec_lattice,
    })
}
