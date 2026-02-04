// src/rendering/primitives.rs

use super::scene::RenderAtom;
use gtk4::cairo::{self, Context, Format, ImageSurface, RadialGradient};
use std::f64::consts::PI;

// Make fields public so painter.rs can access them
#[derive(Clone)]
pub struct RenderBond {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub radius: f64,
}

pub enum RenderPrimitive<'a> {
    Atom(&'a RenderAtom),
    Bond(RenderBond),
}

impl<'a> RenderPrimitive<'a> {
    pub fn z_depth(&self) -> f64 {
        match self {
            RenderPrimitive::Atom(atom) => atom.screen_pos[2],
            RenderPrimitive::Bond(bond) => (bond.start[2] + bond.end[2]) / 2.0,
        }
    }
}

pub fn draw_atom_vector(cr: &cairo::Context, x: f64, y: f64, radius: f64, color: (f64, f64, f64)) {
    let (r, g, b) = color;

    // 1. Create a Radial Gradient to simulate 3D lighting
    // Inner circle (highlight): offset to top-left (-radius/3)
    // Outer circle (shadow): centered
    let gradient = RadialGradient::new(
        x - radius * 0.3,
        y - radius * 0.3,
        radius * 0.1, // Highlight position/size
        x,
        y,
        radius, // Base sphere position/size
    );

    // "Shininess" (Highlight) -> Base Color -> Shadow
    gradient.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0); // White highlight
    gradient.add_color_stop_rgb(0.2, r + 0.2, g + 0.2, b + 0.2); // Lighter base
    gradient.add_color_stop_rgb(1.0, r * 0.6, g * 0.6, b * 0.6); // Darker shadow

    // 2. Draw the Circle
    cr.set_source(&gradient).unwrap();
    cr.arc(x, y, radius, 0.0, 2.0 * std::f64::consts::PI);
    cr.fill().unwrap();

    // Optional: Thin outline for extra crispness in PDF
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.3);
    cr.set_line_width(radius * 0.05);
    cr.arc(x, y, radius, 0.0, 2.0 * std::f64::consts::PI);
    cr.stroke().unwrap();
}

/// Generates a high-quality 128x128 image of an atom.
pub fn create_atom_sprite(
    r: f64,
    g: f64,
    b: f64,
    metallic: f64,
    roughness: f64,
    transmission: f64,
) -> ImageSurface {
    let size = 128;
    let surface =
        ImageSurface::create(Format::ARgb32, size, size).expect("Failed to create sprite surface");
    let cr = Context::new(&surface).expect("Failed to create sprite context");

    let center = size as f64 / 2.0;
    let radius = size as f64 / 2.0;

    let (red, green, blue) = (r, g, b);
    let alpha = 1.0 - transmission;

    let spec_r = 1.0 + (red - 1.0) * metallic;
    let spec_g = 1.0 + (green - 1.0) * metallic;
    let spec_b = 1.0 + (blue - 1.0) * metallic;

    let highlight_size = 0.05 + roughness * 0.35;
    let light_offset = 0.25;

    let pat = cairo::RadialGradient::new(
        center - radius * light_offset,
        center - radius * light_offset,
        radius * highlight_size,
        center,
        center,
        radius,
    );

    let shine_alpha = (1.0 - roughness * 0.5) * alpha;
    pat.add_color_stop_rgba(0.0, spec_r, spec_g, spec_b, shine_alpha);

    let lit_pos = 0.1 + roughness * 0.2;
    pat.add_color_stop_rgba(lit_pos, red, green, blue, alpha);

    let ambient_level = 0.4 - (metallic * 0.3);
    pat.add_color_stop_rgba(
        0.85,
        red * ambient_level,
        green * ambient_level,
        blue * ambient_level,
        alpha,
    );

    let rim_darkness = 0.1 * (1.0 - transmission);
    pat.add_color_stop_rgba(
        1.0,
        red * rim_darkness,
        green * rim_darkness,
        blue * rim_darkness,
        alpha,
    );

    cr.set_source(&pat).unwrap();
    cr.arc(center, center, radius, 0.0, 2.0 * PI);
    cr.fill().unwrap();

    surface
}

pub fn draw_cylinder_impostor(
    cr: &cairo::Context,
    p1: [f64; 3],
    p2: [f64; 3],
    radius: f64,
    color: (f64, f64, f64),
    metallic: f64,
    roughness: f64,
    transmission: f64,
) {
    let dx = p2[0] - p1[0];
    let dy = p2[1] - p1[1];
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.0001 {
        return;
    }

    let nx = -dy / len_sq.sqrt();
    let ny = dx / len_sq.sqrt();

    let c1x = p1[0] + nx * radius;
    let c1y = p1[1] + ny * radius;
    let c2x = p2[0] + nx * radius;
    let c2y = p2[1] + ny * radius;
    let c3x = p2[0] - nx * radius;
    let c3y = p2[1] - ny * radius;
    let c4x = p1[0] - nx * radius;
    let c4y = p1[1] - ny * radius;

    let gradient = cairo::LinearGradient::new(c1x, c1y, c4x, c4y);
    let (r, g, b) = color;
    let alpha = 1.0 - transmission;

    let sr = 1.0 + (r - 1.0) * metallic;
    let sg = 1.0 + (g - 1.0) * metallic;
    let sb = 1.0 + (b - 1.0) * metallic;

    let shadow = 0.3 - (metallic * 0.2);

    gradient.add_color_stop_rgba(0.0, r * shadow, g * shadow, b * shadow, alpha);
    gradient.add_color_stop_rgba(0.3, r, g, b, alpha);

    let h_width = 0.05 + roughness * 0.2;
    gradient.add_color_stop_rgba(0.5 - h_width, r, g, b, alpha);
    gradient.add_color_stop_rgba(0.5, sr, sg, sb, alpha * (1.0 - roughness * 0.3));
    gradient.add_color_stop_rgba(0.5 + h_width, r, g, b, alpha);

    gradient.add_color_stop_rgba(0.7, r, g, b, alpha);
    gradient.add_color_stop_rgba(1.0, r * shadow, g * shadow, b * shadow, alpha);

    cr.set_source(&gradient).unwrap();
    cr.move_to(c1x, c1y);
    cr.line_to(c2x, c2y);
    cr.line_to(c3x, c3y);
    cr.line_to(c4x, c4y);
    cr.close_path();
    cr.fill().unwrap();
}
