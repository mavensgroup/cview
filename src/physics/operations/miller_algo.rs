// src/physics/operations/miller_algo.rs
//
use nalgebra::Vector3;

fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 {
        a.abs().max(1)
    } else {
        gcd(b, a % b)
    }
}

#[derive(Clone, Copy)]
pub struct MillerMath {
    pub h: i32,
    pub k: i32,
    pub l: i32,
}

impl MillerMath {
    pub fn new(h: i32, k: i32, l: i32) -> Self {
        Self { h, k, l }
    }

    // NOTE: there is intentionally no `normal()` here. The true plane
    // normal is h·b1 + k·b2 + l·b3 (reciprocal basis) — the naive
    // normalized (h,k,l) is only correct for cubic cells. Compute the
    // normal from the actual lattice where it is needed (slab.rs does).

    // ==========================================
    // PART A: PHYSICS ENGINE
    // ==========================================
    /// Integer basis (u, v, w) with u, v spanning the (hkl) plane and w a
    /// stacking vector. Non-coprime indices — (200), (220) — are reduced by
    /// their gcd first: they denote the same plane orientation, and the
    /// Bézout condition h·x + k·y + l·z = 1 is unsolvable without the
    /// reduction. Note the u/v choice minimizes the INDEX-space norm, not
    /// the physical length |A·u|, so strongly anisotropic lattices may get
    /// a more skewed surface cell than necessary (valid, just not reduced).
    pub fn find_basis(&self) -> Result<(Vector3<i32>, Vector3<i32>, Vector3<i32>), String> {
        if self.h == 0 && self.k == 0 && self.l == 0 {
            return Err("Miller indices cannot be (0,0,0)".to_string());
        }

        let g = gcd(gcd(self.h, self.k), self.l);
        let (h, k, l) = (self.h / g, self.k / g, self.l / g);
        if g > 1 {
            crate::utils::console::log_info(&format!(
                "Miller indices ({},{},{}) reduced to ({h},{k},{l}) — identical plane orientation",
                self.h, self.k, self.l
            ));
        }

        // 1. Find Surface Vectors (u, v)
        let limit = 10;
        let mut candidates = Vec::new();

        for x in -limit..=limit {
            for y in -limit..=limit {
                for z in -limit..=limit {
                    if x == 0 && y == 0 && z == 0 {
                        continue;
                    }

                    if h * x + k * y + l * z == 0 {
                        candidates.push(Vector3::new(x, y, z));
                    }
                }
            }
        }

        // FIX: Use dot product for integer squared magnitude
        candidates.sort_by_key(|a| a.dot(a));

        if candidates.is_empty() {
            return Err("Could not find surface vectors. Indices might be too high.".to_string());
        }

        let u_vec = candidates[0];

        // Find v_vec: The shortest vector NOT parallel to u_vec
        let mut v_vec = Vector3::zeros();
        let mut found_v = false;

        for cand in candidates.iter().skip(1) {
            let cp = u_vec.cross(cand);
            // Check against zero vector
            if cp != Vector3::zeros() {
                v_vec = *cand;
                found_v = true;
                break;
            }
        }

        if !found_v {
            return Err("Could not define primitive surface unit cell.".to_string());
        }

        // 2. Find Stacking Vector (w)
        let mut w_vec = Vector3::zeros();
        let mut found_w = false;
        let w_limit = 10;

        let mut w_candidates = Vec::new();

        for x in -w_limit..=w_limit {
            for y in -w_limit..=w_limit {
                for z in -w_limit..=w_limit {
                    if h * x + k * y + l * z == 1 {
                        w_candidates.push(Vector3::new(x, y, z));
                    }
                }
            }
        }

        // FIX: Use dot product for integer squared magnitude
        w_candidates.sort_by_key(|a| a.dot(a));

        if !w_candidates.is_empty() {
            w_vec = w_candidates[0];
            found_w = true;
        }

        if !found_w {
            return Err("Could not find valid stacking vector for these indices.".to_string());
        }

        Ok((u_vec, v_vec, w_vec))
    }

    // ==========================================
    // PART B: VISUALIZATION ENGINE
    // ==========================================
    pub fn get_intersection_polygon(&self) -> Vec<[f64; 3]> {
        if self.h == 0 && self.k == 0 && self.l == 0 {
            return vec![];
        }

        let h = self.h as f64;
        let k = self.k as f64;
        let l = self.l as f64;

        let edges = [
            ([0., 0., 0.], [1., 0., 0.]),
            ([0., 0., 0.], [0., 1., 0.]),
            ([0., 0., 0.], [0., 0., 1.]),
            ([1., 0., 0.], [0., 1., 0.]),
            ([1., 0., 0.], [0., 0., 1.]),
            ([0., 1., 0.], [1., 0., 0.]),
            ([0., 1., 0.], [0., 0., 1.]),
            ([0., 0., 1.], [1., 0., 0.]),
            ([0., 0., 1.], [0., 1., 0.]),
            ([1., 1., 0.], [0., 0., 1.]),
            ([1., 0., 1.], [0., 1., 0.]),
            ([0., 1., 1.], [1., 0., 0.]),
        ];

        let mut points = Vec::new();

        for (start, dir) in edges.iter() {
            let start_val = h * start[0] + k * start[1] + l * start[2];
            let dir_val = h * dir[0] + k * dir[1] + l * dir[2];

            if dir_val.abs() > 1e-6 {
                let t = (1.0 - start_val) / dir_val;
                if (-0.0001..=1.0001).contains(&t) {
                    points.push([
                        start[0] + t * dir[0],
                        start[1] + t * dir[1],
                        start[2] + t * dir[2],
                    ]);
                }
            }
        }

        if points.len() < 3 {
            return vec![];
        }

        let cx: f64 = points.iter().map(|p| p[0]).sum::<f64>() / points.len() as f64;
        let cy: f64 = points.iter().map(|p| p[1]).sum::<f64>() / points.len() as f64;
        let cz: f64 = points.iter().map(|p| p[2]).sum::<f64>() / points.len() as f64;
        let centroid = Vector3::new(cx, cy, cz);

        let n = Vector3::new(h, k, l).normalize();

        let mut u = if n.x.abs() < 0.9 {
            Vector3::new(1.0, 0.0, 0.0)
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
        u = n.cross(&u).normalize();
        let v = n.cross(&u).normalize();

        points.sort_by(|a, b| {
            let vec_a = Vector3::new(a[0], a[1], a[2]) - centroid;
            let vec_b = Vector3::new(b[0], b[1], b[2]) - centroid;

            let ang_a = vec_a.dot(&v).atan2(vec_a.dot(&u));
            let ang_b = vec_b.dot(&v).atan2(vec_b.dot(&u));

            ang_a
                .partial_cmp(&ang_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        points.dedup_by(|a, b| {
            (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5 && (a[2] - b[2]).abs() < 1e-5
        });

        points
    }
}
