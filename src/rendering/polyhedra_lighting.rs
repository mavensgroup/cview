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

// ── Colour helpers ────────────────────────────────────────────────────────────

/// Pull a colour toward its grey by `amount` (0 = unchanged, 1 = grey).
pub fn desaturate(rgb: (f64, f64, f64), amount: f64) -> (f64, f64, f64) {
    let lum = 0.299 * rgb.0 + 0.587 * rgb.1 + 0.114 * rgb.2;
    let a = amount.clamp(0.0, 1.0);
    (
        rgb.0 + a * (lum - rgb.0),
        rgb.1 + a * (lum - rgb.1),
        rgb.2 + a * (lum - rgb.2),
    )
}

/// Face brightness: full Lambertian contrast at `strength` 1, flat at 0.
fn brightness(normal: Vector3<f64>, strength: f64) -> f64 {
    1.0 - strength.clamp(0.0, 1.0) * (1.0 - lambertian(normal))
}

// ── Cairo draw ────────────────────────────────────────────────────────────────

/// Everything needed to draw one triangular face.
pub struct FaceDraw<'a> {
    /// [x, y, z(depth)] per vertex: x/y for drawing, z for depth sort only.
    pub screen_verts: &'a [[f64; 3]],
    /// Cartesian vertices and polyhedron centre: used for the lighting normal.
    pub cart_verts: [[f64; 3]; 3],
    pub poly_center_cart: [f64; 3],
    pub color: (f64, f64, f64),
    /// Effective opacity for this face (already scaled for back faces).
    pub alpha: f64,
    /// Which of the three edges (v0v1, v1v2, v2v0) are real polyhedron edges
    /// rather than triangulation diagonals across a flat face.
    pub edge_flags: [bool; 3],
    pub style: &'a crate::config::PolyhedraStyle,
}

/// Draw a shaded triangle, then its real edges as the style asks.
pub fn draw_shaded_face(cr: &cairo::Context, f: &FaceDraw) {
    use crate::config::EdgeStyle;
    if f.screen_verts.len() < 3 {
        return;
    }
    let normal = face_normal(
        f.cart_verts[0],
        f.cart_verts[1],
        f.cart_verts[2],
        f.poly_center_cart,
    );
    let b = brightness(normal, f.style.shading);
    let shaded = shade_color(f.color, b);

    cr.new_path();
    cr.move_to(f.screen_verts[0][0], f.screen_verts[0][1]);
    for v in &f.screen_verts[1..3] {
        cr.line_to(v[0], v[1]);
    }
    cr.close_path();
    cr.set_source_rgba(shaded.0, shaded.1, shaded.2, f.alpha);
    cr.fill().ok();

    let (width, darken, edge_alpha) = match f.style.edge_style {
        EdgeStyle::None => return,
        EdgeStyle::Subtle => (0.8, 0.72, (f.alpha + 0.25).clamp(0.35, 0.8)),
        EdgeStyle::Strong => (1.5, 0.42, (f.alpha + 0.5).clamp(0.6, 0.95)),
    };
    cr.set_source_rgba(
        shaded.0 * darken,
        shaded.1 * darken,
        shaded.2 * darken,
        edge_alpha,
    );
    cr.set_line_width(width);
    for k in 0..3 {
        if f.edge_flags[k] {
            let (v0, v1) = (f.screen_verts[k], f.screen_verts[(k + 1) % 3]);
            cr.move_to(v0[0], v0[1]);
            cr.line_to(v1[0], v1[1]);
        }
    }
    cr.stroke().ok();
}
