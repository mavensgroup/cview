use crate::model::structure::{Structure, Atom};
use moyo::base::{Cell, Lattice, AngleTolerance};
use moyo::MoyoDataset;
use moyo::data::Setting;
use nalgebra::{Matrix3, Vector3};
use crate::model::elements::get_atomic_number;

#[derive(Debug, Clone)]
pub struct KPoint {
    pub label: String,
    pub coords: [f64; 3],
}

#[derive(Debug, Clone)]
pub struct KPathResult {
    pub spacegroup: String,
    pub number: i32,
    pub kpoints: Vec<KPoint>,
    pub path_string: String,
    pub bz_lines: Vec<([f64; 3], [f64; 3])>,
}

pub fn calculate_kpath(structure: &Structure) -> Option<KPathResult> {
    // 1. Convert CView Structure -> Moyo Cell
    let l = structure.lattice;

    // Construct Lattice (Row-major in CView -> Matrix3)
    let lattice_mat = Matrix3::new(
        l[0][0], l[0][1], l[0][2],
        l[1][0], l[1][1], l[1][2],
        l[2][0], l[2][1], l[2][2],
    );
    let lattice = Lattice::new(lattice_mat);

    // CRITICAL FIX: Convert Cartesian -> Fractional
    // Moyo expects fractional coordinates.
    // fractional = (L^T)^-1 * cartesian
    let inv_mat = lattice_mat.try_inverse()?; // Return None if singular

    let mut positions = Vec::new();
    let mut numbers = Vec::new();

    for atom in &structure.atoms {
        let v_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
        let v_frac = inv_mat.transpose() * v_cart; // Convert

        positions.push(v_frac);

        let z = get_atomic_number(&atom.element) as i32;
        numbers.push(if z == 0 { 1 } else { z });
    }

    let cell = Cell::new(lattice, positions, numbers);

    // 2. Run Moyo Symmetry Search
    // API: new(cell, symprec, angle_tolerance, setting, refine_cell)
    let dataset = match MoyoDataset::new(
        &cell,
        1e-4,
        AngleTolerance::Default,
        Setting::Spglib,
        true // Refine/Standardize cell
    ) {
        Ok(d) => d,
        Err(e) => {
            println!("[KPATH] Moyo symmetry search failed: {:?}", e);
            return None;
        }
    };

    let sg_num = dataset.number;
    let hall_num = dataset.hall_number;

    println!("[KPATH] Moyo identified SG #{} (Hall: {})", sg_num, hall_num);

    // 3. DECIDE K-PATH BASED ON SPACE GROUP
    let mut final_sg = sg_num;
    let mut sg_label = format!("SG #{}", sg_num);

    // Geometry Check for R-3m (Primitive BCC vs FCC ambiguity)
    // If Moyo returns 166 (R-3m), it could be Primitive BCC or FCC.
    if sg_num == 166 || sg_num == 167 {
        let alpha = angle_deg(l[1], l[2]);
        if (alpha - 109.47).abs() < 3.0 {
            println!("[KPATH] R-3m (~109.5°) detected -> Treating as Primitive BCC (#229)");
            final_sg = 229;
            sg_label = "Im-3m (Prim)".to_string();
        } else if (alpha - 60.0).abs() < 3.0 {
            println!("[KPATH] R-3m (~60.0°) detected -> Treating as Primitive FCC (#225)");
            final_sg = 225;
            sg_label = "Fm-3m (Prim)".to_string();
        }
    }

    let (points, path_str) = get_standard_path(final_sg);
    let bz_lines = get_bz_lines(final_sg);

    Some(KPathResult {
        spacegroup: sg_label,
        number: final_sg,
        kpoints: points,
        path_string: path_str,
        bz_lines,
    })
}

fn angle_deg(v1: [f64; 3], v2: [f64; 3]) -> f64 {
    let dot = v1[0]*v2[0] + v1[1]*v2[1] + v1[2]*v2[2];
    let m1 = (v1[0].powi(2) + v1[1].powi(2) + v1[2].powi(2)).sqrt();
    let m2 = (v2[0].powi(2) + v2[1].powi(2) + v2[2].powi(2)).sqrt();
    (dot / (m1 * m2)).clamp(-1.0, 1.0).acos() * 180.0 / std::f64::consts::PI
}

// --- STANDARD PATHS ---

fn get_standard_path(sg: i32) -> (Vec<KPoint>, String) {
    let g = KPoint { label: "Γ".to_string(), coords: [0.0, 0.0, 0.0] };

    match sg {
        // FCC Path
        196 | 202 | 203 | 209 | 210 | 216 | 219 | 225..=228 | 230 => {
            let x = KPoint { label: "X".to_string(), coords: [0.5, 0.0, 0.5] };
            let w = KPoint { label: "W".to_string(), coords: [0.5, 0.25, 0.75] };
            let k = KPoint { label: "K".to_string(), coords: [0.375, 0.375, 0.75] };
            let l = KPoint { label: "L".to_string(), coords: [0.5, 0.5, 0.5] };
            (vec![g.clone(), x, w, k, g, l], "Γ -> X -> W -> K -> Γ -> L".to_string())
        },
        // BCC Path
        229 | 197 | 199 | 211 | 214 | 217 | 220 => {
             let h = KPoint { label: "H".to_string(), coords: [0.5, -0.5, 0.5] };
             let n = KPoint { label: "N".to_string(), coords: [0.0, 0.0, 0.5] };
             let p = KPoint { label: "P".to_string(), coords: [0.25, 0.25, 0.25] };
             (vec![g.clone(), h, n.clone(), g, p, n], "Γ -> H -> N -> Γ -> P -> N".to_string())
        },
        // Hexagonal
        168..=194 => {
             let m = KPoint { label: "M".to_string(), coords: [0.5, 0.0, 0.0] };
             let k = KPoint { label: "K".to_string(), coords: [1.0/3.0, 1.0/3.0, 0.0] };
             let a = KPoint { label: "A".to_string(), coords: [0.0, 0.0, 0.5] };
             let l = KPoint { label: "L".to_string(), coords: [0.5, 0.0, 0.5] };
             let h = KPoint { label: "H".to_string(), coords: [1.0/3.0, 1.0/3.0, 0.5] };
             (vec![g.clone(), m, k, g, a.clone(), l, h, a], "Γ -> M -> K -> Γ -> A -> L -> H -> A".to_string())
        },
        // Simple Cubic / Default
        _ => {
             let x = KPoint { label: "X".to_string(), coords: [0.5, 0.0, 0.0] };
             let m = KPoint { label: "M".to_string(), coords: [0.5, 0.5, 0.0] };
             let r = KPoint { label: "R".to_string(), coords: [0.5, 0.5, 0.5] };
             (vec![g.clone(), x.clone(), m, g, r, x], "Γ -> X -> M -> Γ -> R -> X".to_string())
        }
    }
}

// --- BZ WIREFRAMES ---

fn get_bz_lines(sg: i32) -> Vec<([f64; 3], [f64; 3])> {
    match sg {
        // BCC Path -> Rhombic Dodecahedron BZ
        229 | 197 | 199 | 211 | 214 | 217 | 220 => get_bcc_bz(),
        // FCC Path -> Truncated Octahedron (using cube placeholder for now)
        196 | 202 | 203 | 209 | 210 | 216 | 219 | 225..=228 | 230 => get_cube_frame(),
        // Hexagonal
        168..=194 => get_hex_bz(),
        // Default Cube
        _ => get_cube_frame(),
    }
}

fn get_bcc_bz() -> Vec<([f64; 3], [f64; 3])> {
    // Rhombic Dodecahedron
     let mut lines = Vec::new();
     let tips: [[f64; 3]; 6] = [[1.,0.,0.], [-1.,0.,0.], [0.,1.,0.], [0.,-1.,0.], [0.,0.,1.], [0.,0.,-1.]];
     for tip in tips.iter() {
         for dx in [-0.5f64, 0.5] { for dy in [-0.5f64, 0.5] { for dz in [-0.5f64, 0.5] {
             if ((tip[0]-dx).powi(2)+(tip[1]-dy).powi(2)+(tip[2]-dz).powi(2)) < 0.8 {
                 lines.push((*tip, [dx, dy, dz]));
             }
         }}}
     }
     lines
}

fn get_hex_bz() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let r = 2.0/3.0;
    let mut c = Vec::new();
    for i in 0..6 { let a = (i as f64)*60.0*std::f64::consts::PI/180.0; c.push([r*a.cos(), r*a.sin()]); }
    for i in 0..6 {
        let j = (i+1)%6;
        lines.push(([c[i][0],c[i][1],0.5], [c[j][0],c[j][1],0.5]));
        lines.push(([c[i][0],c[i][1],-0.5], [c[j][0],c[j][1],-0.5]));
        lines.push(([c[i][0],c[i][1],-0.5], [c[i][0],c[i][1],0.5]));
    }
    lines
}

fn get_cube_frame() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let d = 0.5;
    let p = [[-d,-d,-d], [d,-d,-d], [d,d,-d], [-d,d,-d], [-d,-d,d], [d,-d,d], [d,d,d], [-d,d,d]];
    lines.push((p[0],p[1])); lines.push((p[1],p[2])); lines.push((p[2],p[3])); lines.push((p[3],p[0]));
    lines.push((p[4],p[5])); lines.push((p[5],p[6])); lines.push((p[6],p[7])); lines.push((p[7],p[4]));
    lines.push((p[0],p[4])); lines.push((p[1],p[5])); lines.push((p[2],p[6])); lines.push((p[3],p[7]));
    lines
}
