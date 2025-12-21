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
    /// Returns the average Z-depth of the object for sorting.
    /// Larger Z = Closer to camera (in this coordinate system).
    fn z_depth(&self) -> f64 {
        match self {
            RenderPrimitive::Atom(atom) => atom.screen_pos[2],
            RenderPrimitive::Bond(bond) => (bond.start[2] + bond.end[2]) / 2.0,
        }
    }
}

// --- 2. Shaders & Helpers ---

/// Calculates a "fog" factor based on Z-depth.
/// Returns 0.0 (far/foggy) to 1.0 (near/clear).
fn calculate_fog(z: f64, min_z: f64, max_z: f64) -> f64 {
    if (max_z - min_z).abs() < 0.001 { return 1.0; }
    let norm = (z - min_z) / (max_z - min_z);
    // Map to 0.5..1.0 so back items don't disappear completely
    0.5 + (norm * 0.5)
}

fn set_publication_gradient(
    cr: &cairo::Context,
    cx: f64, cy: f64, r: f64,
    base_color: (f64, f64, f64),
    fog: f64,
    style: &crate::state::RenderStyle,
) {
    let (red, green, blue) = base_color;
    let fr = red * fog;
    let fg = green * fog;
    let fb = blue * fog;

    // Advanced "Ceramic" Shader for Publication
    let pat = cairo::RadialGradient::new(
        cx - r * 0.3, cy - r * 0.3, r * style.shine_hardness, // Focus point (Highlight)
        cx, cy, r                                             // Extent
    );

    // Stop 0: Specular Highlight (White)
    pat.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, style.shine_strength * fog);

    // Stop 0.3: True Color (Lit side)
    pat.add_color_stop_rgb(0.3, fr, fg, fb);

    // Stop 0.8: Shadow/Shading
    pat.add_color_stop_rgb(0.8, fr * 0.6, fg * 0.6, fb * 0.6);

    // Stop 1.0: Rim Shadow (Dark edges)
    pat.add_color_stop_rgb(1.0, fr * 0.2, fg * 0.2, fb * 0.2);

    cr.set_source(&pat).unwrap();
}

fn draw_cylinder_impostor(
    cr: &cairo::Context,
    p1: [f64; 3],
    p2: [f64; 3],
    radius: f64,
    is_export: bool,
    fog: f64,
    style: &crate::state::RenderStyle,
) {
    let dx = p2[0] - p1[0];
    let dy = p2[1] - p1[1];
    let len_sq = dx*dx + dy*dy;
    if len_sq < 0.0001 { return; }
    let len = len_sq.sqrt();

    // Normal vector for width
    let nx = -dy / len;
    let ny = dx / len;

    // Corners of the bond rectangle
    let c1x = p1[0] + nx * radius; let c1y = p1[1] + ny * radius;
    let c2x = p2[0] + nx * radius; let c2y = p2[1] + ny * radius;
    let c3x = p2[0] - nx * radius; let c3y = p2[1] - ny * radius;
    let c4x = p1[0] - nx * radius; let c4y = p1[1] - ny * radius;

    // Linear Gradient for "Tube" effect
    let gradient = cairo::LinearGradient::new(c1x, c1y, c4x, c4y);

    let (br, bg, bb) = style.bond_color;

    if is_export {
        // High-Quality Metallic Bond for Export
        let r = br * fog;
        let g = bg * fog;
        let b = bb * fog;

        gradient.add_color_stop_rgb(0.0, r*0.2, g*0.2, b*0.2);      // Edge
        gradient.add_color_stop_rgb(0.4, r, g, b);                  // Body
        gradient.add_color_stop_rgb(0.6, r*1.5, g*1.5, b*1.5);      // Specular Highlight
        gradient.add_color_stop_rgb(0.9, r*0.2, g*0.2, b*0.2);      // Edge
    } else {
        // Screen Mode (Transparency enabled)
        gradient.add_color_stop_rgba(0.0, br*0.4, bg*0.4, bb*0.4, fog);
        gradient.add_color_stop_rgba(0.5, br, bg, bb, fog);
        gradient.add_color_stop_rgba(1.0, br*0.2, bg*0.2, bb*0.2, fog);
    }

    cr.set_source(&gradient).unwrap();
    cr.move_to(c1x, c1y);
    cr.line_to(c2x, c2y);
    cr.line_to(c3x, c3y);
    cr.line_to(c4x, c4y);
    cr.close_path();
    cr.fill().unwrap();

    // Optional: Tiny outlines for export definition
    if is_export {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.5 * fog);
        cr.set_line_width(0.5);
        cr.stroke().unwrap();
    }
}

// --- 3. Main Drawing Functions ---

pub fn draw_unit_cell(cr: &cairo::Context, corners: &[[f64; 2]], is_export: bool) {
    if corners.len() != 8 { return; }

    if is_export {
        cr.set_source_rgb(0.1, 0.1, 0.1); // Solid Black/Dark Grey
        cr.set_line_width(1.5);
    } else {
        cr.set_source_rgba(0.6, 0.6, 0.6, 0.3); // Faint on Screen
        cr.set_line_width(1.0);
    }

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

/// The Main Render Function: Sorts and Draws EVERYTHING
pub fn draw_structure(
    cr: &cairo::Context,
    atoms: &[RenderAtom],
    state: &AppState,
    scale: f64,
    is_export: bool
) {
    let cutoff_sq = state.bond_cutoff * state.bond_cutoff;
    let mut primitives: Vec<RenderPrimitive> = Vec::with_capacity(atoms.len() * 4);

    // 1. Collect Atoms
    for atom in atoms {
        primitives.push(RenderPrimitive::Atom(atom));
    }

    // 2. Collect Bonds (Dynamically generated & Shortened)
    for (i, r1) in atoms.iter().enumerate() {
        if r1.is_ghost { continue; }
        for (j, r2) in atoms.iter().enumerate() {
            if i >= j { continue; }

            let v_x = r2.screen_pos[0] - r1.screen_pos[0];
            let v_y = r2.screen_pos[1] - r1.screen_pos[1];
            let v_z = r2.screen_pos[2] - r1.screen_pos[2];

            // Check distance (using scale-normalized coordinates)
            let d_x = v_x / scale;
            let d_y = v_y / scale;
            let d_z = v_z; // Z is roughly in Angstrom scale already in this projection logic

            if (d_x*d_x + d_y*d_y + d_z*d_z) < cutoff_sq {
                // Determine Radii
                let (raw_r1, _) = get_atom_properties(&r1.element);
                let (raw_r2, _) = get_atom_properties(&r2.element);

                // Screen Radii (must match atom draw scale)
                let r1_px = raw_r1 * state.style.atom_scale * scale;
                let r2_px = raw_r2 * state.style.atom_scale * scale;

                let full_dist = (v_x*v_x + v_y*v_y + v_z*v_z).sqrt();

                // Shorten bonds so they start/end INSIDE the sphere but don't poke the face
                // Shortening by 80% of radius ensures they disappear into the sphere
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

    // 3. Calculate Z-Range for Fog
    let mut min_z = f64::MAX;
    let mut max_z = f64::MIN;
    for p in &primitives {
        let z = p.z_depth();
        if z < min_z { min_z = z; }
        if z > max_z { max_z = z; }
    }

    // 4. SORT by Z-Depth (Painter's Algorithm: Back to Front)
    primitives.sort_by(|a, b| {
        a.z_depth().partial_cmp(&b.z_depth()).unwrap_or(Ordering::Equal)
    });

    // 5. Draw Loop
    for prim in primitives {
        match prim {
            RenderPrimitive::Bond(bond) => {
                let z = (bond.start[2] + bond.end[2]) / 2.0;
                let fog = calculate_fog(z, min_z, max_z);
                draw_cylinder_impostor(cr, bond.start, bond.end, bond.radius, is_export, fog, &state.style);
            },
            RenderPrimitive::Atom(atom) => {
                let fog = calculate_fog(atom.screen_pos[2], min_z, max_z);
                let (raw_r, default_rgb) = get_atom_properties(&atom.element);

                // --- COLOR LOGIC: Check Uniform vs Element ---
                let rgb = if state.style.use_uniform_atom_color {
                    state.style.atom_color
                } else {
                    default_rgb
                };

                let radius = raw_r * state.style.atom_scale * scale;

                set_publication_gradient(cr, atom.screen_pos[0], atom.screen_pos[1], radius, rgb, fog, &state.style);

                cr.arc(atom.screen_pos[0], atom.screen_pos[1], radius, 0.0, 2.0 * PI);

                if is_export {
                    cr.fill_preserve().unwrap();
                    // Crisp outline for publication
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.8 * fog);
                    cr.set_line_width(0.8);
                    cr.stroke().unwrap();
                } else {
                    cr.fill().unwrap();
                }
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

    cr.set_line_width(hud_size * 0.05);
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
