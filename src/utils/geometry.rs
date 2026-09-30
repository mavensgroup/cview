// src/geometry.rs
use nalgebra::Vector3;

type Point3 = [f64; 3];

/// Calculates distance between two points (Angstroms)
pub fn calculate_distance(p1: Point3, p2: Point3) -> f64 {
  let v1 = Vector3::from(p1);
  let v2 = Vector3::from(p2);
  // nalgebra's metric_distance is |v1 - v2|
  nalgebra::distance(&v1.into(), &v2.into())
}

/// Calculates angle P1-P2-P3 in degrees
pub fn calculate_angle(p1: Point3, center: Point3, p3: Point3) -> f64 {
  let c = Vector3::from(center);
  let v1 = Vector3::from(p1) - c;
  let v2 = Vector3::from(p3) - c;

  // angle() handles normalization and clamping safely internally
  v1.angle(&v2).to_degrees()
}

/// Calculates torsion (dihedral) angle P1-P2-P3-P4 in degrees
pub fn calculate_dihedral(p1: Point3, p2: Point3, p3: Point3, p4: Point3) -> f64 {
  let v1 = Vector3::from(p1);
  let v2 = Vector3::from(p2);
  let v3 = Vector3::from(p3);
  let v4 = Vector3::from(p4);

  let b1 = v2 - v1;
  let b2 = v3 - v2;
  let b3 = v4 - v3;

  // Normal to plane (p1, p2, p3)
  let n1 = b1.cross(&b2);
  // Normal to plane (p2, p3, p4)
  let n2 = b2.cross(&b3);

  // Calculate angle using atan2 for the correct sign
  // x = dot(n1, n2)
  // y = dot(normalize(b2), cross(n1, n2))

  // Safety: Handle the case where b2 is zero length to avoid NaN
  let b2_u = b2.try_normalize(1e-6).unwrap_or(Vector3::zeros());

  let x = n1.dot(&n2);
  let y = b2_u.dot(&n1.cross(&n2));

  y.atan2(x).to_degrees()
}

/// Shortest periodic image of the Cartesian separation `d` (Å) in a cell
/// whose rows are the lattice vectors a, b, c.
///
/// Returns the shortened vector and the integer translation `n` that was
/// subtracted from it (`d - n·[a, b, c]`). `None` for a singular lattice.
///
/// Rounding the fractional separation alone is not enough in a skewed cell,
/// so the 27 translations around the rounded one are compared as well.
pub fn minimum_image(d: Point3, lattice: &[[f64; 3]; 3]) -> Option<(Point3, [i32; 3])> {
  let rows = nalgebra::Matrix3::from_row_slice(&[
    lattice[0][0], lattice[0][1], lattice[0][2],
    lattice[1][0], lattice[1][1], lattice[1][2],
    lattice[2][0], lattice[2][1], lattice[2][2],
  ]);
  // Cartesian = Lᵀ · fractional when the rows of L are the cell vectors.
  let to_cart = rows.transpose();
  let to_frac = to_cart.try_inverse()?;
  let dv = Vector3::from(d);
  let f = to_frac * dv;
  let base = [f.x.round() as i32, f.y.round() as i32, f.z.round() as i32];

  let mut best: Option<(f64, Vector3<f64>, [i32; 3])> = None;
  for i in -1..=1 {
    for j in -1..=1 {
      for k in -1..=1 {
        let n = [base[0] + i, base[1] + j, base[2] + k];
        let shift = to_cart * Vector3::new(n[0] as f64, n[1] as f64, n[2] as f64);
        let v = dv - shift;
        let len = v.norm_squared();
        if best.map_or(true, |(b, _, _)| len < b) {
          best = Some((len, v, n));
        }
      }
    }
  }
  best.map(|(_, v, n)| ([v.x, v.y, v.z], n))
}

/// Position of the periodic image of `p2` nearest to `p1`, when that image is
/// measurably closer than `p2` itself; `None` when `p2` already is the
/// nearest copy. Also returns the cell translation applied to `p2`.
pub fn closer_periodic_image(
  p1: Point3,
  p2: Point3,
  lattice: &[[f64; 3]; 3],
) -> Option<(Point3, [i32; 3])> {
  let d = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
  let (v, n) = minimum_image(d, lattice)?;
  let direct = Vector3::from(d).norm();
  let shortest = Vector3::from(v).norm();
  if n == [0, 0, 0] || direct - shortest < 1e-4 {
    return None;
  }
  Some(([p1[0] + v[0], p1[1] + v[1], p1[2] + v[2]], [-n[0], -n[1], -n[2]]))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn minimum_image_wraps_across_cubic_cell() {
    let lat = [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]];
    let (v, n) = minimum_image([4.0, 0.0, 0.0], &lat).unwrap();
    assert!((v[0] + 1.0).abs() < 1e-12);
    assert_eq!(n, [1, 0, 0]);
  }

  #[test]
  fn minimum_image_leaves_short_vector_alone() {
    let lat = [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]];
    let (v, n) = minimum_image([1.0, 2.0, -2.0], &lat).unwrap();
    assert_eq!(v, [1.0, 2.0, -2.0]);
    assert_eq!(n, [0, 0, 0]);
  }

  #[test]
  fn minimum_image_handles_skewed_cell() {
    // Strongly sheared cell: a naive rounding of the fractional separation
    // picks a longer image than the true shortest one.
    let lat = [[4.0, 0.0, 0.0], [3.6, 1.0, 0.0], [0.0, 0.0, 10.0]];
    let d = [0.4, 1.0, 0.0];
    let (v, _) = minimum_image(d, &lat).unwrap();
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    // Brute force over a wide range of translations.
    let mut brute = f64::MAX;
    for i in -4..=4 {
      for j in -4..=4 {
        let x = d[0] - i as f64 * 4.0 - j as f64 * 3.6;
        let y = d[1] - j as f64 * 1.0;
        brute = brute.min((x * x + y * y).sqrt());
      }
    }
    assert!((len - brute).abs() < 1e-12, "got {len}, brute force {brute}");
  }

  #[test]
  fn minimum_image_rejects_singular_lattice() {
    let lat = [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(minimum_image([0.5, 0.0, 0.0], &lat).is_none());
  }
}
