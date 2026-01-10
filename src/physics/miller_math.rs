use nalgebra::{Matrix3, Vector3};

// --- VISUALIZATION HELPERS (Used by painter.rs) ---

/// Calculates the cross product for f64 arrays (Visualization)
fn cross_product_f64(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Get the intersection points of the Miller Plane with the Unit Cell (0..1 box)
/// Returns: (List of vertices in Cartesian coords, Normal Vector)
pub fn get_plane_geometry(
    h: i32, k: i32, l: i32,
    lattice: [[f64; 3]; 3]
) -> Option<(Vec<[f64; 3]>, [f64; 3])> {

    if h == 0 && k == 0 && l == 0 { return None; }

    let h_f = h as f64;
    let k_f = k as f64;
    let l_f = l as f64;

    // 1. Calculate Normal Vector using Lattice Basis (Reciprocal direction)
    // Normal = h(b x c) + k(c x a) + l(a x b)
    let v_a = lattice[0];
    let v_b = lattice[1];
    let v_c = lattice[2];

    let b_x_c = cross_product_f64(v_b, v_c);
    let c_x_a = cross_product_f64(v_c, v_a);
    let a_x_b = cross_product_f64(v_a, v_b);

    let nx = h_f * b_x_c[0] + k_f * c_x_a[0] + l_f * a_x_b[0];
    let ny = h_f * b_x_c[1] + k_f * c_x_a[1] + l_f * a_x_b[1];
    let nz = h_f * b_x_c[2] + k_f * c_x_a[2] + l_f * a_x_b[2];

    // Normalize normal
    let mag = (nx*nx + ny*ny + nz*nz).sqrt();
    let normal = if mag > 1e-9 { [nx/mag, ny/mag, nz/mag] } else { [0.0, 1.0, 0.0] };

    // 2. Define the 12 edges of the unit cube (Start Point, Direction)
    let edges = [
        ([0.,0.,0.], [1.,0.,0.]), ([0.,0.,0.], [0.,1.,0.]), ([0.,0.,0.], [0.,0.,1.]),
        ([1.,0.,0.], [0.,1.,0.]), ([1.,0.,0.], [0.,0.,1.]),
        ([0.,1.,0.], [1.,0.,0.]), ([0.,1.,0.], [0.,0.,1.]),
        ([0.,0.,1.], [1.,0.,0.]), ([0.,0.,1.], [0.,1.,0.]),
        ([1.,1.,0.], [0.,0.,1.]), ([1.,0.,1.], [0.,1.,0.]), ([0.,1.,1.], [1.,0.,0.]),
    ];

    let mut points: Vec<[f64; 3]> = Vec::new();

    // 3. Find intersections: h*x + k*y + l*z = 1
    for (start, dir) in edges.iter() {
        // Parametric line: P = start + t*dir
        // Plane eq: h(sx + t*dx) + ... = 1
        let p_dot_d = h_f*dir[0] + k_f*dir[1] + l_f*dir[2];
        let p_dot_s = h_f*start[0] + k_f*start[1] + l_f*start[2];

        if p_dot_d.abs() > 1e-6 {
            let t = (1.0 - p_dot_s) / p_dot_d;
            if t >= -0.001 && t <= 1.001 {
                // Point in Fractional Coords
                let fx = start[0] + t*dir[0];
                let fy = start[1] + t*dir[1];
                let fz = start[2] + t*dir[2];

                // Convert to Cartesian
                let cx = fx*lattice[0][0] + fy*lattice[1][0] + fz*lattice[2][0];
                let cy = fx*lattice[0][1] + fy*lattice[1][1] + fz*lattice[2][1];
                let cz = fx*lattice[0][2] + fy*lattice[1][2] + fz*lattice[2][2];

                points.push([cx, cy, cz]);
            }
        }
    }

    if points.len() < 3 { return None; }

    // Deduplicate
    points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    points.dedup_by(|a, b| {
        (a[0]-b[0]).abs() < 1e-4 && (a[1]-b[1]).abs() < 1e-4 && (a[2]-b[2]).abs() < 1e-4
    });

    if points.len() < 3 { return None; }

    Some((points, normal))
}


// --- PHYSICS LOGIC (Basis Finding) ---

/// Helper for integer cross product (used internally for basis finding)
fn cross_prod_i32(a: Vector3<i32>, b: Vector3<i32>) -> Vector3<i32> {
    Vector3::new(
        a.y*b.z - a.z*b.y,
        a.z*b.x - a.x*b.z,
        a.x*b.y - a.y*b.x
    )
}

/// Calculates the basis vectors for a Miller plane (h k l).
/// Returns (u_vec, v_vec, w_vec)
pub fn find_plane_basis(h: i32, k: i32, l: i32, lattice: [[f64; 3]; 3]) -> Result<(Vector3<i32>, Vector3<i32>, Vector3<i32>), String> {
    if h == 0 && k == 0 && l == 0 {
        return Err("Indices cannot be (0,0,0)".to_string());
    }

    let mat_orig = Matrix3::new(
        lattice[0][0], lattice[0][1], lattice[0][2],
        lattice[1][0], lattice[1][1], lattice[1][2],
        lattice[2][0], lattice[2][1], lattice[2][2],
    );

    // 1. Search for In-Plane Vectors (h*u + k*v + l*w = 0)
    let limit = 4;
    let mut candidates = Vec::new();

    for u in -limit..=limit {
        for v in -limit..=limit {
            for w in -limit..=limit {
                if u == 0 && v == 0 && w == 0 { continue; }

                if h*u + k*v + l*w == 0 {
                    let vec_int = Vector3::new(u as f64, v as f64, w as f64);
                    // Check physical length in Cartesian space
                    let cart = mat_orig.transpose() * vec_int;
                    let len_sq = cart.norm_squared();
                    candidates.push((Vector3::new(u,v,w), len_sq));
                }
            }
        }
    }

    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    if candidates.len() < 2 {
        return Err("Could not find surface vectors. indices too high?".to_string());
    }

    let u_vec = candidates[0].0;

    // Find v_vec (shortest non-parallel to u)
    let mut v_vec = Vector3::zeros();
    let mut found_v = false;
    for cand in candidates.iter().skip(1) {
        let t = cand.0;
        let cp = cross_prod_i32(u_vec, t); // Use our specific integer helper
        if cp.x != 0 || cp.y != 0 || cp.z != 0 {
            v_vec = t;
            found_v = true;
            break;
        }
    }

    if !found_v {
        return Err("Could not define primitive surface unit cell.".to_string());
    }

    // 2. Search for Stacking Vector (Shortest vector NOT in plane)
    let mut w_vec = Vector3::zeros();
    let mut min_len_w = f64::MAX;

    for u in -limit..=limit {
        for v in -limit..=limit {
            for w in -limit..=limit {
                if h*u + k*v + l*w != 0 {
                    let vec_int = Vector3::new(u as f64, v as f64, w as f64);
                    let cart = mat_orig.transpose() * vec_int;
                    let len_sq = cart.norm_squared();
                    if len_sq < min_len_w {
                        min_len_w = len_sq;
                        w_vec = Vector3::new(u,v,w);
                    }
                }
            }
        }
    }

    Ok((u_vec, v_vec, w_vec))
}
