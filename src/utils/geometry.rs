// src/geometry.rs

type Point3 = [f64; 3];

/// Calculates distance between two points (Angstroms)
pub fn calculate_distance(p1: Point3, p2: Point3) -> f64 {
    let diff = sub(p1, p2);
    len(diff)
}

/// Calculates angle P1-P2-P3 in degrees
pub fn calculate_angle(p1: Point3, center: Point3, p3: Point3) -> f64 {
    let v1 = normalize(sub(p1, center));
    let v2 = normalize(sub(p3, center));
    dot(v1, v2).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Calculates torsion (dihedral) angle P1-P2-P3-P4 in degrees
pub fn calculate_dihedral(p1: Point3, p2: Point3, p3: Point3, p4: Point3) -> f64 {
    let b1 = sub(p2, p1);
    let b2 = sub(p3, p2);
    let b3 = sub(p4, p3);

    // Normalize b2 for projection
    let b2_u = normalize(b2);

    // v = vector perpendicular to plane defined by b1, b2
    let v = cross(b1, b2);
    // w = vector perpendicular to plane defined by b2, b3
    let w = cross(b2, b3);

    let x = dot(v, w);
    let y = dot(b2_u, cross(v, w));

    y.atan2(x).to_degrees()
}

// --- Internal Math Helpers for [f64; 3] ---

fn sub(a: Point3, b: Point3) -> Point3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Point3, b: Point3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Point3, b: Point3) -> Point3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len(a: Point3) -> f64 {
    dot(a, a).sqrt()
}

fn normalize(a: Point3) -> Point3 {
    let l = len(a);
    if l == 0.0 { [0.0, 0.0, 0.0] } else { [a[0] / l, a[1] / l, a[2] / l] }
}
