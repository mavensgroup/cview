// src/physics/analysis/kpath.rs

use crate::model::bs_data::{self, BrillouinZoneData};
use crate::model::elements::get_atomic_number;
use crate::model::structure::Structure;
use crate::utils::linalg::cart_to_frac;
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
    // 1. Convert Input Structure to Moyo Cell
    let input_lattice = Matrix3::from_columns(&[
        Vector3::from(structure.lattice[0]),
        Vector3::from(structure.lattice[1]),
        Vector3::from(structure.lattice[2]),
    ]);

    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let frac = cart_to_frac(atom.position, structure.lattice)?;
        positions.push(Vector3::from(frac));
        numbers.push(get_atomic_number(&atom.element) as i32);
    }

    let moyo_cell = Cell::new(Lattice::new(input_lattice.transpose()), positions, numbers);

    // 2. Get Symmetry Dataset
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

    // 3. Extract Standardized Primitive Lattice (Moyo row-major → transpose for columns)
    let std_prim_lattice = dataset.std_cell.lattice.basis.transpose();

    // 4. Reciprocal Lattice: B = 2π * (A⁻¹)ᵀ
    let two_pi = 2.0 * std::f64::consts::PI;
    let rec_lattice = std_prim_lattice.try_inverse()?.transpose() * two_pi;

    // 5. Lookup K-points
    let data = bs_data::get_sc_data(sg_num)?;

    let mut linear_kpoints = Vec::new();
    let mut path_segments = Vec::new();

    let make_kp = |label: &str| -> Option<KPoint> {
        let frac = data.special_points.get(label)?;
        let c_vec = rec_lattice * Vector3::from(*frac);
        Some(KPoint {
            label: label.to_string(),
            coords_frac: *frac,
            coords_cart: [c_vec.x, c_vec.y, c_vec.z],
        })
    };

    for segment_labels in &data.path {
        let mut segment_pts = Vec::new();
        for label in segment_labels {
            if let Some(kp) = make_kp(label) {
                segment_pts.push(kp.clone());
                linear_kpoints.push(kp);
            }
        }
        path_segments.push(segment_pts);
    }

    // 6. Transform Wireframe frac → cart
    let cart_lines = data
        .wireframe
        .iter()
        .map(|(start_f, end_f)| {
            let s = rec_lattice * Vector3::from(*start_f);
            let e = rec_lattice * Vector3::from(*end_f);
            ([s.x, s.y, s.z], [e.x, e.y, e.z])
        })
        .collect();

    Some(KPathResult {
        spacegroup_str: format!("{} ({})", sg_num, dataset.hall_number),
        number: sg_num,
        lattice_type: data.lattice_type,
        kpoints: linear_kpoints,
        path_segments,
        bz_lines: cart_lines,
        rec_lattice,
    })
}
