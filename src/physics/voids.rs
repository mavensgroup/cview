use crate::model::structure::Structure;
use nalgebra::{Matrix3, Vector3};
// FIX: Use get_atom_properties to get Covalent Radii (much smaller than vdW)
use crate::model::elements::get_atom_properties;

pub struct VoidResult {
    pub max_sphere_radius: f64,
    pub max_sphere_center: [f64; 3],
    pub void_fraction: f64,
}

pub fn calculate_voids(structure: &Structure, resolution: f64) -> VoidResult {
    let lat = structure.lattice;

    // Construct Basis Matrix where Columns are Lattice Vectors (a, b, c)
    // nalgebra Matrix3::new takes arguments in Row-Major order.
    // We want Col 1 = a, Col 2 = b, Col 3 = c.
    // So Row 1 = [ax, bx, cx], Row 2 = [ay, by, cy], etc.
    let basis = Matrix3::new(
        lat[0][0], lat[1][0], lat[2][0],
        lat[0][1], lat[1][1], lat[2][1],
        lat[0][2], lat[1][2], lat[2][2],
    );

    // Grid Dimensions
    let a_len = (lat[0][0].powi(2) + lat[0][1].powi(2) + lat[0][2].powi(2)).sqrt();
    let b_len = (lat[1][0].powi(2) + lat[1][1].powi(2) + lat[1][2].powi(2)).sqrt();
    let c_len = (lat[2][0].powi(2) + lat[2][1].powi(2) + lat[2][2].powi(2)).sqrt();

    let nx = (a_len / resolution).ceil() as usize;
    let ny = (b_len / resolution).ceil() as usize;
    let nz = (c_len / resolution).ceil() as usize;

    let mut max_dist = 0.0f64;
    let mut best_point = [0.0, 0.0, 0.0];
    let mut void_count = 0;
    let total_points = nx * ny * nz;

    // Cache fractional coords and radii
    let atoms_frac: Vec<(Vector3<f64>, f64)> = structure.atoms.iter().map(|a| {
        let cart = Vector3::new(a.position[0], a.position[1], a.position[2]);
        let frac = basis.try_inverse().unwrap() * cart;

        // FIX: Use Covalent Radius (get_atom_properties().0)
        // This is crucial for dense crystals like Perovskites.
        let (r, _) = get_atom_properties(&a.element);

        (frac, r)
    }).collect();

    for i in 0..nx {
        for j in 0..ny {
            for k in 0..nz {
                // Current Point in Fractional Coords [0, 1]
                let pt_f = Vector3::new(
                    i as f64 / nx as f64,
                    j as f64 / ny as f64,
                    k as f64 / nz as f64,
                );

                let mut min_d_to_atom = f64::MAX;

                for (atom_f, r_atom) in &atoms_frac {
                    // Minimum Image Convention (Fractional)
                    let mut dx = pt_f.x - atom_f.x;
                    let mut dy = pt_f.y - atom_f.y;
                    let mut dz = pt_f.z - atom_f.z;

                    dx -= dx.round();
                    dy -= dy.round();
                    dz -= dz.round();

                    let d_frac = Vector3::new(dx, dy, dz);
                    let d_cart = basis * d_frac;

                    let dist_surface = d_cart.norm() - r_atom;
                    if dist_surface < min_d_to_atom {
                        min_d_to_atom = dist_surface;
                    }
                }

                if min_d_to_atom > max_dist {
                    max_dist = min_d_to_atom;
                    let cart = basis * pt_f;
                    best_point = [cart.x, cart.y, cart.z];
                }

                if min_d_to_atom > 0.0 {
                    void_count += 1;
                }
            }
        }
    }

    VoidResult {
        max_sphere_radius: max_dist,
        max_sphere_center: best_point,
        void_fraction: if total_points > 0 {
            (void_count as f64) / (total_points as f64) * 100.0
        } else { 0.0 },
    }
}
