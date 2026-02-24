// src/rendering/polyhedra_lighting.rs
// Lambertian shading for coordination polyhedra.
// All vector math via nalgebra::Vector3 — no hand-rolled helpers.

use gtk4::cairo;
use nalgebra::Vector3;

// ── Light (fixed world space, top-left-front) ────────────────────────────────
fn light() -> Vector3<f64> {
    Vector3::new(0.5, -1.0, 0.8).normalize()
}

const AMBIENT: f64 = 0.35;
const DIFFUSE: f64 = 0.65;

// ── Shading ───────────────────────────────────────────────────────────────────

fn to_vec(v: [f64; 3]) -> Vector3<f64> {
    Vector3::new(v[0], v[1], v[2])
}

/// Outward-facing unit normal of a triangle in Cartesian space.
fn face_normal(v0: [f64; 3], v1: [f64; 3], v2: [f64; 3], center: [f64; 3]) -> Vector3<f64> {
    let e1 = to_vec(v1) - to_vec(v0);
    let e2 = to_vec(v2) - to_vec(v0);
    let mut n = e1.cross(&e2);
    // Ensure outward orientation
    if n.dot(&(to_vec(v0) - to_vec(center))) < 0.0 {
        n = -n;
    }
    n.normalize()
}

/// Lambertian brightness in [AMBIENT, 1.0].
fn lambertian(normal: Vector3<f64>) -> f64 {
    AMBIENT + DIFFUSE * normal.dot(&light()).max(0.0)
}

fn shade_color(rgb: (f64, f64, f64), brightness: f64) -> (f64, f64, f64) {
    (
        (rgb.0 * brightness).clamp(0.0, 1.0),
        (rgb.1 * brightness).clamp(0.0, 1.0),
        (rgb.2 * brightness).clamp(0.0, 1.0),
    )
}

fn edge_color(rgb: (f64, f64, f64), brightness: f64, alpha: f64) -> (f64, f64, f64, f64) {
    let eb = (brightness * 0.55).clamp(0.0, 1.0);
    (
        (rgb.0 * eb).clamp(0.0, 1.0),
        (rgb.1 * eb).clamp(0.0, 1.0),
        (rgb.2 * eb).clamp(0.0, 1.0),
        (alpha * 0.85).clamp(0.0, 1.0),
    )
}

// ── Cairo draw ────────────────────────────────────────────────────────────────

/// Draw a shaded triangle.
/// `screen_verts`: [x, y, z(depth)] — x/y for drawing, z for depth sort only.
/// `cart_verts` + `poly_center_cart`: Cartesian coords used for the lighting normal.
pub fn draw_shaded_face(
    cr: &cairo::Context,
    screen_verts: &[[f64; 3]],
    cart_verts: [[f64; 3]; 3],
    poly_center_cart: [f64; 3],
    base_color: (f64, f64, f64),
    alpha: f64,
    draw_edges: bool,
) {
    if screen_verts.len() < 3 {
        return;
    }

    let normal = face_normal(
        cart_verts[0],
        cart_verts[1],
        cart_verts[2],
        poly_center_cart,
    );
    let brightness = lambertian(normal);
    let shaded = shade_color(base_color, brightness);

    cr.move_to(screen_verts[0][0], screen_verts[0][1]);
    for v in &screen_verts[1..] {
        cr.line_to(v[0], v[1]);
    }
    cr.close_path();

    cr.set_source_rgba(shaded.0, shaded.1, shaded.2, alpha);
    if draw_edges {
        cr.fill_preserve().ok();
        let (er, eg, eb, ea) = edge_color(base_color, brightness, alpha);
        cr.set_source_rgba(er, eg, eb, ea);
        cr.set_line_width(0.8);
        cr.stroke().ok();
    } else {
        cr.fill().ok();
    }
}
