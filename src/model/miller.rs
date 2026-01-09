// src/model/miller.rs
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
                p[2].clamp(0.0, 1.0)
            ]);
        };

        // --- 1. Edges along X (y=0/1, z=0/1) ---
        // Equation: h*x + k*y + l*z = d
        if h.abs() > 1e-6 {
            // y=0, z=0 => hx = d
            let x = d / h;
            if x >= -1e-5 && x <= 1.00001 { add_point([x, 0.0, 0.0]); }

            // y=1, z=0 => hx + k = d
            let x = (d - k) / h;
            if x >= -1e-5 && x <= 1.00001 { add_point([x, 1.0, 0.0]); }

            // y=0, z=1 => hx + l = d
            let x = (d - l) / h;
            if x >= -1e-5 && x <= 1.00001 { add_point([x, 0.0, 1.0]); }

            // y=1, z=1 => hx + k + l = d
            let x = (d - k - l) / h;
            if x >= -1e-5 && x <= 1.00001 { add_point([x, 1.0, 1.0]); }
        }

        // --- 2. Edges along Y ---
        if k.abs() > 1e-6 {
            // x=0, z=0 => ky = d
            let y = d / k;
            if y >= -1e-5 && y <= 1.00001 { add_point([0.0, y, 0.0]); }

            // x=1, z=0 => h + ky = d
            let y = (d - h) / k;
            if y >= -1e-5 && y <= 1.00001 { add_point([1.0, y, 0.0]); }

            // x=0, z=1 => ky + l = d
            let y = (d - l) / k;
            if y >= -1e-5 && y <= 1.00001 { add_point([0.0, y, 1.0]); }

            // x=1, z=1 => h + ky + l = d
            let y = (d - h - l) / k;
            if y >= -1e-5 && y <= 1.00001 { add_point([1.0, y, 1.0]); }
        }

        // --- 3. Edges along Z ---
        if l.abs() > 1e-6 {
            // x=0, y=0 => lz = d
            let z = d / l;
            if z >= -1e-5 && z <= 1.00001 { add_point([0.0, 0.0, z]); }

            // x=1, y=0 => h + lz = d
            let z = (d - h) / l;
            if z >= -1e-5 && z <= 1.00001 { add_point([1.0, 0.0, z]); }

            // x=0, y=1 => k + lz = d
            let z = (d - k) / l;
            if z >= -1e-5 && z <= 1.00001 { add_point([0.0, 1.0, z]); }

            // x=1, y=1 => h + k + lz = d
            let z = (d - h - k) / l;
            if z >= -1e-5 && z <= 1.00001 { add_point([1.0, 1.0, z]); }
        }

        // Remove duplicates (points close to corners might be added multiple times)
        // Sort first to make dedup work
        points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(Ordering::Equal));
        points.dedup_by(|a, b| {
            (a[0] - b[0]).abs() < 1e-4 &&
            (a[1] - b[1]).abs() < 1e-4 &&
            (a[2] - b[2]).abs() < 1e-4
        });

        if points.len() < 3 { return Vec::new(); }

        sort_points_angularly(&mut points);
        points
    }
}

fn sort_points_angularly(points: &mut Vec<[f64; 3]>) {
    if points.is_empty() { return; }

    // 1. Calculate Centroid
    let mut cx = 0.0; let mut cy = 0.0; let mut cz = 0.0;
    for p in points.iter() { cx += p[0]; cy += p[1]; cz += p[2]; }
    let n = points.len() as f64;
    cx /= n; cy /= n; cz /= n;

    // 2. Find a valid normal and reference vector
    // We try to find two vectors (v1, v2) from the centroid that are not parallel.
    let mut v1 = [0.0, 0.0, 0.0];
    let mut normal = [0.0, 0.0, 0.0];
    let mut found_normal = false;

    for i in 0..points.len() {
        let va = [points[i][0] - cx, points[i][1] - cy, points[i][2] - cz];
        if dot(va, va) < 1e-8 { continue; } // Point too close to centroid

        for j in (i+1)..points.len() {
            let vb = [points[j][0] - cx, points[j][1] - cy, points[j][2] - cz];
            if dot(vb, vb) < 1e-8 { continue; }

            // Cross product
            let nx = va[1]*vb[2] - va[2]*vb[1];
            let ny = va[2]*vb[0] - va[0]*vb[2];
            let nz = va[0]*vb[1] - va[1]*vb[0];
            let len_sq = nx*nx + ny*ny + nz*nz;

            if len_sq > 1e-8 {
                normal = [nx, ny, nz];
                v1 = va;
                found_normal = true;
                break;
            }
        }
        if found_normal { break; }
    }

    // If we couldn't find a normal, the points are collinear or degenerate.
    if !found_normal { return; }

    // 3. Define Basis Vectors U and V on the plane
    // Basis U = v1 normalized
    let u_len = dot(v1, v1).sqrt();
    let u = [v1[0]/u_len, v1[1]/u_len, v1[2]/u_len];

    // Basis V = Normal x U (normalized)
    let nx = normal[0]; let ny = normal[1]; let nz = normal[2];
    let vx = ny*u[2] - nz*u[1];
    let vy = nz*u[0] - nx*u[2];
    let vz = nx*u[1] - ny*u[0];
    let v_len = (vx*vx + vy*vy + vz*vz).sqrt();
    let v = [vx/v_len, vy/v_len, vz/v_len];

    // 4. Sort by Angle
    points.sort_by(|a, b| {
        let va = [a[0] - cx, a[1] - cy, a[2] - cz];
        let vb = [b[0] - cx, b[1] - cy, b[2] - cz];

        // Project onto basis vectors
        let dot_ua = dot(va, u);
        let dot_va = dot(va, v);
        let angle_a = dot_va.atan2(dot_ua);

        let dot_ub = dot(vb, u);
        let dot_vb = dot(vb, v);
        let angle_b = dot_vb.atan2(dot_ub);

        // SAFE COMPARE: unwrap_or handles NaN cases
        angle_a.partial_cmp(&angle_b).unwrap_or(Ordering::Equal)
    });
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0]*b[0] + a[1]*b[1] + a[2]*b[2]
}
