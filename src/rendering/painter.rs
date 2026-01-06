// src/rendering/painter.rs
use gtk4::cairo;
use std::f64::consts::PI;
use std::cmp::Ordering;
use crate::state::AppState;
use crate::elements::get_atom_properties;
use super::scene::RenderAtom;

// --- 1. Primitive Structures ---

#[derive(Clone)]
struct RenderBond {
    start: [f64; 3],
    end:   [f64; 3],
    radius: f64,
}

enum RenderPrimitive<'a> {
    Atom(&'a RenderAtom),
    Bond(RenderBond),
}

impl<'a> RenderPrimitive<'a> {
    fn z_depth(&self) -> f64 {
        match self {
            RenderPrimitive::Atom(atom) => atom.screen_pos[2],
            RenderPrimitive::Bond(bond) => (bond.start[2] + bond.end[2]) / 2.0,
        }
    }
}

// --- 2. Principled BSDF Shaders ---
// (Keep set_principled_gradient and draw_cylinder_impostor exactly as they were)

/// Creates a radial gradient simulating physically based materials
fn set_principled_gradient(
    cr: &cairo::Context,
    cx: f64, cy: f64, r: f64,
    base_color: (f64, f64, f64),
    metallic: f64,
    roughness: f64,
    transmission: f64
) {
    let (red, green, blue) = base_color;
    let alpha = 1.0 - transmission;

    let spec_r = 1.0 + (red - 1.0) * metallic;
    let spec_g = 1.0 + (green - 1.0) * metallic;
    let spec_b = 1.0 + (blue - 1.0) * metallic;

    let highlight_size = 0.05 + roughness * 0.35;
    let light_offset = 0.25;

    let pat = cairo::RadialGradient::new(
        cx - r * light_offset, cy - r * light_offset, r * highlight_size,
        cx, cy, r
    );

    let shine_alpha = (1.0 - roughness * 0.5) * alpha;
    pat.add_color_stop_rgba(0.0, spec_r, spec_g, spec_b, shine_alpha);

    let lit_pos = 0.1 + roughness * 0.2;
    pat.add_color_stop_rgba(lit_pos, red, green, blue, alpha);

    let ambient_level = 0.4 - (metallic * 0.3);
    pat.add_color_stop_rgba(0.85, red * ambient_level, green * ambient_level, blue * ambient_level, alpha);

    let rim_darkness = 0.1 * (1.0 - transmission);
    pat.add_color_stop_rgba(1.0, red * rim_darkness, green * rim_darkness, blue * rim_darkness, alpha);

    cr.set_source(&pat).unwrap();
}

fn draw_cylinder_impostor(
    cr: &cairo::Context,
    p1: [f64; 3], p2: [f64; 3], radius: f64,
    color: (f64, f64, f64),
    metallic: f64, roughness: f64, transmission: f64
) {
    let dx = p2[0] - p1[0];
    let dy = p2[1] - p1[1];
    let len_sq = dx*dx + dy*dy;
    if len_sq < 0.0001 { return; }
    let len = len_sq.sqrt();

    let nx = -dy / len;
    let ny = dx / len;

    let c1x = p1[0] + nx * radius; let c1y = p1[1] + ny * radius;
    let c2x = p2[0] + nx * radius; let c2y = p2[1] + ny * radius;
    let c3x = p2[0] - nx * radius; let c3y = p2[1] - ny * radius;
    let c4x = p1[0] - nx * radius; let c4y = p1[1] - ny * radius;

    let gradient = cairo::LinearGradient::new(c1x, c1y, c4x, c4y);
    let (r, g, b) = color;
    let alpha = 1.0 - transmission;

    let sr = 1.0 + (r - 1.0) * metallic;
    let sg = 1.0 + (g - 1.0) * metallic;
    let sb = 1.0 + (b - 1.0) * metallic;

    let shadow = 0.3 - (metallic * 0.2);

    gradient.add_color_stop_rgba(0.0, r*shadow, g*shadow, b*shadow, alpha);
    gradient.add_color_stop_rgba(0.3, r, g, b, alpha);

    let h_width = 0.05 + roughness * 0.2;
    gradient.add_color_stop_rgba(0.5 - h_width, r, g, b, alpha);
    gradient.add_color_stop_rgba(0.5, sr, sg, sb, alpha * (1.0 - roughness * 0.3));
    gradient.add_color_stop_rgba(0.5 + h_width, r, g, b, alpha);

    gradient.add_color_stop_rgba(0.7, r, g, b, alpha);
    gradient.add_color_stop_rgba(1.0, r*shadow, g*shadow, b*shadow, alpha);

    cr.set_source(&gradient).unwrap();
    cr.move_to(c1x, c1y);
    cr.line_to(c2x, c2y);
    cr.line_to(c3x, c3y);
    cr.line_to(c4x, c4y);
    cr.close_path();
    cr.fill().unwrap();
}

// --- 3. Main Drawing Functions ---

pub fn draw_unit_cell(cr: &cairo::Context, corners: &[[f64; 2]], is_export: bool) {
    if corners.len() != 8 { return; }

    cr.set_source_rgba(0.0, 0.0, 0.0, 1.0);
    cr.set_line_width(if is_export { 2.0 } else { 1.5 });

    let edges = [
        (0,1), (0,2), (0,4), (1,3), (1,5), (2,3),
        (2,6), (4,5), (4,6), (7,6), (7,5), (7,3)
    ];

    for (start, end) in edges {
        let p1 = corners[start];
        let p2 = corners[end];
        cr.move_to(p1[0], p1[1]);
        cr.line_to(p2[0], p2[1]);
        cr.stroke().unwrap();
    }
}

pub fn draw_structure(
    cr: &cairo::Context,
    atoms: &[RenderAtom],
    state: &AppState,
    scale: f64,
    _is_export: bool
) {
    let cutoff_sq = state.bond_cutoff * state.bond_cutoff;
    let mut primitives: Vec<RenderPrimitive> = Vec::with_capacity(atoms.len() * 4);

    // 1. Collect Atoms
    for atom in atoms {
        primitives.push(RenderPrimitive::Atom(atom));
    }

    // 2. Collect Bonds
    for (i, r1) in atoms.iter().enumerate() {
        if r1.is_ghost { continue; }
        for (j, r2) in atoms.iter().enumerate() {
            if i >= j { continue; }

            let v_x = r2.screen_pos[0] - r1.screen_pos[0];
            let v_y = r2.screen_pos[1] - r1.screen_pos[1];
            let v_z = r2.screen_pos[2] - r1.screen_pos[2];

            let d_x = v_x / scale;
            let d_y = v_y / scale;
            let d_z = v_z;

            if (d_x*d_x + d_y*d_y + d_z*d_z) < cutoff_sq {
                let (raw_r1, _) = get_atom_properties(&r1.element);
                let (raw_r2, _) = get_atom_properties(&r2.element);

                let r1_px = raw_r1 * state.style.atom_scale * scale;
                let r2_px = raw_r2 * state.style.atom_scale * scale;

                let full_dist = (v_x*v_x + v_y*v_y + v_z*v_z).sqrt();
                let off1 = r1_px * 0.8;
                let off2 = r2_px * 0.8;

                if full_dist > (off1 + off2) {
                    let t1 = off1 / full_dist;
                    let t2 = off2 / full_dist;

                    let start = [
                        r1.screen_pos[0] + v_x * t1,
                        r1.screen_pos[1] + v_y * t1,
                        r1.screen_pos[2] + v_z * t1,
                    ];
                    let end = [
                        r2.screen_pos[0] - v_x * t2,
                        r2.screen_pos[1] - v_y * t2,
                        r2.screen_pos[2] - v_z * t2,
                    ];

                    primitives.push(RenderPrimitive::Bond(RenderBond {
                        start,
                        end,
                        radius: state.style.bond_radius * scale,
                    }));
                }
            }
        }
    }

    // 3. Sort by Z-Depth (Painter's Algorithm)
    primitives.sort_by(|a, b| {
        a.z_depth().partial_cmp(&b.z_depth()).unwrap_or(Ordering::Equal)
    });

    // 4. Draw Loop
    for prim in primitives {
        match prim {
            RenderPrimitive::Bond(bond) => {
                draw_cylinder_impostor(
                    cr,
                    bond.start, bond.end, bond.radius,
                    state.style.bond_color,
                    state.style.metallic, state.style.roughness, state.style.transmission
                );
            },
            RenderPrimitive::Atom(atom) => {
                let (raw_r, default_rgb) = get_atom_properties(&atom.element);

                let rgb = state.style.element_colors
                    .get(&atom.element)
                    .copied()
                    .unwrap_or(default_rgb);

                let radius = raw_r * state.style.atom_scale * scale;

                // --- NEW: Highlight Selection ---
                // Draw a glow BEHIND the atom if it is selected
                if state.selected_indices.contains(&atom.original_index) {
                    cr.save().unwrap();
                    let highlight_radius = radius + 4.0;

                    // Golden/Yellow Glow
                    cr.set_source_rgba(1.0, 0.85, 0.0, 0.8);
                    cr.arc(atom.screen_pos[0], atom.screen_pos[1], highlight_radius, 0.0, 2.0 * PI);
                    cr.fill().unwrap();

                    // Optional: Thin outline for contrast
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
                    cr.set_line_width(1.5);
                    cr.arc(atom.screen_pos[0], atom.screen_pos[1], highlight_radius, 0.0, 2.0 * PI);
                    cr.stroke().unwrap();

                    cr.restore().unwrap();
                }
                // --------------------------------

                set_principled_gradient(
                    cr,
                    atom.screen_pos[0], atom.screen_pos[1], radius,
                    rgb,
                    state.style.metallic, state.style.roughness, state.style.transmission
                );

                cr.arc(atom.screen_pos[0], atom.screen_pos[1], radius, 0.0, 2.0 * PI);
                cr.fill().unwrap();
            }
        }
    }
}

pub fn draw_axes(cr: &cairo::Context, state: &AppState, width: f64, height: f64) {
    let hud_size = (width * 0.08).clamp(50.0, 150.0);
    let hud_cx = hud_size * 0.8;
    let hud_cy = height - hud_size * 0.8;

    let (sin_x, cos_x) = state.rot_x.sin_cos();
    let (_, cos_y) = state.rot_y.sin_cos();
    let (sin_y, _) = state.rot_y.sin_cos();

    let rotate_vec = |v: [f64; 3]| -> [f64; 3] {
        let x = v[0]; let y = v[1]; let z = v[2];
        let y1 = y * cos_x - z * sin_x;
        let z1 = y * sin_x + z * cos_x;
        let x2 = x * cos_y - z1 * sin_y;
        let z2 = x * sin_y + z1 * cos_y;
        [x2, y1, z2]
    };

    let axes_data = [
        ([1.0, 0.0, 0.0], (0.9, 0.2, 0.2), state.show_axis_x),
        ([0.0, 1.0, 0.0], (0.2, 0.7, 0.2), state.show_axis_y),
        ([0.0, 0.0, 1.0], (0.2, 0.4, 0.9), state.show_axis_z),
    ];

    let mut sorted_axes: Vec<_> = axes_data.iter().map(|(v, c, show)| {
        (rotate_vec(*v), c, show)
    }).collect();

    sorted_axes.sort_by(|(a,_,_), (b,_,_)| a[2].partial_cmp(&b[2]).unwrap());

    cr.set_line_width(2.0);
    cr.set_line_cap(cairo::LineCap::Round);

    for (r, color, show) in sorted_axes {
        if *show {
            cr.set_source_rgb(color.0, color.1, color.2);
            cr.move_to(hud_cx, hud_cy);
            cr.line_to(hud_cx + r[0] * hud_size, hud_cy + r[1] * hud_size);
            cr.stroke().unwrap();
        }
    }
}
