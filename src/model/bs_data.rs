// src/model/bs_data.rs

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct BrillouinZoneData {
    pub lattice_type: String,
    pub special_points: HashMap<String, [f64; 3]>,
    pub path: Vec<Vec<String>>,
    pub wireframe: Vec<([f64; 3], [f64; 3])>,
}

pub fn get_sc_data(sg_num: i32) -> Option<BrillouinZoneData> {
    match sg_num {
        // Cubic (P)
        195..=206 => Some(simple_cubic()),
        // Cubic (I)
        207..=214 | 217 | 220 | 229 => Some(bcc()),
        // Cubic (F)
        215..=216 | 218..=219 | 221..=228 | 230 => Some(fcc()),
        // Hexagonal
        168..=194 => Some(hexagonal()),
        _ => None,
    }
}

fn simple_cubic() -> BrillouinZoneData {
    let mut pts = HashMap::new();
    pts.insert("Γ".to_string(), [0.0, 0.0, 0.0]);
    pts.insert("X".to_string(), [0.0, 0.5, 0.0]);
    pts.insert("M".to_string(), [0.5, 0.5, 0.0]);
    pts.insert("R".to_string(), [0.5, 0.5, 0.5]);

    BrillouinZoneData {
        lattice_type: "Cubic (P)".to_string(),
        special_points: pts,
        path: vec![
            vec![
                "Γ".into(),
                "X".into(),
                "M".into(),
                "Γ".into(),
                "R".into(),
                "X".into(),
            ],
            vec!["M".into(), "R".into()],
        ],
        wireframe: get_cube_lines(),
    }
}

fn bcc() -> BrillouinZoneData {
    let mut pts = HashMap::new();
    pts.insert("Γ".to_string(), [0.0, 0.0, 0.0]);
    pts.insert("H".to_string(), [0.5, -0.5, 0.5]);
    pts.insert("P".to_string(), [0.25, 0.25, 0.25]);
    pts.insert("N".to_string(), [0.0, 0.0, 0.5]);

    BrillouinZoneData {
        lattice_type: "Cubic (I)".to_string(),
        special_points: pts,
        path: vec![
            vec![
                "Γ".into(),
                "H".into(),
                "N".into(),
                "Γ".into(),
                "P".into(),
                "H".into(),
            ],
            vec!["P".into(), "N".into()],
        ],
        wireframe: get_rhombic_dodecahedron_lines(),
    }
}

fn fcc() -> BrillouinZoneData {
    let mut pts = HashMap::new();
    pts.insert("Γ".to_string(), [0.0, 0.0, 0.0]);
    pts.insert("X".to_string(), [0.5, 0.0, 0.5]);
    pts.insert("W".to_string(), [0.5, 0.25, 0.75]);
    pts.insert("K".to_string(), [0.375, 0.375, 0.75]);
    pts.insert("L".to_string(), [0.5, 0.5, 0.5]);
    pts.insert("U".to_string(), [0.625, 0.25, 0.625]);

    BrillouinZoneData {
        lattice_type: "Cubic (F)".to_string(),
        special_points: pts,
        path: vec![
            vec![
                "Γ".into(),
                "X".into(),
                "W".into(),
                "K".into(),
                "Γ".into(),
                "L".into(),
                "U".into(),
                "W".into(),
                "L".into(),
                "K".into(),
            ],
            vec!["U".into(), "X".into()],
        ],
        wireframe: get_truncated_octahedron_lines(),
    }
}

fn hexagonal() -> BrillouinZoneData {
    let mut pts = HashMap::new();
    pts.insert("Γ".to_string(), [0.0, 0.0, 0.0]);
    pts.insert("M".to_string(), [0.5, 0.0, 0.0]);
    pts.insert("K".to_string(), [1.0 / 3.0, 1.0 / 3.0, 0.0]);
    pts.insert("A".to_string(), [0.0, 0.0, 0.5]);
    pts.insert("L".to_string(), [0.5, 0.0, 0.5]);
    pts.insert("H".to_string(), [1.0 / 3.0, 1.0 / 3.0, 0.5]);

    BrillouinZoneData {
        lattice_type: "Hexagonal".to_string(),
        special_points: pts,
        path: vec![
            vec![
                "Γ".into(),
                "M".into(),
                "K".into(),
                "Γ".into(),
                "A".into(),
                "L".into(),
                "H".into(),
                "A".into(),
            ],
            vec!["L".into(), "M".into()],
            vec!["K".into(), "H".into()],
        ],
        wireframe: get_hex_prism_lines(),
    }
}

// --- Wireframe Helpers ---

fn get_cube_lines() -> Vec<([f64; 3], [f64; 3])> {
    let d = 0.5;
    let v: [[f64; 3]; 8] = [
        [-d, -d, -d],
        [d, -d, -d],
        [d, d, -d],
        [-d, d, -d],
        [-d, -d, d],
        [d, -d, d],
        [d, d, d],
        [-d, d, d],
    ];
    let idxs = [
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
    idxs.iter().map(|(i, j)| (v[*i], v[*j])).collect()
}

fn get_rhombic_dodecahedron_lines() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let tips: [[f64; 3]; 6] = [
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
    ];
    let corners: [[f64; 3]; 8] = [
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
            let dist_sq = dx * dx + dy * dy + dz * dz;
            // 0.866^2 approx 0.75.
            if dist_sq < 0.8 {
                lines.push((t, c));
            }
        }
    }
    lines
}

fn get_truncated_octahedron_lines() -> Vec<([f64; 3], [f64; 3])> {
    // Placeholder: Return cube lines to avoid empty drawing
    get_cube_lines()
}

fn get_hex_prism_lines() -> Vec<([f64; 3], [f64; 3])> {
    let mut lines = Vec::new();
    let r = 2.0 / 3.0; // Reciprocal radius approximation
    let h = 0.5;
    for z in [-h, h] {
        for i in 0..6 {
            let a1 = (i as f64) * std::f64::consts::PI / 3.0;
            let a2 = ((i + 1) % 6) as f64 * std::f64::consts::PI / 3.0;
            lines.push((
                [r * a1.cos(), r * a1.sin(), z],
                [r * a2.cos(), r * a2.sin(), z],
            ));
        }
    }
    for i in 0..6 {
        let a = (i as f64) * std::f64::consts::PI / 3.0;
        lines.push((
            [r * a.cos(), r * a.sin(), -h],
            [r * a.cos(), r * a.sin(), h],
        ));
    }
    lines
}
