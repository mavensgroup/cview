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

// --- Internal Math Helpers ---
// WE NO LONGER NEED THESE!
// nalgebra::Vector3 handles sub, dot, cross, len, and normalize.
