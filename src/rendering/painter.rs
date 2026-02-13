// src/rendering/painter.rs

use super::primitives::*;
use super::scene::RenderAtom;
use crate::model::elements::{get_atom_cov, get_atom_properties};
use crate::physics::operations::miller_algo::MillerMath;
use crate::state::TabState;
use gtk4::cairo;
use std::cmp::Ordering;
use std::f64::consts::PI;

// --- 1. Unit Cell Drawing ---
pub fn draw_unit_cell(cr: &cairo::Context, corners: &[[f64; 2]], is_export: bool) {
    if corners.len() != 8 {
        return;
    }
    cr.set_source_rgb(0.5, 0.5, 0.5);
    cr.set_line_width(if is_export { 2.0 } else { 1.5 });

    let edges = [
        (0, 1),
        (0, 2),
        (0, 4),
        (1, 3),
        (1, 5),
        (2, 3),
        (2, 6),
        (4, 5),
        (4, 6),
        (7, 6),
        (7, 5),
        (7, 3),
    ];

    for (start, end) in edges {
        let p1 = corners[start];
        let p2 = corners[end];
        cr.move_to(p1[0], p1[1]);
        cr.line_to(p2[0], p2[1]);
        cr.stroke().unwrap();
    }
}

// --- 2. Main Structure Drawing ---
pub fn draw_structure(
    cr: &cairo::Context,
    atoms: &[RenderAtom],
    tab: &TabState,
    scale: f64,
    is_export: bool, // UPDATED: Renamed from _is_export to use the flag
) {
    // Access volatile view settings from the tab
    let tolerance = if tab.view.bond_cutoff < 0.1 || tab.view.bond_cutoff > 2.0 {
        1.15
    } else {
        tab.view.bond_cutoff
    };

    // Separate lists for strict layering
    let mut render_atoms: Vec<&RenderAtom> = Vec::with_capacity(atoms.len());
    let mut render_bonds: Vec<RenderBond> = Vec::with_capacity(atoms.len() * 2);

    // 1. Collect Atoms
    for atom in atoms {
        render_atoms.push(atom);
    }

    // 2. Collect Bonds
    for (i, r1) in atoms.iter().enumerate() {
        if r1.is_ghost {
            continue;
        }

        let rad1 = get_atom_cov(&r1.element);

        for (j, r2) in atoms.iter().enumerate() {
            if i >= j {
                continue;
            }

            let v_x = r2.screen_pos[0] - r1.screen_pos[0];
            let v_y = r2.screen_pos[1] - r1.screen_pos[1];
            let v_z = r2.screen_pos[2] - r1.screen_pos[2];

            let d_x = v_x / scale;
            let d_y = v_y / scale;
            let d_z = v_z;

            let dist_sq = d_x * d_x + d_y * d_y + d_z * d_z;
            if dist_sq > 16.0 {
                continue;
            }

            let dist = dist_sq.sqrt();
            let rad2 = get_atom_cov(&r2.element);

            let max_bond_dist = (rad1 + rad2) * tolerance;
            let min_bond_dist = 0.4;

            if dist > min_bond_dist && dist < max_bond_dist {
                let (raw_r1, _) = get_atom_properties(&r1.element);
                let (raw_r2, _) = get_atom_properties(&r2.element);

                // Access style from the TAB (Session settings), not global config
                let r1_px = raw_r1 * tab.style.atom_scale * scale;
                let r2_px = raw_r2 * tab.style.atom_scale * scale;

                let full_screen_dist = (v_x * v_x + v_y * v_y + v_z * v_z).sqrt();

                let off1 = r1_px * 0.95;
                let off2 = r2_px * 0.95;

                if full_screen_dist > (off1 + off2) {
                    let t1 = off1 / full_screen_dist;
                    let t2 = off2 / full_screen_dist;

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

                    render_bonds.push(RenderBond {
                        start,
                        end,
                        radius: tab.style.bond_radius * scale,
                    });
                }
            }
        }
    }

    // 3. Sort Both Lists by Depth (Descending: Far -> Near)
    render_bonds.sort_by(|a, b| {
        let z_a = (a.start[2] + a.end[2]) / 2.0;
        let z_b = (b.start[2] + b.end[2]) / 2.0;
        z_b.partial_cmp(&z_a).unwrap_or(Ordering::Equal)
    });

    render_atoms.sort_by(|a, b| {
        b.screen_pos[2]
            .partial_cmp(&a.screen_pos[2])
            .unwrap_or(Ordering::Equal)
    });

    // 4. DRAW BONDS FIRST (Layer 0)
    for bond in render_bonds {
        draw_cylinder_impostor(
            cr,
            bond.start,
            bond.end,
            bond.radius,
            tab.style.bond_color,
            tab.style.metallic,
            tab.style.roughness,
            tab.style.transmission,
        );
    }

    // 5. DRAW ATOMS SECOND (Layer 1)
    let sprite_size = 128.0;
    let mut cache_access = tab.style.atom_cache.borrow_mut();

    for atom in render_atoms {
        let (raw_r, default_rgb) = get_atom_properties(&atom.element);

        // Use Tab Style
        let rgb = tab
            .style
            .element_colors
            .get(&atom.element)
            .copied()
            .unwrap_or(default_rgb);
        let target_atom_cov = raw_r * tab.style.atom_scale * scale;

        // Selection Glow
        if tab.interaction.selected_indices.contains(&atom.unique_id)
        // Changed to unique_id
        {
            cr.save().unwrap();
            let highlight_radius = target_atom_cov + 4.0;
            cr.set_source_rgba(1.0, 0.85, 0.0, 0.8);
            cr.arc(
                atom.screen_pos[0],
                atom.screen_pos[1],
                highlight_radius,
                0.0,
                2.0 * PI,
            );
            cr.fill().unwrap();
            cr.restore().unwrap();
        }

        // --- RENDER STRATEGY SWITCH ---
        if is_export {
            // PURE VECTOR MODE (Publication Quality)
            // Uses mathematical gradients instead of sprites
            draw_atom_vector(
                cr,
                atom.screen_pos[0],
                atom.screen_pos[1],
                target_atom_cov,
                rgb,
            );
        } else {
            // SPRITE MODE (High Performance)
            // Uses cached bitmaps for 60 FPS rotation
            if !cache_access.contains_key(&atom.element) {
                let sprite = create_atom_sprite(
                    rgb.0,
                    rgb.1,
                    rgb.2,
                    tab.style.metallic,
                    tab.style.roughness,
                    tab.style.transmission,
                );
                cache_access.insert(atom.element.clone(), sprite);
            }
            let sprite = cache_access.get(&atom.element).unwrap();

            cr.save().unwrap();
            cr.translate(atom.screen_pos[0], atom.screen_pos[1]);
            let scale_factor = (target_atom_cov * 2.0) / sprite_size;
            cr.scale(scale_factor, scale_factor);
            cr.set_source_surface(sprite, -sprite_size / 2.0, -sprite_size / 2.0)
                .unwrap();
            cr.paint().unwrap();
            cr.restore().unwrap();
        }
    }
}

pub fn draw_axes(cr: &cairo::Context, tab: &TabState, width: f64, height: f64) {
    let hud_size = (width * 0.12).clamp(60.0, 150.0);
    let hud_cx = hud_size * 0.6;
    let hud_cy = height - hud_size * 0.6;

    let (sin_x, cos_x) = tab.view.rot_x.to_radians().sin_cos();
    let (sin_y, cos_y) = tab.view.rot_y.to_radians().sin_cos();

    let rotate_vec = |v: [f64; 3]| -> [f64; 3] {
        let x = v[0];
        let y = v[1];
        let z = v[2];

        // 1. Rotate around Y (Yaw)
        let x1 = x * cos_y + z * sin_y;
        let y1 = y;
        let z1 = -x * sin_y + z * cos_y;

        // 2. Rotate around X (Pitch)
        let x2 = x1;
        let y2 = y1 * cos_x - z1 * sin_x;
        let z2 = y1 * sin_x + z1 * cos_x;

        [x2, y2, z2]
    };

    let axes_data = [
        ([1.0, 0.0, 0.0], (0.85, 0.2, 0.2), tab.view.show_axes[0]), // X Red
        ([0.0, 1.0, 0.0], (0.2, 0.7, 0.2), tab.view.show_axes[1]),  // Y Green
        ([0.0, 0.0, 1.0], (0.2, 0.4, 0.85), tab.view.show_axes[2]), // Z Blue
    ];

    let mut sorted_axes: Vec<_> = axes_data
        .iter()
        .map(|(v, c, show)| (rotate_vec(*v), c, show))
        .collect();

    // Fix Axes sorting too: Descending Z
    sorted_axes.sort_by(|(a, _, _), (b, _, _)| b[2].partial_cmp(&a[2]).unwrap());

    let shaft_radius = 2.5;
    let head_radius = 6.0;
    let head_length = 16.0;
    let axis_length = hud_size;

    for (r, color, show) in sorted_axes {
        if !*show {
            continue;
        }

        let dx = r[0] * axis_length;
        let dy = -r[1] * axis_length;

        let len_sq = dx * dx + dy * dy;
        if len_sq < 1.0 {
            continue;
        }
        let len = len_sq.sqrt();

        let nx = -dy / len;
        let ny = dx / len;

        let start_x = hud_cx;
        let start_y = hud_cy;
        let end_x = hud_cx + dx;
        let end_y = hud_cy + dy;
        let shaft_end_x = end_x - (dx / len) * head_length;
        let shaft_end_y = end_y - (dy / len) * head_length;

        // Gradient
        let grad_start_x = start_x - nx * head_radius;
        let grad_start_y = start_y - ny * head_radius;
        let grad_end_x = start_x + nx * head_radius;
        let grad_end_y = start_y + ny * head_radius;

        let gradient =
            cairo::LinearGradient::new(grad_start_x, grad_start_y, grad_end_x, grad_end_y);
        let (cr_r, cr_g, cr_b) = *color;

        gradient.add_color_stop_rgb(0.0, cr_r * 0.4, cr_g * 0.4, cr_b * 0.4);
        gradient.add_color_stop_rgb(0.35, cr_r, cr_g, cr_b);
        gradient.add_color_stop_rgb(0.5, cr_r * 1.3, cr_g * 1.3, cr_b * 1.3);
        gradient.add_color_stop_rgb(0.65, cr_r, cr_g, cr_b);
        gradient.add_color_stop_rgb(1.0, cr_r * 0.3, cr_g * 0.3, cr_b * 0.3);

        cr.set_source(&gradient).unwrap();

        // Shaft
        cr.move_to(start_x - nx * shaft_radius, start_y - ny * shaft_radius);
        cr.line_to(
            shaft_end_x - nx * shaft_radius,
            shaft_end_y - ny * shaft_radius,
        );
        cr.line_to(
            shaft_end_x + nx * shaft_radius,
            shaft_end_y + ny * shaft_radius,
        );
        cr.line_to(start_x + nx * shaft_radius, start_y + ny * shaft_radius);
        cr.close_path();
        cr.fill().unwrap();

        // Arrow Head
        cr.move_to(end_x, end_y);
        cr.line_to(
            shaft_end_x + nx * head_radius,
            shaft_end_y + ny * head_radius,
        );
        cr.line_to(
            shaft_end_x - nx * head_radius,
            shaft_end_y - ny * head_radius,
        );
        cr.close_path();
        cr.fill().unwrap();
    }

    // Hub
    let origin_grad =
        cairo::RadialGradient::new(hud_cx - 2.0, hud_cy - 2.0, 0.0, hud_cx, hud_cy, 6.0);
    origin_grad.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    origin_grad.add_color_stop_rgb(1.0, 0.2, 0.2, 0.2);
    cr.set_source(&origin_grad).unwrap();
    cr.arc(hud_cx, hud_cy, 5.0, 0.0, 2.0 * std::f64::consts::PI);
    cr.fill().unwrap();
}

// --- 4. DIAMOND STANDARD: Miller Planes ---
pub fn draw_miller_planes(
    cr: &cairo::Context,
    tab: &TabState,
    lattice_corners: &[[f64; 2]],
    _scale: f64,
    _width: f64,
    _height: f64,
) {
    if lattice_corners.len() < 5 {
        return;
    }

    // Vectors defining the Unit Cell Box on Screen
    let p_origin = lattice_corners[0];
    let p_x_vec = [
        lattice_corners[4][0] - p_origin[0],
        lattice_corners[4][1] - p_origin[1],
    ];
    let p_y_vec = [
        lattice_corners[2][0] - p_origin[0],
        lattice_corners[2][1] - p_origin[1],
    ];
    let p_z_vec = [
        lattice_corners[1][0] - p_origin[0],
        lattice_corners[1][1] - p_origin[1],
    ];

    for plane in &tab.miller_planes {
        // 1. DELEGATE TO THE BRAIN
        let math = MillerMath::new(plane.h, plane.k, plane.l);

        // This returns fractional coordinates [0..1, 0..1, 0..1]
        // sorted and ready to draw.
        let poly_3d = math.get_intersection_polygon();

        if poly_3d.len() < 3 {
            continue;
        }

        // 2. Map 3D Fractional -> 2D Screen
        let poly_points: Vec<[f64; 2]> = poly_3d
            .iter()
            .map(|p| {
                let u = p[0];
                let v = p[1];
                let w = p[2];

                let sx = p_origin[0] + u * p_x_vec[0] + v * p_y_vec[0] + w * p_z_vec[0];
                let sy = p_origin[1] + u * p_x_vec[1] + v * p_y_vec[1] + w * p_z_vec[1];
                [sx, sy]
            })
            .collect();

        // 3. Draw
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.4); // Blue transparent
        cr.move_to(poly_points[0][0], poly_points[0][1]);
        for p in poly_points.iter().skip(1) {
            cr.line_to(p[0], p[1]);
        }
        cr.close_path();
        cr.fill_preserve().unwrap();

        cr.set_source_rgba(0.0, 0.2, 0.8, 0.8); // Blue Outline
        cr.set_line_width(2.0);
        cr.stroke().unwrap();
    }
}
// --- NEW FUNCTION: Draw Box Selection ---
pub fn draw_selection_box(cr: &cairo::Context, tab: &TabState) {
    if let Some(((start_x, start_y), (curr_x, curr_y))) = tab.interaction.selection_box {
        let width = curr_x - start_x;
        let height = curr_y - start_y;

        // Draw Rectangle
        cr.rectangle(start_x, start_y, width, height);

        // Fill: Semi-transparent blue
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.2);
        cr.fill_preserve().unwrap();

        // Border: Solid Blue with Dashes
        cr.set_source_rgb(0.0, 0.5, 1.0);
        cr.set_line_width(1.0);
        cr.set_dash(&[4.0, 4.0], 0.0); // Create dash pattern
        cr.stroke().unwrap();

        // Reset dash for future drawing operations
        cr.set_dash(&[], 0.0);
    }
}
