// src/physics/analysis/kpath.rs
//
// Orchestrates k-path calculation:
//   1. Symmetry detection via moyo → space group + standardized cell
//   2. Bravais classification + k-point computation (bravais submodule)
//   3. Brillouin zone wireframe via Voronoi construction (voronoi submodule)
//
// Convention (Setyawan-Curtarolo 2010):
//   - moyo's conventional-cell parameters are first mapped onto the SC10
//     conventions (bravais::sc_conventional_params)
//   - Bravais classification uses those SC-convention parameters
//   - The reciprocal lattice is built from the SC primitive cell constructed
//     explicitly per SC10 (bravais::sc_primitive_lattice); k-point fractional
//     coordinates are in that primitive reciprocal basis

use super::bravais;
use super::voronoi;
use crate::model::elements::get_atomic_number;
use crate::model::structure::Structure;
use crate::utils::linalg::{cart_to_frac, lattice_to_matrix3};
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;
use nalgebra::{Matrix3, Vector3};

// Unified application-wide symmetry tolerance (KP-12/SY-2): kpath must
// agree with the symmetry tab and cell conversion about the space group.
use super::symmetry::SYMPREC;

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

    // 3. Lattice parameters from the CONVENTIONAL standardized cell.
    //    moyo's `Lattice::new` transposes its row-vector input, so the stored
    //    `basis` already has COLUMNS = lattice vectors — use it directly.
    let ita_params = bravais::extract_lattice_params(&dataset.std_cell.lattice.basis);

    // 4. Map ITA/spglib conventional parameters onto the SC10 conventions
    //    (rhombohedral cell for R groups, oblique angle → α for monoclinic,
    //    A→C permutation for SG 38-41, ordering constraints).
    let params = bravais::sc_conventional_params(sg_num, &ita_params);

    crate::utils::console::log_debug(&format!(
        "[KPATH] SC lattice params: a={:.4}, b={:.4}, c={:.4}, α={:.2}°, β={:.2}°, γ={:.2}°",
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

    // 6. Reciprocal lattice from the explicitly constructed SC primitive cell:
    //    B = 2π (P^{-1})^T, columns of P / B are direct / reciprocal vectors.
    //    Building P ourselves (rather than trusting moyo's prim_std_cell to
    //    match SC10 axis order) keeps the fractional k-point coordinates and
    //    the reciprocal basis consistent by construction.
    let prim = bravais::sc_primitive_lattice(bravais_type, &params);
    let two_pi = 2.0 * std::f64::consts::PI;
    let rec_lattice = prim.try_inverse()?.transpose() * two_pi;

    // 7. Build k-point list and path segments
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

    // 8. Compute BZ wireframe via Voronoi construction
    let bz_lines = voronoi::compute_bz_wireframe(&rec_lattice);

    crate::utils::console::log_debug(&format!(
        "[KPATH] BZ wireframe: {} edges, {} k-points on path",
        bz_lines.len(),
        linear_kpoints.len()
    ));

    Some(KPathResult {
        // Show the Hermann-Mauguin symbol, not the opaque hall number.
        spacegroup_str: format!(
            "{} ({})",
            sg_num,
            super::symmetry::spacegroup_symbol(sg_num)
        ),
        number: sg_num,
        lattice_type: kdata.label,
        kpoints: linear_kpoints,
        path_segments,
        bz_lines,
        rec_lattice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::{Atom, Structure};

    /// Build a Structure from lattice vectors (rows) and fractional coords.
    fn structure_from_frac(lat: [[f64; 3]; 3], sites: &[(&str, [f64; 3])]) -> Structure {
        let a = Vector3::from(lat[0]);
        let b = Vector3::from(lat[1]);
        let c = Vector3::from(lat[2]);
        Structure {
            lattice: lat,
            atoms: sites
                .iter()
                .enumerate()
                .map(|(i, (e, f))| {
                    let r = a * f[0] + b * f[1] + c * f[2];
                    Atom {
                        element: e.to_string(),
                        position: [r.x, r.y, r.z],
                        original_index: i,
                        oxidation: None,
                        occupancy: 1.0,
                    }
                })
                .collect(),
            formula: String::new(),
            is_periodic: true,
        }
    }

    /// Baddeleyite ZrO2, P2_1/c (#14), unique axis b, beta = 99.23 deg.
    /// Sites from Smith & Newkirk (1965); 4e orbit expanded by hand.
    fn zro2_baddeleyite() -> Structure {
        let (a, b, c) = (5.1505, 5.2116, 5.3173);
        let beta = 99.23_f64.to_radians();
        let lat = [
            [a, 0.0, 0.0],
            [0.0, b, 0.0],
            [c * beta.cos(), 0.0, c * beta.sin()],
        ];
        let base: [(&str, [f64; 3]); 3] = [
            ("Zr", [0.2758, 0.0411, 0.2082]),
            ("O", [0.0703, 0.3359, 0.3406]),
            ("O", [0.4423, 0.7549, 0.4789]),
        ];
        let mut sites: Vec<(&str, [f64; 3])> = Vec::new();
        for (e, p) in base {
            let [x, y, z] = p;
            for q in [
                [x, y, z],
                [-x, y + 0.5, -z + 0.5],
                [-x, -y, -z],
                [x, -y + 0.5, z + 0.5],
            ] {
                sites.push((e, [q[0].rem_euclid(1.0), q[1].rem_euclid(1.0), q[2].rem_euclid(1.0)]));
            }
        }
        structure_from_frac(lat, &sites)
    }

    /// Bi2Se3, R-3m (#166), hexagonal axes a = 4.143, c = 28.636.
    /// 3a (Se) + two 6c orbits (Bi z = 0.4008, Se z = 0.2117), R-centred.
    fn bi2se3() -> Structure {
        let (a, c) = (4.143, 28.636);
        let lat = [
            [a, 0.0, 0.0],
            [-a / 2.0, a * 3.0_f64.sqrt() / 2.0, 0.0],
            [0.0, 0.0, c],
        ];
        let r_shifts: [[f64; 3]; 3] = [
            [0.0, 0.0, 0.0],
            [2.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
            [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0],
        ];
        let mut sites: Vec<(&str, [f64; 3])> = Vec::new();
        let mut push = |e: &'static str, z: f64, pm: bool| {
            for s in &r_shifts {
                for zz in if pm { vec![z, -z] } else { vec![z] } {
                    sites.push((
                        e,
                        [
                            s[0].rem_euclid(1.0),
                            s[1].rem_euclid(1.0),
                            (zz + s[2]).rem_euclid(1.0),
                        ],
                    ));
                }
            }
        };
        push("Se", 0.0, false);
        push("Bi", 0.4008, true);
        push("Se", 0.2117, true);
        structure_from_frac(lat, &sites)
    }

    fn find_kp<'a>(r: &'a KPathResult, label: &str) -> &'a KPoint {
        r.kpoints
            .iter()
            .find(|k| k.label == label)
            .unwrap_or_else(|| panic!("k-point {label} not on path"))
    }

    fn assert_frac(kp: &KPoint, expect: [f64; 3], tol: f64) {
        for i in 0..3 {
            assert!(
                (kp.coords_frac[i] - expect[i]).abs() < tol,
                "{}: frac {:?} != expected {:?}",
                kp.label,
                kp.coords_frac,
                expect
            );
        }
    }

    /// Bi2Se3 (R-3m) must classify as RHL1 with the rhombohedral α ≈ 24.3°,
    /// not RHL2 from the hexagonal cell's 90° angles (KP-1).
    #[test]
    fn test_bi2se3_rhl1() {
        let r = calculate_kpath(&bi2se3()).expect("kpath failed");
        assert_eq!(r.number, 166);
        assert_eq!(r.lattice_type, "RHL1", "Bi2Se3 must be RHL1");

        // α_r = 24.30° → η = (1+4cosα)/(2+4cosα) = 0.8229, ν = 0.75-η/2 = 0.3386
        assert_frac(find_kp(&r, "L"), [0.5, 0.0, 0.0], 1e-6);
        assert_frac(find_kp(&r, "F"), [0.5, 0.5, 0.0], 1e-6);
        assert_frac(find_kp(&r, "Z"), [0.5, 0.5, 0.5], 1e-6);
        assert_frac(find_kp(&r, "P"), [0.8229, 0.3386, 0.3386], 1e-3);
    }

    /// Baddeleyite ZrO2 (P2_1/c) must classify as MCL with non-degenerate
    /// η, ν computed from the oblique angle (KP-3). With the old code
    /// (α = 90° read from the ITA cell) η = ν = 1/2 and H collapses onto C.
    #[test]
    fn test_zro2_baddeleyite_mcl() {
        let r = calculate_kpath(&zro2_baddeleyite()).expect("kpath failed");
        assert_eq!(r.number, 14);
        assert_eq!(r.lattice_type, "MCL", "ZrO2 must be MCL");

        // SC params (b,a,c,180°-β) = (5.2116, 5.1505, 5.3173, 80.77°)
        // → η = (1 - b cosα/c)/(2 sin²α) = 0.4335, ν = 0.5 - η c cosα/b = 0.4282
        let h = find_kp(&r, "H");
        assert_frac(h, [0.0, 0.4335, 0.5718], 2e-3);
        let c = find_kp(&r, "C");
        let dist: f64 = (0..3)
            .map(|i| (h.coords_frac[i] - c.coords_frac[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(dist > 0.05, "H must not collapse onto C (got dist {dist:.4})");
    }

    /// Si diamond (Fd-3m): FCC, X at 2π/a — locks the primitive/reciprocal
    /// basis construction (KP-7).
    #[test]
    fn test_si_fcc_kpath() {
        let a = 5.4309;
        let lat = [[a, 0.0, 0.0], [0.0, a, 0.0], [0.0, 0.0, a]];
        let frac: [[f64; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [0.5, 0.5, 0.0],
            [0.5, 0.0, 0.5],
            [0.0, 0.5, 0.5],
            [0.25, 0.25, 0.25],
            [0.75, 0.75, 0.25],
            [0.75, 0.25, 0.75],
            [0.25, 0.75, 0.75],
        ];
        let sites: Vec<(&str, [f64; 3])> = frac.iter().map(|f| ("Si", *f)).collect();
        let r = calculate_kpath(&structure_from_frac(lat, &sites)).expect("kpath failed");
        assert_eq!(r.number, 227);
        assert_eq!(r.lattice_type, "FCC");

        let x = find_kp(&r, "X");
        assert_frac(x, [0.5, 0.0, 0.5], 1e-6);
        let k_norm = Vector3::from(x.coords_cart).norm();
        let expect = 2.0 * std::f64::consts::PI / a;
        assert!(
            (k_norm - expect).abs() < 1e-4,
            "|X| should be 2π/a = {expect:.5}, got {k_norm:.5}"
        );
    }

    /// Mg (P6_3/mmc): HEX, |K| = 4π/(3a) — checks the hexagonal Cartesian
    /// geometry that the old transposed reciprocal basis distorted.
    #[test]
    fn test_mg_hex_kpath() {
        let (a, c) = (3.2094, 5.2108);
        let lat = [
            [a, 0.0, 0.0],
            [-a / 2.0, a * 3.0_f64.sqrt() / 2.0, 0.0],
            [0.0, 0.0, c],
        ];
        let sites: Vec<(&str, [f64; 3])> = vec![
            ("Mg", [1.0 / 3.0, 2.0 / 3.0, 0.25]),
            ("Mg", [2.0 / 3.0, 1.0 / 3.0, 0.75]),
        ];
        let r = calculate_kpath(&structure_from_frac(lat, &sites)).expect("kpath failed");
        assert_eq!(r.number, 194);
        assert_eq!(r.lattice_type, "HEX");

        let k = find_kp(&r, "K");
        let k_norm = Vector3::from(k.coords_cart).norm();
        let expect = 4.0 * std::f64::consts::PI / (3.0 * a);
        assert!(
            (k_norm - expect).abs() < 1e-4,
            "|K| should be 4π/3a = {expect:.5}, got {k_norm:.5}"
        );
    }

    /// α-U (Cmcm): ORCC with ζ from a < b (KP-5's C-centred ordering).
    #[test]
    fn test_alpha_u_orcc() {
        let (a, b, c) = (2.854, 5.869, 4.955);
        let lat = [[a, 0.0, 0.0], [0.0, b, 0.0], [0.0, 0.0, c]];
        let y = 0.1025;
        let sites: Vec<(&str, [f64; 3])> = vec![
            ("U", [0.0, y, 0.25]),
            ("U", [0.0, 1.0 - y, 0.75]),
            ("U", [0.5, y + 0.5, 0.25]),
            ("U", [0.5, 0.5 - y, 0.75]),
        ];
        let r = calculate_kpath(&structure_from_frac(lat, &sites)).expect("kpath failed");
        assert_eq!(r.number, 63);
        assert_eq!(r.lattice_type, "ORCC");

        // ζ = (1 + a²/b²)/4 = 0.30911
        assert_frac(find_kp(&r, "X"), [0.30911, 0.30911, 0.0], 2e-3);
    }
}
