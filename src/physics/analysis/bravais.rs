// src/physics/analysis/bravais.rs
//
// Runtime classification of Bravais lattice type from space group + conventional
// cell parameters, and computation of high-symmetry k-point coordinates using the
// Setyawan-Curtarolo (2010) conventions.
//
// Reference: W. Setyawan and S. Curtarolo, Comp. Mat. Sci. 49, 299 (2010).

use nalgebra::{Matrix3, Vector3};
use std::collections::HashMap;

/// The 14 Bravais lattice types, with sub-variants where k-point coordinates
/// depend on lattice parameter ratios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BravaisType {
    CUB,   // Simple cubic
    FCC,   // Face-centered cubic
    BCC,   // Body-centered cubic
    TET,   // Simple tetragonal
    BCT1,  // Body-centered tetragonal, c < a
    BCT2,  // Body-centered tetragonal, c > a
    ORC,   // Simple orthorhombic
    ORCF1, // Face-centered orthorhombic, 1/a² > 1/b² + 1/c²
    ORCF2, // Face-centered orthorhombic, 1/a² < 1/b² + 1/c²
    ORCF3, // Face-centered orthorhombic, 1/a² = 1/b² + 1/c²
    ORCI,  // Body-centered orthorhombic
    ORCC,  // Base-centered orthorhombic (C)
    HEX,   // Hexagonal
    RHL1,  // Rhombohedral, α < 90°
    RHL2,  // Rhombohedral, α > 90°
    MCL,   // Simple monoclinic
    MCLC1, // Base-centered monoclinic, kγ > 90°
    MCLC2, // Base-centered monoclinic, kγ = 90°
    MCLC3, // Base-centered monoclinic, kγ < 90°, b cos α / c + b² sin²α / a² < 1
    MCLC4, // Base-centered monoclinic, kγ < 90°, above = 1
    MCLC5, // Base-centered monoclinic, kγ < 90°, above > 1
    TRI1A, // Triclinic 1a, kα > 90°, kβ > 90°, kγ > 90°
    TRI1B, // Triclinic 1b, kα > 90°, kβ > 90°, kγ = 90°
    TRI2A, // Triclinic 2a, kα < 90°, kβ < 90°, kγ < 90°
    TRI2B, // Triclinic 2b, kα < 90°, kβ < 90°, kγ = 90°
}

/// Conventional lattice parameters extracted from a standardized cell.
#[derive(Debug, Clone)]
pub struct LatticeParams {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub alpha: f64, // in radians
    pub beta: f64,
    pub gamma: f64,
}

/// Result of Bravais classification: the type, a human-readable label,
/// computed special k-points, and the recommended path.
#[derive(Debug, Clone)]
pub struct BravaisKData {
    pub bravais_type: BravaisType,
    pub label: String,
    pub special_points: HashMap<String, [f64; 3]>,
    pub path: Vec<Vec<String>>,
}

// =========================================================================
// 1. LATTICE PARAMETER EXTRACTION
// =========================================================================

/// Extract (a, b, c, α, β, γ) from a column-major lattice matrix.
/// Columns of the matrix are the lattice vectors.
pub fn extract_lattice_params(lattice: &nalgebra::Matrix3<f64>) -> LatticeParams {
    let a_vec = lattice.column(0);
    let b_vec = lattice.column(1);
    let c_vec = lattice.column(2);

    let a = a_vec.norm();
    let b = b_vec.norm();
    let c = c_vec.norm();

    let alpha = (b_vec.dot(&c_vec) / (b * c)).clamp(-1.0, 1.0).acos();
    let beta = (a_vec.dot(&c_vec) / (a * c)).clamp(-1.0, 1.0).acos();
    let gamma = (a_vec.dot(&b_vec) / (a * b)).clamp(-1.0, 1.0).acos();

    LatticeParams {
        a,
        b,
        c,
        alpha,
        beta,
        gamma,
    }
}

// =========================================================================
// 2. BRAVAIS TYPE CLASSIFICATION
// =========================================================================

/// Centering type derived from space group number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Centering {
    P,
    F,
    I,
    A, // or C — base-centered
    R,
}

/// Crystal system from space group number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrystalSystem {
    Triclinic,
    Monoclinic,
    Orthorhombic,
    Tetragonal,
    Trigonal,
    Hexagonal,
    Cubic,
}

fn crystal_system(sg: i32) -> CrystalSystem {
    match sg {
        1..=2 => CrystalSystem::Triclinic,
        3..=15 => CrystalSystem::Monoclinic,
        16..=74 => CrystalSystem::Orthorhombic,
        75..=142 => CrystalSystem::Tetragonal,
        143..=167 => CrystalSystem::Trigonal,
        168..=194 => CrystalSystem::Hexagonal,
        195..=230 => CrystalSystem::Cubic,
        _ => CrystalSystem::Triclinic,
    }
}

/// Determine centering from the first letter of the Hermann-Mauguin symbol.
/// This is encoded by space group ranges following ITA conventions.
fn centering(sg: i32) -> Centering {
    // For trigonal with R centering
    let r_groups: &[i32] = &[146, 148, 155, 160, 161, 166, 167];
    if r_groups.contains(&sg) {
        return Centering::R;
    }

    match crystal_system(sg) {
        CrystalSystem::Cubic => {
            // Cubic: P (195-199,200-206,207-214), F (196,202,203,209,210,216,219,225-228),
            //        I (197,199,204,206,211,214,217,220,229,230)
            let f_groups: &[i32] = &[196, 202, 203, 209, 210, 216, 219, 225, 226, 227, 228];
            let i_groups: &[i32] = &[197, 199, 204, 206, 211, 214, 217, 220, 229, 230];
            if f_groups.contains(&sg) {
                Centering::F
            } else if i_groups.contains(&sg) {
                Centering::I
            } else {
                Centering::P
            }
        }
        CrystalSystem::Tetragonal => {
            // I-centered tetragonal
            let i_groups: &[i32] = &[
                79, 80, 82, 87, 88, 97, 98, 107, 108, 109, 110, 119, 120, 121, 122, 139, 140, 141,
                142,
            ];
            if i_groups.contains(&sg) {
                Centering::I
            } else {
                Centering::P
            }
        }
        CrystalSystem::Orthorhombic => {
            let f_groups: &[i32] = &[22, 42, 43, 69, 70];
            let i_groups: &[i32] = &[23, 24, 44, 45, 46, 71, 72, 73, 74];
            // C/A-centered (base-centered): remaining non-P groups
            let c_groups: &[i32] = &[20, 21, 35, 36, 37, 38, 39, 40, 41, 63, 64, 65, 66, 67, 68];
            if f_groups.contains(&sg) {
                Centering::F
            } else if i_groups.contains(&sg) {
                Centering::I
            } else if c_groups.contains(&sg) {
                Centering::A
            } else {
                Centering::P
            }
        }
        CrystalSystem::Monoclinic => {
            // C-centered monoclinic
            let c_groups: &[i32] = &[5, 8, 9, 12, 15];
            if c_groups.contains(&sg) {
                Centering::A
            } else {
                Centering::P
            }
        }
        _ => Centering::P,
    }
}

const ANGLE_TOL: f64 = 1e-5;

/// Map moyo/spglib (ITA-convention) conventional cell parameters onto the
/// Setyawan-Curtarolo convention assumed by `classify` and the k-point
/// formulas:
///   - R space groups: spglib's conventional cell is the triple hexagonal
///     cell; SC10 works in the primitive rhombohedral cell (a_r, α_r).
///   - Monoclinic: ITA standard is unique-axis-b (α = γ = 90°, β oblique);
///     SC10 puts the oblique angle in α (between b and c) with α < 90°,
///     and for primitive MCL additionally requires b ≤ c.
///   - A-centred orthorhombic (SG 38-41): cyclic axis permutation maps the
///     b-c face centring onto the a-b face (C-centring); all base-centred
///     orthorhombic cells then enforce SC10's a < b.
pub fn sc_conventional_params(sg: i32, ita: &LatticeParams) -> LatticeParams {
    let half_pi = std::f64::consts::FRAC_PI_2;
    match crystal_system(sg) {
        CrystalSystem::Trigonal if centering(sg) == Centering::R => {
            // Hexagonal (a_h, c_h) → rhombohedral (a_r, α_r):
            //   a_r = sqrt(3 a_h² + c_h²) / 3
            //   sin(α_r/2) = 3 / (2 sqrt(3 + (c_h/a_h)²))
            let (ah, ch) = (ita.a, ita.c);
            let ar = (3.0 * ah * ah + ch * ch).sqrt() / 3.0;
            let alpha_r = 2.0 * (3.0 / (2.0 * (3.0 + (ch / ah).powi(2)).sqrt())).asin();
            LatticeParams {
                a: ar,
                b: ar,
                c: ar,
                alpha: alpha_r,
                beta: alpha_r,
                gamma: alpha_r,
            }
        }
        CrystalSystem::Monoclinic => {
            // Rotate whichever angle is oblique into α = angle(b, c).
            let (a, mut b, mut c, mut alpha) = if (ita.beta - half_pi).abs() > ANGLE_TOL {
                (ita.b, ita.a, ita.c, ita.beta) // unique axis b (ITA standard)
            } else if (ita.gamma - half_pi).abs() > ANGLE_TOL {
                (ita.c, ita.a, ita.b, ita.gamma) // unique axis c
            } else {
                (ita.a, ita.b, ita.c, ita.alpha) // unique axis a (or degenerate)
            };
            // SC10 requires α < 90°; folding corresponds to flipping the sign
            // of the c vector (plus a compensating flip to keep handedness).
            if alpha > half_pi {
                alpha = std::f64::consts::PI - alpha;
            }
            // Primitive MCL additionally requires b ≤ c; a b↔c swap keeps α
            // between them. Not applied to MCLC — swapping would move the
            // centring out of the a-b plane.
            if centering(sg) == Centering::P && b > c {
                std::mem::swap(&mut b, &mut c);
            }
            LatticeParams {
                a,
                b,
                c,
                alpha,
                beta: half_pi,
                gamma: half_pi,
            }
        }
        CrystalSystem::Orthorhombic if centering(sg) == Centering::A => {
            // SG 38-41 keep true A-centring in their standard setting.
            let a_groups: &[i32] = &[38, 39, 40, 41];
            let (mut a, mut b, c) = if a_groups.contains(&sg) {
                (ita.b, ita.c, ita.a) // (a,b,c) → (b,c,a): A → C centring
            } else {
                (ita.a, ita.b, ita.c)
            };
            // SC10 ORCC requires a < b; an a↔b swap keeps the centring in
            // the a-b plane.
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            LatticeParams {
                a,
                b,
                c,
                alpha: half_pi,
                beta: half_pi,
                gamma: half_pi,
            }
        }
        _ => ita.clone(),
    }
}

/// Setyawan-Curtarolo primitive lattice vectors (matrix columns) constructed
/// explicitly from SC-convention conventional parameters (SC10 Sec. 2-5).
/// Building the reciprocal basis from this construction — rather than from
/// moyo's `prim_std_cell` — guarantees the fractional k-point coordinates and
/// the reciprocal basis can never disagree about axis conventions.
pub fn sc_primitive_lattice(btype: BravaisType, p: &LatticeParams) -> Matrix3<f64> {
    let (a, b, c) = (p.a, p.b, p.c);
    let cols: [[f64; 3]; 3] = match btype {
        BravaisType::CUB | BravaisType::TET | BravaisType::ORC => {
            [[a, 0.0, 0.0], [0.0, b, 0.0], [0.0, 0.0, c]]
        }
        BravaisType::FCC => [
            [0.0, a / 2.0, a / 2.0],
            [a / 2.0, 0.0, a / 2.0],
            [a / 2.0, a / 2.0, 0.0],
        ],
        BravaisType::BCC => [
            [-a / 2.0, a / 2.0, a / 2.0],
            [a / 2.0, -a / 2.0, a / 2.0],
            [a / 2.0, a / 2.0, -a / 2.0],
        ],
        BravaisType::BCT1 | BravaisType::BCT2 => [
            [-a / 2.0, a / 2.0, c / 2.0],
            [a / 2.0, -a / 2.0, c / 2.0],
            [a / 2.0, a / 2.0, -c / 2.0],
        ],
        BravaisType::ORCF1 | BravaisType::ORCF2 | BravaisType::ORCF3 => [
            [0.0, b / 2.0, c / 2.0],
            [a / 2.0, 0.0, c / 2.0],
            [a / 2.0, b / 2.0, 0.0],
        ],
        BravaisType::ORCI => [
            [-a / 2.0, b / 2.0, c / 2.0],
            [a / 2.0, -b / 2.0, c / 2.0],
            [a / 2.0, b / 2.0, -c / 2.0],
        ],
        BravaisType::ORCC => [
            [a / 2.0, -b / 2.0, 0.0],
            [a / 2.0, b / 2.0, 0.0],
            [0.0, 0.0, c],
        ],
        BravaisType::HEX => [
            [a / 2.0, -a * 3.0_f64.sqrt() / 2.0, 0.0],
            [a / 2.0, a * 3.0_f64.sqrt() / 2.0, 0.0],
            [0.0, 0.0, c],
        ],
        BravaisType::RHL1 | BravaisType::RHL2 => {
            let (ch, sh) = ((p.alpha / 2.0).cos(), (p.alpha / 2.0).sin());
            let z = (1.0 - p.alpha.cos().powi(2) / (ch * ch)).max(0.0).sqrt();
            [
                [a * ch, -a * sh, 0.0],
                [a * ch, a * sh, 0.0],
                [a * p.alpha.cos() / ch, 0.0, a * z],
            ]
        }
        BravaisType::MCL => [
            [a, 0.0, 0.0],
            [0.0, b, 0.0],
            [0.0, c * p.alpha.cos(), c * p.alpha.sin()],
        ],
        BravaisType::MCLC1
        | BravaisType::MCLC2
        | BravaisType::MCLC3
        | BravaisType::MCLC4
        | BravaisType::MCLC5 => [
            [a / 2.0, b / 2.0, 0.0],
            [-a / 2.0, b / 2.0, 0.0],
            [0.0, c * p.alpha.cos(), c * p.alpha.sin()],
        ],
        BravaisType::TRI1A | BravaisType::TRI1B | BravaisType::TRI2A | BravaisType::TRI2B => {
            let (ca, cb, cg) = (p.alpha.cos(), p.beta.cos(), p.gamma.cos());
            let sg_ = p.gamma.sin();
            let vol_term = (1.0 - ca * ca - cb * cb - cg * cg + 2.0 * ca * cb * cg)
                .max(0.0)
                .sqrt();
            [
                [a, 0.0, 0.0],
                [b * cg, b * sg_, 0.0],
                [c * cb, c * (ca - cb * cg) / sg_, c * vol_term / sg_],
            ]
        }
    };
    Matrix3::from_columns(&[
        Vector3::from(cols[0]),
        Vector3::from(cols[1]),
        Vector3::from(cols[2]),
    ])
}

/// Classify the full Bravais type from space group number and conventional
/// lattice parameters. `params` must already be in the SC convention
/// (see `sc_conventional_params`).
pub fn classify(sg: i32, params: &LatticeParams) -> BravaisType {
    let sys = crystal_system(sg);
    let cent = centering(sg);

    let a = params.a;
    let b = params.b;
    let c = params.c;
    let alpha = params.alpha;

    match sys {
        CrystalSystem::Cubic => match cent {
            Centering::P => BravaisType::CUB,
            Centering::F => BravaisType::FCC,
            Centering::I => BravaisType::BCC,
            _ => BravaisType::CUB,
        },
        CrystalSystem::Hexagonal => BravaisType::HEX,
        CrystalSystem::Trigonal => {
            if cent == Centering::R {
                let half_pi = std::f64::consts::FRAC_PI_2;
                if alpha < half_pi - ANGLE_TOL {
                    BravaisType::RHL1
                } else {
                    BravaisType::RHL2
                }
            } else {
                BravaisType::HEX
            }
        }
        CrystalSystem::Tetragonal => {
            if cent == Centering::I {
                if c < a - ANGLE_TOL {
                    BravaisType::BCT1
                } else {
                    BravaisType::BCT2
                }
            } else {
                BravaisType::TET
            }
        }
        CrystalSystem::Orthorhombic => match cent {
            Centering::P => BravaisType::ORC,
            Centering::F => {
                let inv_a2 = 1.0 / (a * a);
                let inv_b2 = 1.0 / (b * b);
                let inv_c2 = 1.0 / (c * c);
                let diff = inv_a2 - (inv_b2 + inv_c2);
                if diff.abs() < ANGLE_TOL {
                    BravaisType::ORCF3
                } else if diff > 0.0 {
                    BravaisType::ORCF1
                } else {
                    BravaisType::ORCF2
                }
            }
            Centering::I => BravaisType::ORCI,
            Centering::A => BravaisType::ORCC,
            _ => BravaisType::ORC,
        },
        CrystalSystem::Monoclinic => {
            if cent == Centering::P {
                BravaisType::MCL
            } else {
                // Base-centered monoclinic: classify by kγ — the γ angle of
                // the reciprocal of the MCLC *primitive* cell. For the
                // conventional cell this angle is identically 90° and would
                // classify everything as MCLC2.
                let k_gamma = mclc_kgamma(params);
                let half_pi = std::f64::consts::FRAC_PI_2;
                if k_gamma > half_pi + ANGLE_TOL {
                    BravaisType::MCLC1
                } else if (k_gamma - half_pi).abs() < ANGLE_TOL {
                    BravaisType::MCLC2
                } else {
                    // kγ < 90°: sub-classify by parameter condition
                    let cond = mclc_condition(params);
                    if (cond - 1.0).abs() < ANGLE_TOL {
                        BravaisType::MCLC4
                    } else if cond < 1.0 {
                        BravaisType::MCLC3
                    } else {
                        BravaisType::MCLC5
                    }
                }
            }
        }
        CrystalSystem::Triclinic => {
            let (ka, kb, kg) = reciprocal_angles(params);
            let half_pi = std::f64::consts::FRAC_PI_2;
            // SC10's TRI1/TRI2 assignment assumes a reduced cell in which
            // all reciprocal angles lie on one side of 90°. Mixed-sign
            // angles mean the cell is not in that form (no Niggli
            // reduction is performed here) — say so instead of silently
            // classifying as TRI2A.
            let above = [ka, kb, kg]
                .iter()
                .filter(|&&x| x > half_pi + ANGLE_TOL)
                .count();
            let below = [ka, kb, kg]
                .iter()
                .filter(|&&x| x < half_pi - ANGLE_TOL)
                .count();
            if above > 0 && below > 0 {
                crate::utils::console::log_warn(
                    "Triclinic cell has reciprocal angles on both sides of 90° (not SC10-reduced) — k-path labels are approximate",
                );
            }
            // Type 1: all reciprocal angles > 90° (or = 90°)
            // Type 2: all reciprocal angles < 90° (or = 90°)
            if ka > half_pi - ANGLE_TOL && kb > half_pi - ANGLE_TOL && kg > half_pi - ANGLE_TOL {
                if (ka - half_pi).abs() < ANGLE_TOL
                    || (kb - half_pi).abs() < ANGLE_TOL
                    || (kg - half_pi).abs() < ANGLE_TOL
                {
                    BravaisType::TRI1B
                } else {
                    BravaisType::TRI1A
                }
            } else if (ka - half_pi).abs() < ANGLE_TOL
                || (kb - half_pi).abs() < ANGLE_TOL
                || (kg - half_pi).abs() < ANGLE_TOL
            {
                BravaisType::TRI2B
            } else {
                BravaisType::TRI2A
            }
        }
    }
}

/// Compute reciprocal lattice angles (kα, kβ, kγ) from direct lattice params.
fn reciprocal_angles(p: &LatticeParams) -> (f64, f64, f64) {
    let (sa, ca) = (p.alpha.sin(), p.alpha.cos());
    let (sb, cb) = (p.beta.sin(), p.beta.cos());
    let (_sg, cg) = (p.gamma.sin(), p.gamma.cos());

    let k_alpha = ((cb * cg - ca) / (sb * p.gamma.sin()))
        .clamp(-1.0, 1.0)
        .acos();
    let k_beta = ((ca * cg - cb) / (sa * p.gamma.sin()))
        .clamp(-1.0, 1.0)
        .acos();
    let k_gamma = ((ca * cb - cg) / (sa * sb)).clamp(-1.0, 1.0).acos();

    (k_alpha, k_beta, k_gamma)
}

/// kγ for MCLC classification: the γ angle of the reciprocal lattice of the
/// SC MCLC primitive cell (this is also pymatgen's definition:
/// `primitive.reciprocal_lattice.parameters[5]`).
fn mclc_kgamma(p: &LatticeParams) -> f64 {
    let prim = sc_primitive_lattice(BravaisType::MCLC1, p);
    let rec = prim
        .try_inverse()
        .unwrap_or_else(Matrix3::identity)
        .transpose();
    let b1 = rec.column(0);
    let b2 = rec.column(1);
    (b1.dot(&b2) / (b1.norm() * b2.norm()))
        .clamp(-1.0, 1.0)
        .acos()
}

/// MCLC sub-classification condition:
/// b cos α / c + b² sin²α / a²
fn mclc_condition(p: &LatticeParams) -> f64 {
    let (sa, ca) = (p.alpha.sin(), p.alpha.cos());
    (p.b * ca / p.c) + (p.b * p.b * sa * sa / (p.a * p.a))
}

// =========================================================================
// 3. K-POINT AND PATH COMPUTATION (Setyawan-Curtarolo 2010)
// =========================================================================

/// Compute the full BravaisKData (special points + path) for the given type
/// and lattice parameters. All k-point coordinates are in fractional
/// reciprocal-lattice coordinates.
pub fn compute_kdata(btype: BravaisType, params: &LatticeParams) -> BravaisKData {
    let (label, pts, path) = match btype {
        BravaisType::CUB => kpoints_cub(),
        BravaisType::FCC => kpoints_fcc(),
        BravaisType::BCC => kpoints_bcc(),
        BravaisType::TET => kpoints_tet(),
        BravaisType::BCT1 => kpoints_bct1(params),
        BravaisType::BCT2 => kpoints_bct2(params),
        BravaisType::ORC => kpoints_orc(),
        BravaisType::ORCF1 => kpoints_orcf1(params),
        BravaisType::ORCF2 => kpoints_orcf2(params),
        BravaisType::ORCF3 => kpoints_orcf3(params),
        BravaisType::ORCI => kpoints_orci(params),
        BravaisType::ORCC => kpoints_orcc(params),
        BravaisType::HEX => kpoints_hex(),
        BravaisType::RHL1 => kpoints_rhl1(params),
        BravaisType::RHL2 => kpoints_rhl2(params),
        BravaisType::MCL => kpoints_mcl(params),
        BravaisType::MCLC1 => kpoints_mclc1(params),
        BravaisType::MCLC2 => kpoints_mclc2(params),
        BravaisType::MCLC3 => kpoints_mclc3(params),
        BravaisType::MCLC4 => kpoints_mclc4(params),
        BravaisType::MCLC5 => kpoints_mclc5(params),
        BravaisType::TRI1A | BravaisType::TRI1B => kpoints_tri1(),
        BravaisType::TRI2A | BravaisType::TRI2B => kpoints_tri2(),
    };

    BravaisKData {
        bravais_type: btype,
        label: label.to_string(),
        special_points: pts,
        path,
    }
}

// ---------------------------------------------------------------------------
// Helper: shorthand to build HashMap + path from arrays
// ---------------------------------------------------------------------------
type KData = (&'static str, HashMap<String, [f64; 3]>, Vec<Vec<String>>);

fn pts(entries: &[(&str, [f64; 3])]) -> HashMap<String, [f64; 3]> {
    entries.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn seg(labels: &[&str]) -> Vec<String> {
    labels.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// CUB — Simple Cubic
// ---------------------------------------------------------------------------
fn kpoints_cub() -> KData {
    (
        "CUB",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("X", [0.0, 0.5, 0.0]),
            ("M", [0.5, 0.5, 0.0]),
            ("R", [0.5, 0.5, 0.5]),
        ]),
        vec![seg(&["Γ", "X", "M", "Γ", "R", "X"]), seg(&["M", "R"])],
    )
}

// ---------------------------------------------------------------------------
// FCC — Face-Centered Cubic
// ---------------------------------------------------------------------------
fn kpoints_fcc() -> KData {
    (
        "FCC",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("K", [3.0 / 8.0, 3.0 / 8.0, 3.0 / 4.0]),
            ("L", [0.5, 0.5, 0.5]),
            ("U", [5.0 / 8.0, 1.0 / 4.0, 5.0 / 8.0]),
            ("W", [0.5, 1.0 / 4.0, 3.0 / 4.0]),
            ("X", [0.5, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "X", "W", "K", "Γ", "L", "U", "W", "L", "K"]),
            seg(&["U", "X"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// BCC — Body-Centered Cubic
// ---------------------------------------------------------------------------
fn kpoints_bcc() -> KData {
    (
        "BCC",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("H", [0.5, -0.5, 0.5]),
            ("P", [0.25, 0.25, 0.25]),
            ("N", [0.0, 0.0, 0.5]),
        ]),
        vec![seg(&["Γ", "H", "N", "Γ", "P", "H"]), seg(&["P", "N"])],
    )
}

// ---------------------------------------------------------------------------
// TET — Simple Tetragonal
// ---------------------------------------------------------------------------
fn kpoints_tet() -> KData {
    (
        "TET",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("A", [0.5, 0.5, 0.5]),
            ("M", [0.5, 0.5, 0.0]),
            ("R", [0.0, 0.5, 0.5]),
            ("X", [0.0, 0.5, 0.0]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "X", "M", "Γ", "Z", "R", "A", "Z"]),
            seg(&["X", "R"]),
            seg(&["M", "A"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// BCT1 — Body-Centered Tetragonal, c < a
// ---------------------------------------------------------------------------
fn kpoints_bct1(p: &LatticeParams) -> KData {
    let eta = (1.0 + p.c * p.c / (p.a * p.a)) / 4.0;
    (
        "BCT1",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("M", [-0.5, 0.5, 0.5]),
            ("N", [0.0, 0.5, 0.0]),
            ("P", [0.25, 0.25, 0.25]),
            ("X", [0.0, 0.0, 0.5]),
            ("Z", [eta, eta, -eta]),
            ("Z₁", [-eta, 1.0 - eta, eta]),
        ]),
        vec![
            seg(&["Γ", "X", "M", "Γ", "Z", "P", "N", "Z₁", "M"]),
            seg(&["X", "P"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// BCT2 — Body-Centered Tetragonal, c > a
// ---------------------------------------------------------------------------
fn kpoints_bct2(p: &LatticeParams) -> KData {
    let eta = (1.0 + p.a * p.a / (p.c * p.c)) / 4.0;
    let zeta = p.a * p.a / (2.0 * p.c * p.c);
    (
        "BCT2",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("N", [0.0, 0.5, 0.0]),
            ("P", [0.25, 0.25, 0.25]),
            ("Σ", [-eta, eta, eta]),
            ("Σ₁", [eta, 1.0 - eta, -eta]),
            ("X", [0.0, 0.0, 0.5]),
            ("Y", [-zeta, zeta, 0.5]),
            ("Y₁", [0.5, 0.5, -zeta]),
            ("Z", [0.5, 0.5, -0.5]),
        ]),
        vec![
            seg(&["Γ", "X", "Y", "Σ", "Γ", "Z", "Σ₁", "N", "P", "Y₁", "Z"]),
            seg(&["X", "P"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// ORC — Simple Orthorhombic
// ---------------------------------------------------------------------------
fn kpoints_orc() -> KData {
    (
        "ORC",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("R", [0.5, 0.5, 0.5]),
            ("S", [0.5, 0.5, 0.0]),
            ("T", [0.0, 0.5, 0.5]),
            ("U", [0.5, 0.0, 0.5]),
            ("X", [0.5, 0.0, 0.0]),
            ("Y", [0.0, 0.5, 0.0]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "X", "S", "Y", "Γ", "Z", "U", "R", "T", "Z"]),
            seg(&["Y", "T"]),
            seg(&["U", "X"]),
            seg(&["S", "R"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// ORCF1 — Face-Centered Orthorhombic, 1/a² > 1/b² + 1/c²
// ---------------------------------------------------------------------------
fn kpoints_orcf1(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let c2 = p.c * p.c;
    let zeta = (1.0 + a2 / b2 - a2 / c2) / 4.0;
    let eta = (1.0 + a2 / b2 + a2 / c2) / 4.0;
    (
        "ORCF1",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("A", [0.5, 0.5 + zeta, zeta]),
            ("A₁", [0.5, 0.5 - zeta, 1.0 - zeta]),
            ("L", [0.5, 0.5, 0.5]),
            ("T", [1.0, 0.5, 0.5]),
            ("X", [0.0, eta, eta]),
            ("X₁", [1.0, 1.0 - eta, 1.0 - eta]),
            ("Y", [0.5, 0.0, 0.5]),
            ("Z", [0.5, 0.5, 0.0]),
        ]),
        vec![
            seg(&["Γ", "Y", "T", "Z", "Γ", "X", "A₁", "Y"]),
            seg(&["T", "X₁"]),
            seg(&["X", "A", "Z"]),
            seg(&["L", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// ORCF2 — Face-Centered Orthorhombic, 1/a² < 1/b² + 1/c²
// ---------------------------------------------------------------------------
fn kpoints_orcf2(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let c2 = p.c * p.c;
    let phi = (1.0 + c2 / b2 - c2 / a2) / 4.0;
    let eta = (1.0 + a2 / b2 - a2 / c2) / 4.0;
    let delta = (1.0 + b2 / a2 - b2 / c2) / 4.0;
    (
        "ORCF2",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("C", [0.5, 0.5 - eta, 1.0 - eta]),
            ("C₁", [0.5, 0.5 + eta, eta]),
            ("D", [0.5 - delta, 0.5, 1.0 - delta]),
            ("D₁", [0.5 + delta, 0.5, delta]),
            ("L", [0.5, 0.5, 0.5]),
            ("H", [1.0 - phi, 0.5 - phi, 0.5]),
            ("H₁", [phi, 0.5 + phi, 0.5]),
            ("X", [0.0, 0.5, 0.5]),
            ("Y", [0.5, 0.0, 0.5]),
            ("Z", [0.5, 0.5, 0.0]),
        ]),
        vec![
            seg(&["Γ", "Y", "C", "D", "X", "Γ", "Z", "D₁", "H", "C"]),
            seg(&["C₁", "Z"]),
            seg(&["X", "H₁"]),
            seg(&["H", "Y"]),
            seg(&["L", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// ORCF3 — Face-Centered Orthorhombic, 1/a² = 1/b² + 1/c²
// ---------------------------------------------------------------------------
fn kpoints_orcf3(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let c2 = p.c * p.c;
    let zeta = (1.0 + a2 / b2 - a2 / c2) / 4.0;
    let eta = (1.0 + a2 / b2 + a2 / c2) / 4.0;
    (
        "ORCF3",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("A", [0.5, 0.5 + zeta, zeta]),
            ("A₁", [0.5, 0.5 - zeta, 1.0 - zeta]),
            ("L", [0.5, 0.5, 0.5]),
            ("T", [1.0, 0.5, 0.5]),
            ("X", [0.0, eta, eta]),
            ("X₁", [1.0, 1.0 - eta, 1.0 - eta]),
            ("Y", [0.5, 0.0, 0.5]),
            ("Z", [0.5, 0.5, 0.0]),
        ]),
        vec![
            seg(&["Γ", "Y", "T", "Z", "Γ", "X", "A₁", "Y"]),
            seg(&["X", "A", "Z"]),
            seg(&["L", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// ORCI — Body-Centered Orthorhombic
// ---------------------------------------------------------------------------
fn kpoints_orci(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let c2 = p.c * p.c;
    let zeta = (1.0 + a2 / c2) / 4.0;
    let eta = (1.0 + b2 / c2) / 4.0;
    let delta = (b2 - a2) / (4.0 * c2);
    let mu = (a2 + b2) / (4.0 * c2);
    (
        "ORCI",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("L", [-mu, mu, 0.5 - delta]),
            ("L₁", [mu, -mu, 0.5 + delta]),
            ("L₂", [0.5 - delta, 0.5 + delta, -mu]),
            ("R", [0.0, 0.5, 0.0]),
            ("S", [0.5, 0.0, 0.0]),
            ("T", [0.0, 0.0, 0.5]),
            ("W", [0.25, 0.25, 0.25]),
            ("X", [-zeta, zeta, zeta]),
            ("X₁", [zeta, 1.0 - zeta, -zeta]),
            ("Y", [eta, -eta, eta]),
            ("Y₁", [1.0 - eta, eta, -eta]),
            ("Z", [0.5, 0.5, -0.5]),
        ]),
        vec![
            seg(&["Γ", "X", "L", "T", "W", "R", "X₁", "Z", "Γ", "Y", "S", "W"]),
            seg(&["L₁", "Y"]),
            seg(&["Y₁", "Z"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// ORCC — Base-Centered Orthorhombic
// ---------------------------------------------------------------------------
fn kpoints_orcc(p: &LatticeParams) -> KData {
    let zeta = (1.0 + p.a * p.a / (p.b * p.b)) / 4.0;
    (
        "ORCC",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("A", [zeta, zeta, 0.5]),
            ("A₁", [-zeta, 1.0 - zeta, 0.5]),
            ("R", [0.0, 0.5, 0.5]),
            ("S", [0.0, 0.5, 0.0]),
            ("T", [-0.5, 0.5, 0.5]),
            ("X", [zeta, zeta, 0.0]),
            ("X₁", [-zeta, 1.0 - zeta, 0.0]),
            ("Y", [-0.5, 0.5, 0.0]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "X", "S", "R", "A", "Z", "Γ", "Y", "X₁", "A₁", "T", "Y"]),
            seg(&["Z", "T"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// HEX — Hexagonal
// ---------------------------------------------------------------------------
fn kpoints_hex() -> KData {
    (
        "HEX",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("A", [0.0, 0.0, 0.5]),
            ("H", [1.0 / 3.0, 1.0 / 3.0, 0.5]),
            ("K", [1.0 / 3.0, 1.0 / 3.0, 0.0]),
            ("L", [0.5, 0.0, 0.5]),
            ("M", [0.5, 0.0, 0.0]),
        ]),
        vec![
            seg(&["Γ", "M", "K", "Γ", "A", "L", "H", "A"]),
            seg(&["L", "M"]),
            seg(&["K", "H"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// RHL1 — Rhombohedral, α < 90°
// ---------------------------------------------------------------------------
fn kpoints_rhl1(p: &LatticeParams) -> KData {
    let ca = p.alpha.cos();
    let eta = (1.0 + 4.0 * ca) / (2.0 + 4.0 * ca);
    let nu = 0.75 - eta / 2.0;
    (
        "RHL1",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("B", [eta, 0.5, 1.0 - eta]),
            ("B₁", [0.5, 1.0 - eta, eta - 1.0]),
            ("F", [0.5, 0.5, 0.0]),
            ("L", [0.5, 0.0, 0.0]),
            ("L₁", [0.0, 0.0, -0.5]),
            ("P", [eta, nu, nu]),
            ("P₁", [1.0 - nu, 1.0 - nu, 1.0 - eta]),
            ("P₂", [nu, nu, eta - 1.0]),
            ("Q", [1.0 - nu, nu, 0.0]),
            ("X", [nu, 0.0, -nu]),
            ("Z", [0.5, 0.5, 0.5]),
        ]),
        vec![
            seg(&["Γ", "L", "B₁"]),
            seg(&["B", "Z", "Γ", "X"]),
            seg(&["Q", "F", "P₁", "Z"]),
            seg(&["L", "P"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// RHL2 — Rhombohedral, α > 90°
// ---------------------------------------------------------------------------
fn kpoints_rhl2(p: &LatticeParams) -> KData {
    // SC10: η = 1 / (2 tan²(α/2))
    let eta = 1.0 / (2.0 * (p.alpha / 2.0).tan().powi(2));
    let nu = 0.75 - eta / 2.0;
    (
        "RHL2",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("F", [0.5, -0.5, 0.0]),
            ("L", [0.5, 0.0, 0.0]),
            ("P", [1.0 - nu, -nu, 1.0 - nu]),
            ("P₁", [nu, nu - 1.0, nu - 1.0]),
            ("Q", [eta, eta, eta]),
            ("Q₁", [1.0 - eta, -eta, -eta]),
            ("Z", [0.5, -0.5, 0.5]),
        ]),
        vec![seg(&["Γ", "P", "Z", "Q", "Γ", "F", "P₁", "Q₁", "L", "Z"])],
    )
}

// ---------------------------------------------------------------------------
// MCL — Simple Monoclinic
// ---------------------------------------------------------------------------
fn kpoints_mcl(p: &LatticeParams) -> KData {
    let ca = p.alpha.cos();
    let eta = (1.0 - p.b * ca / p.c) / (2.0 * p.alpha.sin() * p.alpha.sin());
    let nu = 0.5 - eta * p.c * ca / p.b;
    (
        "MCL",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("A", [0.5, 0.5, 0.0]),
            ("C", [0.0, 0.5, 0.5]),
            ("D", [0.5, 0.0, 0.5]),
            ("D₁", [0.5, 0.0, -0.5]),
            ("E", [0.5, 0.5, 0.5]),
            ("H", [0.0, eta, 1.0 - nu]),
            ("H₁", [0.0, 1.0 - eta, nu]),
            ("H₂", [0.0, eta, -nu]),
            ("M", [0.5, eta, 1.0 - nu]),
            ("M₁", [0.5, 1.0 - eta, nu]),
            ("M₂", [0.5, eta, -nu]),
            ("X", [0.0, 0.5, 0.0]),
            ("Y", [0.0, 0.0, 0.5]),
            ("Y₁", [0.0, 0.0, -0.5]),
            ("Z", [0.5, 0.0, 0.0]),
        ]),
        vec![
            seg(&["Γ", "Y", "H", "C", "E", "M₁", "A", "X", "H₁"]),
            seg(&["M", "D", "Z"]),
            seg(&["Y", "D"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// MCLC1 — Base-Centered Monoclinic, kγ > 90°
// ---------------------------------------------------------------------------
fn kpoints_mclc1(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let ca = p.alpha.cos();
    let sa2 = p.alpha.sin() * p.alpha.sin();

    let zeta = (2.0 - p.b * ca / p.c) / (4.0 * sa2);
    let eta = 0.5 + 2.0 * zeta * p.c * ca / p.b;
    let psi = 0.75 - a2 / (4.0 * b2 * sa2);
    let phi = psi + (0.75 - psi) * p.b * ca / p.c;

    (
        "MCLC1",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("N", [0.5, 0.0, 0.0]),
            ("N₁", [0.0, -0.5, 0.0]),
            ("F", [1.0 - zeta, 1.0 - zeta, 1.0 - eta]),
            ("F₁", [zeta, zeta, eta]),
            ("F₂", [-zeta, -zeta, 1.0 - eta]),
            ("I", [phi, 1.0 - phi, 0.5]),
            ("I₁", [1.0 - phi, phi - 1.0, 0.5]),
            ("L", [0.5, 0.5, 0.5]),
            ("M", [0.5, 0.0, 0.5]),
            ("X", [1.0 - psi, psi - 1.0, 0.0]),
            ("X₁", [psi, 1.0 - psi, 0.0]),
            ("X₂", [psi - 1.0, -psi, 0.0]),
            ("Y", [0.5, 0.5, 0.0]),
            ("Y₁", [-0.5, -0.5, 0.0]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "Y", "F", "L", "I"]),
            seg(&["I₁", "Z", "F₁"]),
            seg(&["Y", "X₁"]),
            seg(&["X", "Γ", "N"]),
            seg(&["M", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// MCLC2 — Base-Centered Monoclinic, kγ = 90°
// ---------------------------------------------------------------------------
fn kpoints_mclc2(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let ca = p.alpha.cos();
    let sa2 = p.alpha.sin() * p.alpha.sin();

    let zeta = (2.0 - p.b * ca / p.c) / (4.0 * sa2);
    let eta = 0.5 + 2.0 * zeta * p.c * ca / p.b;
    let psi = 0.75 - a2 / (4.0 * b2 * sa2);
    let phi = psi + (0.75 - psi) * p.b * ca / p.c;

    (
        "MCLC2",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("N", [0.5, 0.0, 0.0]),
            ("N₁", [0.0, -0.5, 0.0]),
            ("F", [1.0 - zeta, 1.0 - zeta, 1.0 - eta]),
            ("F₁", [zeta, zeta, eta]),
            ("F₂", [-zeta, -zeta, 1.0 - eta]),
            ("F₃", [1.0 - zeta, -zeta, 1.0 - eta]),
            ("I", [phi, 1.0 - phi, 0.5]),
            ("I₁", [1.0 - phi, phi - 1.0, 0.5]),
            ("L", [0.5, 0.5, 0.5]),
            ("M", [0.5, 0.0, 0.5]),
            ("X", [1.0 - psi, psi - 1.0, 0.0]),
            ("X₁", [psi, 1.0 - psi, 0.0]),
            ("X₂", [psi - 1.0, -psi, 0.0]),
            ("Y", [0.5, 0.5, 0.0]),
            ("Y₁", [-0.5, -0.5, 0.0]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "Y", "F", "L", "I"]),
            seg(&["I₁", "Z", "F₁"]),
            seg(&["N", "Γ", "M"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// MCLC3 — Base-Centered Monoclinic, kγ < 90°, condition < 1
// ---------------------------------------------------------------------------
fn kpoints_mclc3(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let ca = p.alpha.cos();
    let sa2 = p.alpha.sin() * p.alpha.sin();

    // SC10: μ = (1 + b²/a²)/4
    let mu = (1.0 + b2 / a2) / 4.0;
    let delta = p.b * p.c * ca / (2.0 * p.a * p.a);
    let zeta = mu - 0.25 + (1.0 - p.b * ca / p.c) / (4.0 * sa2);
    let eta = 0.5 + 2.0 * zeta * p.c * ca / p.b;
    let phi = 1.0 + zeta - 2.0 * mu;
    let psi = eta - 2.0 * delta;

    (
        "MCLC3",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("F", [1.0 - phi, 1.0 - phi, 1.0 - psi]),
            ("F₁", [phi, phi - 1.0, psi]),
            ("F₂", [1.0 - phi, -phi, 1.0 - psi]),
            ("H", [zeta, zeta, eta]),
            ("H₁", [1.0 - zeta, -zeta, 1.0 - eta]),
            ("H₂", [-zeta, -zeta, 1.0 - eta]),
            ("I", [0.5, -0.5, 0.5]),
            ("M", [0.5, 0.0, 0.5]),
            ("N", [0.5, 0.0, 0.0]),
            ("N₁", [0.0, -0.5, 0.0]),
            ("X", [0.5, -0.5, 0.0]),
            ("Y", [mu, mu, delta]),
            ("Y₁", [1.0 - mu, -mu, -delta]),
            ("Y₂", [-mu, -mu, -delta]),
            ("Y₃", [mu, mu - 1.0, delta]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "Y", "F", "H", "Z", "I"]),
            seg(&["H₁", "Y₁", "X", "Γ", "N"]),
            seg(&["M", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// MCLC4 — Base-Centered Monoclinic, kγ < 90°, condition = 1
// ---------------------------------------------------------------------------
fn kpoints_mclc4(p: &LatticeParams) -> KData {
    // Same formulas as MCLC3 at the boundary
    kpoints_mclc3(p)
}

// ---------------------------------------------------------------------------
// MCLC5 — Base-Centered Monoclinic, kγ < 90°, condition > 1
// ---------------------------------------------------------------------------
fn kpoints_mclc5(p: &LatticeParams) -> KData {
    let a2 = p.a * p.a;
    let b2 = p.b * p.b;
    let ca = p.alpha.cos();
    let sa2 = p.alpha.sin() * p.alpha.sin();

    // SC10: ζ = (b²/a² + (1 − b cosα/c)/sin²α)/4,
    //       μ = η/2 + b²/(4a²) − b c cosα/(2a²)
    let zeta = (b2 / a2 + (1.0 - p.b * ca / p.c) / sa2) / 4.0;
    let eta = 0.5 + 2.0 * zeta * p.c * ca / p.b;
    let mu = eta / 2.0 + b2 / (4.0 * a2) - p.b * p.c * ca / (2.0 * a2);
    let nu = 2.0 * mu - zeta;
    let omega = (4.0 * nu - 1.0 - b2 * sa2 / (p.a * p.a)) * p.c / (2.0 * p.b * ca);
    let delta = zeta * p.c * ca / p.b + omega / 2.0 - 0.25;
    let rho = 1.0 - zeta * p.a * p.a / (p.b * p.b);

    (
        "MCLC5",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("F", [nu, nu, omega]),
            ("F₁", [1.0 - nu, 1.0 - nu, 1.0 - omega]),
            ("F₂", [nu, nu - 1.0, omega]),
            ("H", [zeta, zeta, eta]),
            ("H₁", [1.0 - zeta, -zeta, 1.0 - eta]),
            ("H₂", [-zeta, -zeta, 1.0 - eta]),
            ("I", [rho, 1.0 - rho, 0.5]),
            ("I₁", [1.0 - rho, rho - 1.0, 0.5]),
            ("L", [0.5, 0.5, 0.5]),
            ("M", [0.5, 0.0, 0.5]),
            ("N", [0.5, 0.0, 0.0]),
            ("N₁", [0.0, -0.5, 0.0]),
            ("X", [0.5, -0.5, 0.0]),
            ("Y", [mu, mu, delta]),
            ("Y₁", [1.0 - mu, -mu, -delta]),
            ("Y₂", [-mu, -mu, -delta]),
            ("Y₃", [mu, mu - 1.0, delta]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["Γ", "Y", "F", "L", "I"]),
            seg(&["I₁", "Z", "H", "F₁"]),
            seg(&["H₁", "Y₁", "X", "Γ", "N"]),
            seg(&["M", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// TRI1 — Triclinic, all reciprocal angles > 90°
// ---------------------------------------------------------------------------
fn kpoints_tri1() -> KData {
    (
        "TRI1",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("L", [0.5, 0.5, 0.0]),
            ("M", [0.0, 0.5, 0.5]),
            ("N", [0.5, 0.0, 0.5]),
            ("R", [0.5, 0.5, 0.5]),
            ("X", [0.5, 0.0, 0.0]),
            ("Y", [0.0, 0.5, 0.0]),
            ("Z", [0.0, 0.0, 0.5]),
        ]),
        vec![
            seg(&["X", "Γ", "Y"]),
            seg(&["L", "Γ", "Z"]),
            seg(&["N", "Γ", "M"]),
            seg(&["R", "Γ"]),
        ],
    )
}

// ---------------------------------------------------------------------------
// TRI2 — Triclinic, all reciprocal angles < 90°
// ---------------------------------------------------------------------------
fn kpoints_tri2() -> KData {
    (
        "TRI2",
        pts(&[
            ("Γ", [0.0, 0.0, 0.0]),
            ("L", [0.5, -0.5, 0.0]),
            ("M", [0.0, 0.0, 0.5]),
            ("N", [-0.5, -0.5, 0.5]),
            ("R", [0.0, -0.5, 0.5]),
            ("X", [0.0, -0.5, 0.0]),
            ("Y", [0.5, 0.0, 0.0]),
            ("Z", [-0.5, 0.0, 0.5]),
        ]),
        vec![
            seg(&["X", "Γ", "Y"]),
            seg(&["L", "Γ", "Z"]),
            seg(&["N", "Γ", "M"]),
            seg(&["R", "Γ"]),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(a: f64, b: f64, c: f64, al: f64, be: f64, ga: f64) -> LatticeParams {
        LatticeParams {
            a,
            b,
            c,
            alpha: al.to_radians(),
            beta: be.to_radians(),
            gamma: ga.to_radians(),
        }
    }

    /// KP-1: hexagonal → rhombohedral parameter conversion for R groups.
    #[test]
    fn test_sc_params_rhombohedral() {
        // Bi2Se3: a_h = 4.143, c_h = 28.636 → a_r = 9.8403, α_r = 24.30°
        let sc = sc_conventional_params(166, &params(4.143, 4.143, 28.636, 90.0, 90.0, 120.0));
        assert!((sc.a - 9.8403).abs() < 1e-3, "a_r = {}", sc.a);
        assert!((sc.alpha.to_degrees() - 24.30).abs() < 0.02, "α_r = {}", sc.alpha.to_degrees());
        assert_eq!(classify(166, &sc), BravaisType::RHL1);
    }

    /// KP-3: ITA unique-axis-b monoclinic → SC oblique-α mapping.
    #[test]
    fn test_sc_params_monoclinic() {
        // ZrO2-like: β = 99.23° oblique → α_SC = 80.77°, axes (b, a, c)
        let sc = sc_conventional_params(14, &params(5.1505, 5.2116, 5.3173, 90.0, 99.23, 90.0));
        assert!((sc.a - 5.2116).abs() < 1e-9);
        assert!((sc.b - 5.1505).abs() < 1e-9);
        assert!((sc.c - 5.3173).abs() < 1e-9);
        assert!((sc.alpha.to_degrees() - 80.77).abs() < 1e-9);
        assert!((sc.beta.to_degrees() - 90.0).abs() < 1e-9);
        assert_eq!(classify(14, &sc), BravaisType::MCL);

        // MCL b ≤ c enforcement: mapped b > c must swap
        let sc = sc_conventional_params(4, &params(7.0, 6.0, 5.0, 90.0, 100.0, 90.0));
        assert!((sc.a - 6.0).abs() < 1e-9);
        assert!((sc.b - 5.0).abs() < 1e-9);
        assert!((sc.c - 7.0).abs() < 1e-9);
    }

    /// KP-5: A-centred orthorhombic (SG 38-41) permutes to C-centring,
    /// and base-centred cells enforce a < b.
    #[test]
    fn test_sc_params_a_centered_orthorhombic() {
        // Amm2 (#38): (a,b,c) → (b,c,a)
        let sc = sc_conventional_params(38, &params(8.0, 4.0, 6.0, 90.0, 90.0, 90.0));
        assert_eq!((sc.a, sc.b, sc.c), (4.0, 6.0, 8.0));

        // Permutation then a↔b swap when a > b
        let sc = sc_conventional_params(38, &params(8.0, 7.0, 4.0, 90.0, 90.0, 90.0));
        assert_eq!((sc.a, sc.b, sc.c), (4.0, 7.0, 8.0));

        // Plain C-centred (#63) with a > b swaps
        let sc = sc_conventional_params(63, &params(6.0, 4.0, 5.0, 90.0, 90.0, 90.0));
        assert_eq!((sc.a, sc.b, sc.c), (4.0, 6.0, 5.0));
    }

    /// KP-2: RHL2 η = 1/(2 tan²(α/2)); α = 100° → η = 0.35205, ν = 0.57398.
    #[test]
    fn test_rhl2_eta() {
        let p = params(5.0, 5.0, 5.0, 100.0, 100.0, 100.0);
        let (_, pts, _) = kpoints_rhl2(&p);
        let q = pts["Q"];
        assert!((q[0] - 0.35205).abs() < 1e-4, "η = {}", q[0]);
        let pp = pts["P"];
        assert!((pp[0] - (1.0 - 0.57398)).abs() < 1e-4, "1-ν = {}", pp[0]);
    }

    /// KP-6: ORCI L₂ = (½−δ, ½+δ, −μ).
    #[test]
    fn test_orci_l2() {
        let p = params(3.0, 4.0, 5.0, 90.0, 90.0, 90.0);
        // δ = (b²−a²)/4c² = 0.07, μ = (a²+b²)/4c² = 0.25
        let (_, pts, _) = kpoints_orci(&p);
        let l2 = pts["L₂"];
        assert!((l2[0] - 0.43).abs() < 1e-9);
        assert!((l2[1] - 0.57).abs() < 1e-9);
        assert!((l2[2] - (-0.25)).abs() < 1e-9);
    }

    /// KP-4: MCLC sub-classification must use kγ of the primitive reciprocal
    /// cell; the branches MCLC1/3/5 must all be reachable.
    #[test]
    fn test_mclc_classification_branches() {
        // kγ > 90° ⟺ a < b sinα (from the MCLC primitive construction)
        let p1 = params(3.0, 8.0, 9.0, 80.0, 90.0, 90.0);
        assert_eq!(classify(15, &p1), BravaisType::MCLC1);

        // kγ < 90°, condition b cosα/c + b²sin²α/a² = 0.194 < 1 → MCLC3
        let p3 = params(8.0, 3.0, 9.0, 80.0, 90.0, 90.0);
        assert_eq!(classify(15, &p3), BravaisType::MCLC3);

        // kγ < 90°, condition = 1.0385 > 1 → MCLC5
        let p5 = params(5.0, 4.9, 5.0, 85.0, 90.0, 90.0);
        assert_eq!(classify(15, &p5), BravaisType::MCLC5);
    }

    /// MCLC3 μ = (1 + b²/a²)/4 (was b²/c²) — checked via Y = (μ, μ, δ).
    #[test]
    fn test_mclc3_mu() {
        let p = params(8.0, 3.0, 9.0, 80.0, 90.0, 90.0);
        // μ = (1 + 9/64)/4 = 0.285156, δ = bc·cosα/(2a²) = 0.036629
        let (_, pts, _) = kpoints_mclc3(&p);
        let y = pts["Y"];
        assert!((y[0] - 0.285156).abs() < 1e-5, "μ = {}", y[0]);
        assert!((y[2] - 0.036629).abs() < 1e-5, "δ = {}", y[2]);
    }

    /// The SC primitive construction must reproduce the conventional cell
    /// volume divided by the number of centring points.
    #[test]
    fn test_sc_primitive_volumes() {
        let p = params(3.0, 4.0, 5.0, 90.0, 90.0, 90.0);
        let vol_conv = 60.0;
        for (bt, div) in [
            (BravaisType::ORC, 1.0),
            (BravaisType::ORCF1, 4.0),
            (BravaisType::ORCI, 2.0),
            (BravaisType::ORCC, 2.0),
        ] {
            let v = sc_primitive_lattice(bt, &p).determinant().abs();
            assert!(
                (v - vol_conv / div).abs() < 1e-9,
                "{bt:?}: volume {v} != {}",
                vol_conv / div
            );
        }

        // MCL/MCLC with oblique α
        let pm = params(3.0, 4.0, 5.0, 80.0, 90.0, 90.0);
        let vol_m = 3.0 * 4.0 * 5.0 * pm.alpha.sin();
        let v_mcl = sc_primitive_lattice(BravaisType::MCL, &pm).determinant().abs();
        let v_mclc = sc_primitive_lattice(BravaisType::MCLC1, &pm).determinant().abs();
        assert!((v_mcl - vol_m).abs() < 1e-9);
        assert!((v_mclc - vol_m / 2.0).abs() < 1e-9);
    }
}
