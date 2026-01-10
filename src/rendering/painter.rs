// src/rendering/painter.rs

use gtk4::cairo::{self, ImageSurface, Format, Context};
use std::f64::consts::PI;
use std::cmp::Ordering;
use crate::state::AppState;
use crate::model::elements::get_atom_properties;
use super::scene::RenderAtom;
// use crate::physics::miller_math;
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

// --- 2. The "Stamp" Generator ---

/// Generates a high-quality 128x128 image of an atom.
/// We do the heavy gradient math here ONCE per element.
fn create_atom_sprite(
    r: f64, g: f64, b: f64,
    metallic: f64, roughness: f64, transmission: f64
) -> ImageSurface {
    let size = 128; // High res for zooming
    let surface = ImageSurface::create(Format::ARgb32, size, size)
        .expect("Failed to create sprite surface");
    let cr = Context::new(&surface).expect("Failed to create sprite context");

    let center = size as f64 / 2.0;
    let radius = size as f64 / 2.0;

    // --- YOUR EXISTING SHADER LOGIC ADAPTED HERE ---
    let (red, green, blue) = (r, g, b);
    let alpha = 1.0 - transmission;

    let spec_r = 1.0 + (red - 1.0) * metallic;
    let spec_g = 1.0 + (green - 1.0) * metallic;
    let spec_b = 1.0 + (blue - 1.0) * metallic;

    let highlight_size = 0.05 + roughness * 0.35;
    let light_offset = 0.25;

    let pat = cairo::RadialGradient::new(
        center - radius * light_offset, center - radius * light_offset, radius * highlight_size,
        center, center, radius
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
    cr.arc(center, center, radius, 0.0, 2.0 * PI);
    cr.fill().unwrap();

    surface
}

// --- 3. Cylinder Impostor (Bonds are still drawn fast normally) ---

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
    // let len = len_sq.sqrt(); // Unused optimization

    let nx = -dy / len_sq.sqrt();
    let ny = dx / len_sq.sqrt();

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

// --- 4. Main Drawing Functions ---

pub fn draw_unit_cell(cr: &cairo::Context, corners: &[[f64; 2]], is_export: bool) {
    if corners.len() != 8 { return; }
    cr.set_source_rgb(0.5, 0.5, 0.5);
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

    // 2. Collect Bonds (Unchanged)
    for (i, r1) in atoms.iter().enumerate() {
        if r1.is_ghost { continue; }
        for (j, r2) in atoms.iter().enumerate() {
            if i >= j { continue; }

            let v_x = r2.screen_pos[0] - r1.screen_pos[0];
            let v_y = r2.screen_pos[1] - r1.screen_pos[1];
            let v_z = r2.screen_pos[2] - r1.screen_pos[2];

            let d_x = v_x / scale;
            let d_y = v_y / scale;
            let d_z = v_z; // Z is depth, not scaled for cutoff check usually, but consistency matters

            // Note: simple Euclidean distance check
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
                        start, end,
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

    // --- SPRITE SETUP ---
    // Access the cache from AppState
    let sprite_size = 128.0;
    let mut cache_access = state.style.atom_cache.borrow_mut();

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
                let rgb = state.style.element_colors.get(&atom.element).copied().unwrap_or(default_rgb);

                // Calculate size on screen
                let target_radius = raw_r * state.style.atom_scale * scale;

                // --- A. SELECTION GLOW (Vector - Drawn Behind) ---
                if state.selected_indices.contains(&atom.original_index) {
                    cr.save().unwrap();
                    let highlight_radius = target_radius + 4.0;
                    cr.set_source_rgba(1.0, 0.85, 0.0, 0.8);
                    cr.arc(atom.screen_pos[0], atom.screen_pos[1], highlight_radius, 0.0, 2.0 * PI);
                    cr.fill().unwrap();
                    cr.restore().unwrap();
                }

                // --- B. DRAW SPRITE (Bitblt - The Fast Part) ---

                // 1. Get or Create Sprite
                if !cache_access.contains_key(&atom.element) {
                    let sprite = create_atom_sprite(
                        rgb.0, rgb.1, rgb.2,
                        state.style.metallic,
                        state.style.roughness,
                        state.style.transmission
                    );
                    cache_access.insert(atom.element.clone(), sprite);
                }

                let sprite = cache_access.get(&atom.element).unwrap();

                // 2. Transform & Stamp
                cr.save().unwrap();

                // Move to atom position
                cr.translate(atom.screen_pos[0], atom.screen_pos[1]);

                // Calculate scale: We need 128px sprite to equal (target_radius * 2) pixels
                let scale_factor = (target_radius * 2.0) / sprite_size;
                cr.scale(scale_factor, scale_factor);

                // Center the sprite (at 0,0 after translate)
                // Sprite is 128x128, so center is at 64,64 inside the image.
                // We draw it at -64,-64 so the middle lands on 0,0
                cr.set_source_surface(sprite, -sprite_size/2.0, -sprite_size/2.0).unwrap();
                cr.paint().unwrap();

                cr.restore().unwrap();
            }
        }
    }
}

pub fn draw_axes(cr: &cairo::Context, state: &AppState, width: f64, height: f64) {
    // 1. HUD Position
    let hud_size = (width * 0.12).clamp(60.0, 150.0);
    let hud_cx = hud_size * 0.6;
    let hud_cy = height - hud_size * 0.6;

    // 2. Rotation Math (CORRECTED ORDER)
    // Most orbit controls apply Yaw (Y/Azimuth) first, then Pitch (X/Elevation).
    // If we don't match that order, the axes desync.
    let (sin_x, cos_x) = state.rot_x.sin_cos();
    let (sin_y, cos_y) = state.rot_y.sin_cos();

    let rotate_vec = |v: [f64; 3]| -> [f64; 3] {
        let x = v[0]; let y = v[1]; let z = v[2];

        // Step 1: Rotate around Y (Yaw/Azimuth)
        // Standard rotation matrix for Y
        let x1 = x * cos_y + z * sin_y;
        let y1 = y;
        let z1 = -x * sin_y + z * cos_y;

        // Step 2: Rotate around X (Pitch/Elevation)
        // Apply to the result of Step 1
        let x2 = x1;
        let y2 = y1 * cos_x - z1 * sin_x;
        let z2 = y1 * sin_x + z1 * cos_x;

        [x2, y2, z2]
    };

    // 3. Axes Data
    let axes_data = [
        ([1.0, 0.0, 0.0], (0.85, 0.2, 0.2), state.show_axis_x), // Red (X)
        ([0.0, 1.0, 0.0], (0.2, 0.7, 0.2), state.show_axis_y), // Green (Y)
        ([0.0, 0.0, 1.0], (0.2, 0.4, 0.85), state.show_axis_z), // Blue (Z)
    ];

    // 4. Sort by Depth
    let mut sorted_axes: Vec<_> = axes_data.iter().map(|(v, c, show)| {
        (rotate_vec(*v), c, show)
    }).collect();

    sorted_axes.sort_by(|(a,_,_), (b,_,_)| a[2].partial_cmp(&b[2]).unwrap());

    // 5. Drawing Config
    let shaft_radius = 2.5;
    let head_radius = 6.0;
    let head_length = 16.0;
    let axis_length = hud_size;

    for (r, color, show) in sorted_axes {
        if !*show { continue; }

        // --- A. Geometry ---
        // Project to 2D.
        // NOTE: We invert Y (-r[1]) because in Cairo +Y is DOWN, but in 3D +Y is UP.
        let dx = r[0] * axis_length;
        let dy = -r[1] * axis_length;

        let len_sq = dx*dx + dy*dy;
        if len_sq < 1.0 { continue; }
        let len = len_sq.sqrt();

        let nx = -dy / len;
        let ny = dx / len;

        let start_x = hud_cx;
        let start_y = hud_cy;
        let end_x = hud_cx + dx;
        let end_y = hud_cy + dy;
        let shaft_end_x = end_x - (dx / len) * head_length;
        let shaft_end_y = end_y - (dy / len) * head_length;

        // --- B. Gradient ---
        let grad_start_x = start_x - nx * head_radius;
        let grad_start_y = start_y - ny * head_radius;
        let grad_end_x   = start_x + nx * head_radius;
        let grad_end_y   = start_y + ny * head_radius;

        let gradient = cairo::LinearGradient::new(grad_start_x, grad_start_y, grad_end_x, grad_end_y);
        let (cr_r, cr_g, cr_b) = *color;

        gradient.add_color_stop_rgb(0.0, cr_r * 0.4, cr_g * 0.4, cr_b * 0.4);
        gradient.add_color_stop_rgb(0.35, cr_r, cr_g, cr_b);
        gradient.add_color_stop_rgb(0.5,  cr_r * 1.3, cr_g * 1.3, cr_b * 1.3);
        gradient.add_color_stop_rgb(0.65, cr_r, cr_g, cr_b);
        gradient.add_color_stop_rgb(1.0, cr_r * 0.3, cr_g * 0.3, cr_b * 0.3);

        cr.set_source(&gradient).unwrap();

        // --- C. Shaft ---
        cr.move_to(start_x - nx * shaft_radius, start_y - ny * shaft_radius);
        cr.line_to(shaft_end_x - nx * shaft_radius, shaft_end_y - ny * shaft_radius);
        cr.line_to(shaft_end_x + nx * shaft_radius, shaft_end_y + ny * shaft_radius);
        cr.line_to(start_x + nx * shaft_radius, start_y + ny * shaft_radius);
        cr.close_path();
        cr.fill().unwrap();

        // --- D. Arrow Head ---
        cr.move_to(end_x, end_y);
        cr.line_to(shaft_end_x + nx * head_radius, shaft_end_y + ny * head_radius);
        cr.line_to(shaft_end_x - nx * head_radius, shaft_end_y - ny * head_radius);
        cr.close_path();
        cr.fill().unwrap();
    }

    // 7. Central Hub
    let origin_grad = cairo::RadialGradient::new(hud_cx - 2.0, hud_cy - 2.0, 0.0, hud_cx, hud_cy, 6.0);
    origin_grad.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    origin_grad.add_color_stop_rgb(1.0, 0.2, 0.2, 0.2);
    cr.set_source(&origin_grad).unwrap();
    cr.arc(hud_cx, hud_cy, 5.0, 0.0, 2.0 * std::f64::consts::PI);
    cr.fill().unwrap();
}


pub fn draw_miller_planes(
    cr: &cairo::Context,
    state: &AppState,
    lattice_corners: &[[f64; 2]], // <--- FIXED TYPE: Expects 2D points
    _scale: f64,                  // Unused for position, but kept for signature compatibility
    _width: f64,
    _height: f64
) {
    // We need at least the origin and axis tips (indices 0, 1, 2, 4)
    if lattice_corners.len() < 5 { return; }

    // 1. Define Screen-Space Basis Vectors from the 2D Lattice Corners
    // Since lattice_corners are already projected/scaled/centered,
    // we use them directly to "interpolate" the plane position.

    // Indices based on standard binary order (000, 001, 010...):
    // 0=Origin, 4=X-tip, 2=Y-tip, 1=Z-tip
    let p_origin = lattice_corners[0];
    let p_x_vec  = [lattice_corners[4][0] - p_origin[0], lattice_corners[4][1] - p_origin[1]];
    let p_y_vec  = [lattice_corners[2][0] - p_origin[0], lattice_corners[2][1] - p_origin[1]];
    let p_z_vec  = [lattice_corners[1][0] - p_origin[0], lattice_corners[1][1] - p_origin[1]];

    for plane in &state.miller_planes {
        let h = plane.h as f64;
        let k = plane.k as f64;
        let l = plane.l as f64;

        if h == 0. && k == 0. && l == 0. { continue; }

        // Edges of the unit cell in fractional coordinates (0 to 1)
        let edges_frac = [
            ([0.,0.,0.], [1.,0.,0.]), ([0.,0.,0.], [0.,1.,0.]), ([0.,0.,0.], [0.,0.,1.]),
            ([1.,0.,0.], [0.,1.,0.]), ([1.,0.,0.], [0.,0.,1.]),
            ([0.,1.,0.], [1.,0.,0.]), ([0.,1.,0.], [0.,0.,1.]),
            ([0.,0.,1.], [1.,0.,0.]), ([0.,0.,1.], [0.,1.,0.]),
            ([1.,1.,0.], [0.,0.,1.]), ([1.,0.,1.], [0.,1.,0.]), ([0.,1.,1.], [1.,0.,0.]),
        ];

        let mut poly_points: Vec<[f64; 2]> = Vec::new();

        // 2. Find intersections in Fractional space (u,v,w)
        for (start, dir) in edges_frac.iter() {
            let denom = h * dir[0] + k * dir[1] + l * dir[2];
            let numer = 1.0 - (h * start[0] + k * start[1] + l * start[2]);

            if denom.abs() > 1e-6 {
                let t = numer / denom;
                // Allow slight tolerance for floating point errors
                if t >= -0.001 && t <= 1.001 {
                    let u = start[0] + t * dir[0];
                    let v = start[1] + t * dir[1];
                    let w = start[2] + t * dir[2];

                    // 3. Map Fractional Point (u,v,w) directly to Screen Space (sx, sy)
                    // P_screen = Origin + u*AxisX + v*AxisY + w*AxisZ
                    let sx = p_origin[0] + u * p_x_vec[0] + v * p_y_vec[0] + w * p_z_vec[0];
                    let sy = p_origin[1] + u * p_x_vec[1] + v * p_y_vec[1] + w * p_z_vec[1];

                    poly_points.push([sx, sy]);
                }
            }
        }

        // 4. Draw Polygon
        if poly_points.len() >= 3 {
            // Calculate Centroid
            let cen_x: f64 = poly_points.iter().map(|p| p[0]).sum::<f64>() / poly_points.len() as f64;
            let cen_y: f64 = poly_points.iter().map(|p| p[1]).sum::<f64>() / poly_points.len() as f64;

            // Sort clockwise around centroid
            poly_points.sort_by(|a, b| {
                let ang_a = (a[1] - cen_y).atan2(a[0] - cen_x);
                let ang_b = (b[1] - cen_y).atan2(b[0] - cen_x);
                ang_a.partial_cmp(&ang_b).unwrap()
            });

            // Deduplicate close points
            poly_points.dedup_by(|a, b| (a[0]-b[0]).abs() < 1e-4 && (a[1]-b[1]).abs() < 1e-4);

            if poly_points.len() < 3 { continue; }

            cr.set_source_rgba(0.0, 0.5, 1.0, 0.4); // Blue transparent fill
            cr.move_to(poly_points[0][0], poly_points[0][1]);
            for p in poly_points.iter().skip(1) {
                cr.line_to(p[0], p[1]);
            }
            cr.close_path();
            cr.fill_preserve().unwrap();

            cr.set_source_rgba(0.0, 0.2, 0.8, 0.8); // Darker blue border
            cr.set_line_width(2.0);
            cr.stroke().unwrap();
        }
    }
}
