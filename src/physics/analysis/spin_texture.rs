// src/physics/analysis/spin_texture.rs
//
// Numbers that describe a magnetization texture on a regular mesh (an OVF
// file): the topological charge Q and the size of the reversed core.
//
// Q is the lattice version of (1/4π) ∫ m · (∂ₐm × ∂ᵦm) da db, after Berg
// and Lüscher (Nucl. Phys. B 190, 412 (1981)): each plaquette is split
// into two triangles and the solid angle the three unit vectors span is
// summed. On a lattice it is exact: for a smooth texture it lands on an
// integer up to discretisation at the edges, and it never needs
// derivatives. (a, b) are the in-plane axes taken right-handed about the
// film normal, so a skyrmion with its core along +n in a −n background has
// Q = +1.

/// Q and core size of one magnetization frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureStats {
    /// Topological charge, averaged over the layers of the film.
    pub q: f64,
    /// Area whose normal component is opposite to the background (the
    /// sign of the mean normal component), per layer, Å².
    pub core_area: f64,
}

/// Solid angle of the spherical triangle spanned by three unit vectors,
/// signed by their orientation (Oosterom–Strackee).
fn solid_angle(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let cross = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
    2.0 * dot(a, cross).atan2(1.0 + dot(a, b) + dot(b, c) + dot(c, a))
}

/// Q and core area of the moments `m` on the sites `idx` of a mesh with
/// `counts` cells along each axis, `normal` its film normal and
/// `cell_area` the in-plane area of one cell (Å²). Sites with no moment
/// (outside the magnet's shape) are skipped, as are triangles touching
/// them. None when no layer has a single plaquette.
pub fn analyse(idx: &[[i64; 3]], counts: [i64; 3], normal: usize, cell_area: f64, m: &[[f64; 3]]) -> Option<TextureStats> {
    let (a, b) = match normal {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let (na, nb, nl) = (counts[a], counts[b], counts[normal]);
    if na < 2 || nb < 2 || idx.len() != m.len() {
        return None;
    }
    let at = |ia: i64, ib: i64, il: i64| ((il * nb + ib) * na + ia) as usize;
    let mut unit: Vec<Option<[f64; 3]>> = vec![None; (na * nb * nl) as usize];
    for (p, v) in idx.iter().zip(m) {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if n > 1e-6 && (0..3).all(|d| p[d] >= 0 && p[d] < counts[d]) {
            unit[at(p[a], p[b], p[normal])] = Some(v.map(|x| x / n));
        }
    }

    let (mut q_sum, mut area_sum, mut layers) = (0.0, 0.0, 0usize);
    for il in 0..nl {
        let mut omega = 0.0;
        let mut any = false;
        for ib in 0..nb - 1 {
            for ia in 0..na - 1 {
                let s00 = unit[at(ia, ib, il)];
                let s10 = unit[at(ia + 1, ib, il)];
                let s11 = unit[at(ia + 1, ib + 1, il)];
                let s01 = unit[at(ia, ib + 1, il)];
                // Counter-clockwise in (a, b): 00 → 10 → 11 and 00 → 11 → 01.
                if let (Some(p), Some(q), Some(r)) = (s00, s10, s11) {
                    omega += solid_angle(p, q, r);
                    any = true;
                }
                if let (Some(p), Some(q), Some(r)) = (s00, s11, s01) {
                    omega += solid_angle(p, q, r);
                    any = true;
                }
            }
        }
        if !any {
            continue;
        }
        let layer = || (0..nb).flat_map(move |ib| (0..na).map(move |ia| (ia, ib))).filter_map(|(ia, ib)| unit[at(ia, ib, il)]);
        let mean_n: f64 = layer().map(|u| u[normal]).sum();
        let s = if mean_n < 0.0 { -1.0 } else { 1.0 };
        let reversed = layer().filter(|u| s * u[normal] < 0.0).count();
        q_sum += omega / (4.0 * std::f64::consts::PI);
        area_sum += reversed as f64 * cell_area;
        layers += 1;
    }
    (layers > 0).then(|| TextureStats { q: q_sum / layers as f64, core_area: area_sum / layers as f64 })
}

/// Radius of a disc of the core area, Å, when the frame holds a single
/// skyrmion (|Q| rounds to 1) — for anything else one radius means nothing.
pub fn core_radius(stats: &TextureStats) -> Option<f64> {
    (stats.q.abs().round() == 1.0 && stats.core_area > 0.0).then(|| (stats.core_area / std::f64::consts::PI).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An n×n film with a skyrmion of radius r (cells) and wall width w at
    /// its centre: core along +z when `up`, Néel or Bloch.
    fn skyrmion(n: i64, r: f64, w: f64, up: bool, bloch: bool) -> (Vec<[i64; 3]>, Vec<[f64; 3]>) {
        let c = (n - 1) as f64 / 2.0;
        let mut idx = Vec::new();
        let mut m = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (i as f64 - c, j as f64 - c);
                let rr = x.hypot(y);
                let phi = y.atan2(x) + if bloch { std::f64::consts::FRAC_PI_2 } else { 0.0 };
                // θ: 0 at the core, π far away.
                let th = 2.0 * ((rr - r) / w).exp().atan();
                let s = if up { 1.0 } else { -1.0 };
                idx.push([i, j, 0]);
                m.push([th.sin() * phi.cos(), th.sin() * phi.sin(), s * th.cos()]);
            }
        }
        (idx, m)
    }

    #[test]
    fn a_skyrmion_has_unit_charge_signed_by_its_core() {
        for bloch in [false, true] {
            let (idx, m) = skyrmion(64, 10.0, 3.0, true, bloch);
            let up = analyse(&idx, [64, 64, 1], 2, 1.0, &m).unwrap();
            assert!((up.q - 1.0).abs() < 0.01, "core up, bloch={bloch}: Q = {}", up.q);
            let (idx, m) = skyrmion(64, 10.0, 3.0, false, bloch);
            let down = analyse(&idx, [64, 64, 1], 2, 1.0, &m).unwrap();
            assert!((down.q + 1.0).abs() < 0.01, "core down, bloch={bloch}: Q = {}", down.q);
        }
    }

    #[test]
    fn core_radius_follows_the_mz_zero_contour() {
        let (idx, m) = skyrmion(80, 15.0, 3.0, true, false);
        let s = analyse(&idx, [80, 80, 1], 2, 4.0, &m).unwrap(); // 2 Å cells
        let r = core_radius(&s).unwrap();
        assert!((r - 30.0).abs() < 2.0, "R = {r} Å");
    }

    #[test]
    fn uniform_and_missing_cells_give_zero() {
        let idx: Vec<[i64; 3]> = (0..100).map(|k| [k % 10, k / 10, 0]).collect();
        let mut m = vec![[0.0, 0.0, 1.0]; 100];
        m[55] = [0.0; 3]; // a hole
        let s = analyse(&idx, [10, 10, 1], 2, 1.0, &m).unwrap();
        assert!(s.q.abs() < 1e-12 && s.core_area == 0.0);
        assert!(core_radius(&s).is_none());
    }

    #[test]
    fn layers_are_averaged_and_any_normal_works() {
        // The same skyrmion in two layers of a film whose normal is x.
        let (idx2, m2) = skyrmion(40, 6.0, 2.0, true, false);
        let mut idx = Vec::new();
        let mut m = Vec::new();
        for l in 0..2 {
            for (p, v) in idx2.iter().zip(&m2) {
                // (a, b, n) = (y, z, x): in-plane i → y, j → z.
                idx.push([l, p[0], p[1]]);
                m.push([v[2], v[0], v[1]]);
            }
        }
        let s = analyse(&idx, [2, 40, 40], 0, 1.0, &m).unwrap();
        assert!((s.q - 1.0).abs() < 0.02, "Q = {}", s.q);
    }
}
