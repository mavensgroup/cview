// src/physics/slab.rs
use crate::model::structure::{Structure, Atom};
use crate::physics::miller_math::find_plane_basis; // <--- Import logic
use nalgebra::{Matrix3, Vector3};

pub fn generate_slab(
    structure: &Structure,
    h: i32, k: i32, l: i32,
    thickness: u32,
    vacuum: f64
) -> Result<Structure, String> {

    // 1. Get Basis using shared math
    let (u_vec, v_vec, w_vec) = find_plane_basis(h, k, l, structure.lattice)?;

    let mat_orig = Matrix3::new(
        structure.lattice[0][0], structure.lattice[0][1], structure.lattice[0][2],
        structure.lattice[1][0], structure.lattice[1][1], structure.lattice[1][2],
        structure.lattice[2][0], structure.lattice[2][1], structure.lattice[2][2],
    );

    // 2. Transformation Matrix
    let m_int = Matrix3::new(
        u_vec.x as f64, v_vec.x as f64, w_vec.x as f64,
        u_vec.y as f64, v_vec.y as f64, w_vec.y as f64,
        u_vec.z as f64, v_vec.z as f64, w_vec.z as f64,
    );

    let det = m_int.determinant().abs();
    if det < 1e-6 { return Err("Singular transformation matrix".to_string()); }

    let lat_new_prim = m_int.transpose() * mat_orig;    // 5. Map Atoms
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
// fn cross_prod(a: Vector3<i32>, b: Vector3<i32>) -> Vector3<i32> {
    // Vector3::new(
        // a.y*b.z - a.z*b.y,
        // a.z*b.x - a.x*b.z,
        // a.x*b.y - a.y*b.x
    // )
// }
