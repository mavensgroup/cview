// src/model/miller.rs
use nalgebra::Vector3;
use std::cmp::Ordering;

pub struct MillerPlane {
    pub h: i32,
    pub k: i32,
    pub l: i32,
    pub shift: f64,
}

impl MillerPlane {
    pub fn new(h: i32, k: i32, l: i32, shift: f64) -> Self {
        Self { h, k, l, shift }
    }

    pub fn get_intersection_points(&self) -> Vec<[f64; 3]> {
        let h = self.h as f64;
        let k = self.k as f64;
        let l = self.l as f64;
        let d = self.shift;

        let mut points = Vec::new();

        // Helper to check and push valid fractional coordinates [0..1]
        // We use a small epsilon (-1e-5) to include points exactly on the boundary
        let mut add_point = |p: [f64; 3]| {
            points.push([
                p[0].clamp(0.0, 1.0),
                p[1].clamp(0.0, 1.0),
                p[2].clamp(0.0, 1.0),
            ]);
        };

        // --- 1. Edges along X (y=0/1, z=0/1) ---
        // Equation: h*x + k*y + l*z = d
        if h.abs() > 1e-6 {
            // y=0, z=0 => hx = d
            let x = d / h;
            if (-1e-5..=1.00001).contains(&x) {
                add_point([x, 0.0, 0.0]);
            }

            // y=1, z=0 => hx + k = d
            let x = (d - k) / h;
            if (-1e-5..=1.00001).contains(&x) {
                add_point([x, 1.0, 0.0]);
            }

            // y=0, z=1 => hx + l = d
            let x = (d - l) / h;
            if (-1e-5..=1.00001).contains(&x) {
                add_point([x, 0.0, 1.0]);
            }

            // y=1, z=1 => hx + k + l = d
            let x = (d - k - l) / h;
            if (-1e-5..=1.00001).contains(&x) {
                add_point([x, 1.0, 1.0]);
            }
        }

        // --- 2. Edges along Y ---
        if k.abs() > 1e-6 {
            // x=0, z=0 => ky = d
            let y = d / k;
            if (-1e-5..=1.00001).contains(&y) {
                add_point([0.0, y, 0.0]);
            }

            // x=1, z=0 => h + ky = d
            let y = (d - h) / k;
            if (-1e-5..=1.00001).contains(&y) {
                add_point([1.0, y, 0.0]);
            }

            // x=0, z=1 => ky + l = d
            let y = (d - l) / k;
            if (-1e-5..=1.00001).contains(&y) {
                add_point([0.0, y, 1.0]);
            }

            // x=1, z=1 => h + ky + l = d
            let y = (d - h - l) / k;
            if (-1e-5..=1.00001).contains(&y) {
                add_point([1.0, y, 1.0]);
            }
        }

        // --- 3. Edges along Z ---
        if l.abs() > 1e-6 {
            // x=0, y=0 => lz = d
            let z = d / l;
            if (-1e-5..=1.00001).contains(&z) {
                add_point([0.0, 0.0, z]);
            }

            // x=1, y=0 => h + lz = d
            let z = (d - h) / l;
            if (-1e-5..=1.00001).contains(&z) {
                add_point([1.0, 0.0, z]);
            }

            // x=0, y=1 => k + lz = d
            let z = (d - k) / l;
            if (-1e-5..=1.00001).contains(&z) {
                add_point([0.0, 1.0, z]);
            }

            // x=1, y=1 => h + k + lz = d
            let z = (d - h - k) / l;
            if (-1e-5..=1.00001).contains(&z) {
                add_point([1.0, 1.0, z]);
            }
        }

        // Remove duplicates (points close to corners might be added multiple times)
        // Sort first to make dedup work
        points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(Ordering::Equal));
        points.dedup_by(|a, b| {
            (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4 && (a[2] - b[2]).abs() < 1e-4
        });

        if points.len() < 3 {
            return Vec::new();
        }

        sort_points_angularly(&mut points);
        points
    }
}

fn sort_points_angularly(points: &mut Vec<[f64; 3]>) {
    if points.is_empty() {
        return;
    }

    // 1. Centroid
    let n = points.len() as f64;
    let centroid: Vector3<f64> = points
        .iter()
        .map(|p| Vector3::new(p[0], p[1], p[2]))
        .sum::<Vector3<f64>>()
        / n;

    // 2. Find normal and reference vector from two non-parallel vectors
    let mut v1 = Vector3::zeros();
    let mut normal = Vector3::zeros();
    let mut found = false;

    'outer: for i in 0..points.len() {
        let va = Vector3::new(points[i][0], points[i][1], points[i][2]) - centroid;
        if va.norm_squared() < 1e-8 {
            continue;
        }
        for j in (i + 1)..points.len() {
            let vb = Vector3::new(points[j][0], points[j][1], points[j][2]) - centroid;
            if vb.norm_squared() < 1e-8 {
                continue;
            }
            let n_candidate = va.cross(&vb);
            if n_candidate.norm_squared() > 1e-8 {
                normal = n_candidate;
                v1 = va;
                found = true;
                break 'outer;
            }
        }
    }

    if !found {
        return;
    }

    // 3. Orthonormal basis on the plane
    let u = v1.normalize();
    let v = normal.cross(&u).normalize();

    // 4. Sort by angle
    points.sort_by(|a, b| {
        let va = Vector3::new(a[0], a[1], a[2]) - centroid;
        let vb = Vector3::new(b[0], b[1], b[2]) - centroid;
        let angle_a = va.dot(&v).atan2(va.dot(&u));
        let angle_b = vb.dot(&v).atan2(vb.dot(&u));
        angle_a.partial_cmp(&angle_b).unwrap_or(Ordering::Equal)
    });
}

// local dot removed — was only used by sort_points_angularly
