use crate::model::elements::get_atomic_number;
use crate::model::structure::Structure;
use moyo::base::{AngleTolerance, Cell, Lattice};
use moyo::data::Setting;
use moyo::MoyoDataset;
use nalgebra::{Matrix3, Vector3};
use std::f64::consts::PI;

const SYMPREC: f64 = 1e-4;

#[derive(Debug, Clone)]
pub struct KPoint {
    pub label: String,
    pub coords: [f64; 3],
}

#[derive(Debug, Clone)]
pub struct KPathResult {
    pub spacegroup: String,
    pub number: i32,
    pub bravais_type: String,
    pub kpoints: Vec<KPoint>,
    pub path_string: String,
    pub bz_lines: Vec<([f64; 3], [f64; 3])>,
}

pub fn calculate_kpath(structure: &Structure) -> Option<KPathResult> {
    // 1. Build Column-Basis Matrix
    let col_a = Vector3::new(
        structure.lattice[0][0],
        structure.lattice[0][1],
        structure.lattice[0][2],
    );
    let col_b = Vector3::new(
        structure.lattice[1][0],
        structure.lattice[1][1],
        structure.lattice[1][2],
    );
    let col_c = Vector3::new(
        structure.lattice[2][0],
        structure.lattice[2][1],
        structure.lattice[2][2],
    );

    let basis_matrix = Matrix3::from_columns(&[col_a, col_b, col_c]);

    if basis_matrix.determinant().abs() < 1e-10 {
        eprintln!("[KPATH] Error: Degenerate lattice");
        return None;
    }

    // 2. Cartesian -> Fractional
    let inv_basis = basis_matrix.try_inverse()?;

    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
        let frac = inv_basis * cart;
        positions.push(frac);
        let z = get_atomic_number(&atom.element) as i32;
        numbers.push(z.max(1));
    }

    // 3. Symmetry (Moyo) - Transpose for Row-Major input
    let moyo_lattice = Lattice::new(basis_matrix.transpose());
    let cell = Cell::new(moyo_lattice, positions, numbers);

    let dataset = match MoyoDataset::new(
        &cell,
        SYMPREC,
        AngleTolerance::Default,
        Setting::Spglib,
        true,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[KPATH] Moyo failed: {:?}", e);
            return None;
        }
    };

    let sg_num = dataset.number;

    // 4. Classify Bravais - Transpose standardized lattice
    let std_mat_rows = dataset.std_cell.lattice.basis; // Already Matrix3 (Rows)
    let bravais = classify_bravais_lattice(sg_num, &std_mat_rows.transpose());

    println!("[KPATH] Detected SG #{} -> {:?}", sg_num, bravais);

    // 5. Get Path & Wireframe
    let (kpoints, path_string) = get_kpath_for_bravais(&bravais);
    let bz_lines = get_bz_wireframe(&bravais);

    Some(KPathResult {
        spacegroup: format!("{} ({})", sg_num, dataset.hall_number),
        number: sg_num,
        bravais_type: format!("{:?}", bravais),
        kpoints,
        path_string,
        bz_lines,
    })
}

// --- CLASSIFICATION ---

#[derive(Debug, Clone, Copy, PartialEq)]
enum BravaisLattice {
    CubicP,
    CubicF,
    CubicI,
    TetragonalP,
    TetragonalI,
    Orthorhombic,
    HexagonalP,
    Rhombohedral,
    Monoclinic,
    Triclinic,
}

fn classify_bravais_lattice(sg: i32, lattice_cols: &Matrix3<f64>) -> BravaisLattice {
    use BravaisLattice::*;
    match sg {
        1..=2 => Triclinic,
        3..=15 => Monoclinic,
        16..=74 => Orthorhombic,
        75..=142 => {
            if is_tetragonal_i(sg) {
                TetragonalI
            } else {
                TetragonalP
            }
        }
        143..=167 => {
            if is_rhombohedral_geometry(lattice_cols) {
                Rhombohedral
            } else {
                HexagonalP
            }
        }
        168..=194 => HexagonalP,
        195..=230 => {
            if sg <= 206 {
                CubicP
            } else if (sg >= 207 && sg <= 214) || [217, 220, 229].contains(&sg) {
                CubicI
            } else {
                CubicF
            }
        }
        _ => Triclinic,
    }
}

fn is_tetragonal_i(sg: i32) -> bool {
    matches!(sg, 79|80|82|87|88|97|98|107..=110|119..=122|139..=142)
}

fn is_rhombohedral_geometry(mat: &Matrix3<f64>) -> bool {
    let a = mat.column(0).norm();
    let b = mat.column(1).norm();
    let c = mat.column(2).norm();
    (a - b).abs() < 1e-3 && (a - c).abs() < 1e-3
}

// --- PATH GENERATION ---

fn get_kpath_for_bravais(bravais: &BravaisLattice) -> (Vec<KPoint>, String) {
    use BravaisLattice::*;
    let g = kp("Γ", 0., 0., 0.);

    match bravais {
        CubicF => {
            // FCC
            let x = kp("X", 0.5, 0.0, 0.5);
            let w = kp("W", 0.5, 0.25, 0.75);
            let k = kp("K", 0.375, 0.375, 0.75);
            let l = kp("L", 0.5, 0.5, 0.5);
            let u = kp("U", 0.625, 0.25, 0.625);
            (
                vec![
                    g.clone(),
                    x.clone(),
                    w.clone(),
                    k.clone(),
                    g.clone(),
                    l.clone(),
                    u.clone(),
                    w,
                    l,
                    k,
                    u,
                    x,
                ],
                "Γ-X-W-K-Γ-L-U-W-L-K|U-X".to_string(),
            )
        }
        CubicI => {
            // BCC
            let h = kp("H", 0.5, -0.5, 0.5);
            let n = kp("N", 0.0, 0.0, 0.5);
            let p = kp("P", 0.25, 0.25, 0.25);
            (
                vec![g.clone(), h.clone(), n.clone(), g, p.clone(), h, p, n],
                "Γ-H-N-Γ-P-H|P-N".to_string(),
            )
        }
        HexagonalP => {
            let m = kp("M", 0.5, 0.0, 0.0);
            let k = kp("K", 1.0 / 3.0, 1.0 / 3.0, 0.0);
            let a = kp("A", 0.0, 0.0, 0.5);
            let l = kp("L", 0.5, 0.0, 0.5);
            let h = kp("H", 1.0 / 3.0, 1.0 / 3.0, 0.5);
            (
                vec![
                    g.clone(),
                    m.clone(),
                    k.clone(),
                    g,
                    a.clone(),
                    l.clone(),
                    h.clone(),
                    a,
                    l,
                    m,
                    k,
                    h,
                ],
                "Γ-M-K-Γ-A-L-H-A|L-M|K-H".to_string(),
            )
        }
        Rhombohedral => {
            let f = kp("F", 0.5, 0.5, 0.0);
            let l = kp("L", 0.5, 0.0, 0.0);
            let z = kp("Z", 0.5, 0.5, 0.5);
            (vec![g.clone(), f, l, z, g], "Γ-F-L-Z-Γ".to_string())
        }
        _ => {
            // Simple Cubic / Default
            let x = kp("X", 0.5, 0.0, 0.0);
            let m = kp("M", 0.5, 0.5, 0.0);
            let r = kp("R", 0.5, 0.5, 0.5);
            (
                vec![g.clone(), x.clone(), m.clone(), g, r.clone(), x, m, r],
                "Γ-X-M-Γ-R-X|M-R".to_string(),
            )
        }
    }
}

fn kp(label: &str, x: f64, y: f64, z: f64) -> KPoint {
    KPoint {
        label: label.to_string(),
        coords: [x, y, z],
    }
}

// --- BRILLOUIN ZONES ---

fn get_bz_wireframe(bravais: &BravaisLattice) -> Vec<([f64; 3], [f64; 3])> {
    use BravaisLattice::*;
    match bravais {
        CubicF => get_truncated_octahedron(),
        CubicI => get_rhombic_dodecahedron(),
        HexagonalP | Rhombohedral => get_hexagonal_prism(),
        _ => get_cube_wireframe(),
    }
}

/// Truncated Octahedron (FCC BZ)
/// Correct Geometry: Permutations of (0, ±1, ±2) scaled
fn get_truncated_octahedron() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let s: f64 = 0.35; // FIX: Explicit type f64 prevents "ambiguous numeric type" error

    // Generate all 24 vertices: Permutations of (0, ±1s, ±2s)
    let mut vertices = Vec::new();
    let coords = [0.0, 1.0 * s, 2.0 * s];

    // Sign combinations
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            // Permutation (0, 1, 2) -> (0, s, 2s)
            vertices.push([0.0, sx * coords[1], sy * coords[2]]);
            vertices.push([0.0, sx * coords[2], sy * coords[1]]);

            // Permutation (1, 0, 2) -> (s, 0, 2s)
            vertices.push([sx * coords[1], 0.0, sy * coords[2]]);
            vertices.push([sx * coords[2], 0.0, sy * coords[1]]);

            // Permutation (1, 2, 0) -> (s, 2s, 0)
            vertices.push([sx * coords[1], sy * coords[2], 0.0]);
            vertices.push([sx * coords[2], sy * coords[1], 0.0]);
        }
    }

    // Connect nearest neighbors
    let target_dist_sq: f64 = 2.0 * s * s;
    let tolerance: f64 = 0.1 * s * s;

    for i in 0..vertices.len() {
        for j in (i + 1)..vertices.len() {
            let dx = vertices[i][0] - vertices[j][0];
            let dy = vertices[i][1] - vertices[j][1];
            let dz = vertices[i][2] - vertices[j][2];
            let d2 = dx * dx + dy * dy + dz * dz;

            if (d2 - target_dist_sq).abs() < tolerance {
                lines.push((vertices[i], vertices[j]));
            }
        }
    }

    lines
}

/// Rhombic Dodecahedron (BCC BZ)
fn get_rhombic_dodecahedron() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    // 14 vertices: 6 tips and 8 corners
    let tips = [
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
    ];
    let corners = [
        [0.5, 0.5, 0.5],
        [0.5, 0.5, -0.5],
        [0.5, -0.5, 0.5],
        [0.5, -0.5, -0.5],
        [-0.5, 0.5, 0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [-0.5, -0.5, -0.5],
    ];

    for t in tips {
        for c in corners {
            let dx = t[0] - c[0];
            let dy = t[1] - c[1];
            let dz = t[2] - c[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            // Dist is sqrt(0.5^2*2 + 0.5^2) = 0.866. d2 = 0.75
            if d2 < 0.8 {
                lines.push((t, c));
            }
        }
    }
    lines
}

/// Hexagonal Prism
fn get_hexagonal_prism() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let r = 0.65;
    let h = 0.5;
    for z in [-h, h] {
        for i in 0..6 {
            let a1 = (i as f64) * PI / 3.0;
            let a2 = ((i + 1) % 6) as f64 * PI / 3.0;
            lines.push((
                [r * a1.cos(), r * a1.sin(), z],
                [r * a2.cos(), r * a2.sin(), z],
            ));
        }
    }
    for i in 0..6 {
        let a = (i as f64) * PI / 3.0;
        lines.push((
            [r * a.cos(), r * a.sin(), -h],
            [r * a.cos(), r * a.sin(), h],
        ));
    }
    lines
}

/// Simple Cube
fn get_cube_wireframe() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let d = 0.5;
    let v = [
        [-d, -d, -d],
        [d, -d, -d],
        [d, d, -d],
        [-d, d, -d],
        [-d, -d, d],
        [d, -d, d],
        [d, d, d],
        [-d, d, d],
    ];
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    for (s, e) in edges {
        lines.push((v[s], v[e]));
    }
    lines
}
