use crate::model::structure::{Structure, Atom};
use nalgebra::{Matrix3, Vector3};

/// Generates a surface slab from the given structure.
/// - Finds the smallest possible surface unit cell (primitive).
/// - Adds vacuum strictly along the surface normal.
pub fn generate_slab(
    structure: &Structure,
    h: i32, k: i32, l: i32,
    thickness: u32,
    vacuum: f64
) -> Result<Structure, String> {
    if h == 0 && k == 0 && l == 0 {
        return Err("Miller indices cannot be (0,0,0)".to_string());
    }

    // 1. Setup Matrices
    let lat = structure.lattice;
    // Rows of mat_orig are the lattice vectors (a, b, c)
    let mat_orig = Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2],
        lat[1][0], lat[1][1], lat[1][2],
        lat[2][0], lat[2][1], lat[2][2],
    );

    // 2. Find Primitive Surface Basis Vectors
    // We search for integer vectors (u, v, w) such that h*u + k*v + l*w = 0.
    // We select the two shortest vectors in *Cartesian* space.
    let limit = 4;
    let mut candidates = Vec::new();

    for u in -limit..=limit {
        for v in -limit..=limit {
            for w in -limit..=limit {
                if u == 0 && v == 0 && w == 0 { continue; }

                // Check if vector is in the plane
                if h*u + k*v + l*w == 0 {
                    let vec_int = Vector3::new(u as f64, v as f64, w as f64);
                    // Convert to Cartesian to check real physical length
                    let cart = mat_orig.transpose() * vec_int;
                    let len_sq = cart.norm_squared();
                    candidates.push((Vector3::new(u,v,w), len_sq));
                }
            }
        }
    }

    // Sort by physical length
    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    if candidates.len() < 2 {
        return Err("Could not find surface vectors. Try smaller indices.".to_string());
    }

    // Vector 1: The absolute shortest in-plane vector
    let u_vec = candidates[0].0;

    // Vector 2: The shortest in-plane vector NOT parallel to u_vec
    let mut v_vec = Vector3::zeros();
    let mut found_v = false;
    for cand in candidates.iter().skip(1) {
        let t = cand.0;
        // Check parallelism (cross product of integer indices is non-zero)
        let cp = cross_prod(u_vec, t);
        if cp.x != 0 || cp.y != 0 || cp.z != 0 {
            v_vec = t;
            found_v = true;
            break;
        }
    }

    if !found_v {
        return Err("Could not define primitive surface unit cell.".to_string());
    }

    // Vector 3: The Stacking Vector (out of plane)
    // Find shortest lattice vector NOT in the plane
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

    // 3. Transformation Matrix
    let m_int = Matrix3::new(
        u_vec.x as f64, v_vec.x as f64, w_vec.x as f64,
        u_vec.y as f64, v_vec.y as f64, w_vec.y as f64,
        u_vec.z as f64, v_vec.z as f64, w_vec.z as f64,
    );

    let det = m_int.determinant().abs();
    if det < 1e-6 { return Err("Singular transformation matrix".to_string()); }

    // 4. Construct New Primitive Cell
    // L_new = M^T * L_old
    let lat_new_prim = m_int.transpose() * mat_orig;

    // 5. Map Atoms
    let search_range = (det.powf(1.0/3.0).ceil() as i32) + 2;
    let mut slab_atoms = Vec::new();
    let inv_m = m_int.try_inverse().ok_or("Invert failed")?;

    for i in -search_range..=search_range {
        for j in -search_range..=search_range {
            for k in -search_range..=search_range {
                let shift = Vector3::new(i as f64, j as f64, k as f64);

                for atom in &structure.atoms {
                    let cart_orig = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);
                    let lat_inv = mat_orig.try_inverse().ok_or("Singular original lattice")?;
                    let frac_orig = lat_inv.transpose() * cart_orig;

                    let f_shifted = frac_orig + shift;
                    let f_new = inv_m * f_shifted;

                    // Relaxed bounds slightly to catch boundary atoms
                    if f_new.x >= -1e-4 && f_new.x < 0.9999
                    && f_new.y >= -1e-4 && f_new.y < 0.9999
                    && f_new.z >= -1e-4 && f_new.z < 0.9999 {
                        slab_atoms.push((atom.element.clone(), f_new));
                    }
                }
            }
        }
    }

    if slab_atoms.is_empty() { return Err("No atoms mapped to slab".to_string()); }

    // 6. Stack Layers
    let mut final_atoms = Vec::new();
    let t_f = thickness as f64;

    for layer in 0..thickness {
        for (el, f_prim) in &slab_atoms {
            let new_z = (f_prim.z + layer as f64) / t_f;
            final_atoms.push((el.clone(), Vector3::new(f_prim.x, f_prim.y, new_z)));
        }
    }

    // 7. Add Vacuum (FIXED INDEXING)
    let mut lat_slab = lat_new_prim;
    // Scale the 'c' vector (Row 2) by thickness
    let c_row = lat_slab.row(2) * t_f;
    lat_slab.set_row(2, &c_row);

    // Extract correct rows (Vectors A, B, C)
    let a_vec = Vector3::new(lat_slab[(0,0)], lat_slab[(0,1)], lat_slab[(0,2)]);
    let b_vec = Vector3::new(lat_slab[(1,0)], lat_slab[(1,1)], lat_slab[(1,2)]);
    let c_vec = Vector3::new(lat_slab[(2,0)], lat_slab[(2,1)], lat_slab[(2,2)]);

    // Normal to the surface (Cross product of new A and B)
    let normal = a_vec.cross(&b_vec).normalize();

    // New C = Old C + Vacuum * Normal
    let c_new = c_vec + normal * vacuum;

    lat_slab[(2,0)] = c_new.x;
    lat_slab[(2,1)] = c_new.y;
    lat_slab[(2,2)] = c_new.z;

    // 8. Final Cartesian Conversion & Re-scaling
    let old_c_proj = c_vec.dot(&normal);
    let new_c_proj = c_new.dot(&normal);
    let scale_ratio = old_c_proj / new_c_proj;

    let mut real_atoms = Vec::new();
    for (i, (el, mut frac)) in final_atoms.into_iter().enumerate() {
        frac.z *= scale_ratio; // Compress Z to keep atoms at bottom
        let cart = lat_slab.transpose() * frac;
        real_atoms.push(Atom {
            element: el,
            position: [cart.x, cart.y, cart.z],
            original_index: i,
        });
    }

    let new_lattice_arr = [
        [lat_slab[(0,0)], lat_slab[(0,1)], lat_slab[(0,2)]],
        [lat_slab[(1,0)], lat_slab[(1,1)], lat_slab[(1,2)]],
        [lat_slab[(2,0)], lat_slab[(2,1)], lat_slab[(2,2)]],
    ];

    Ok(Structure {
        lattice: new_lattice_arr,
        atoms: real_atoms,
        formula: format!("Slab ({}{}{})", h, k, l),
    })
}

// Helper: Vector3<i32> Cross Product
fn cross_prod(a: Vector3<i32>, b: Vector3<i32>) -> Vector3<i32> {
    Vector3::new(
        a.y*b.z - a.z*b.y,
        a.z*b.x - a.x*b.z,
        a.x*b.y - a.y*b.x
    )
}
