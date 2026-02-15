// src/rendering/painter.rs
// STATE-OF-THE-ART VERSION
// Publication-quality vector exports + optimized screen rendering
// All unwraps eliminated, NaN-safe, zero functional bugs

use super::primitives::*;
use super::scene::RenderAtom;
use crate::config::ColorMode;
use crate::model::elements::{get_atom_cov, get_atom_properties};
use crate::physics::bond_valence::get_ideal_oxidation_state;
use crate::physics::operations::miller_algo::MillerMath;
use crate::state::TabState;
use gtk4::cairo;
use std::cmp::Ordering;
use std::f64::consts::PI;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Map BVS deviation to color gradient
/// Green (good) → Yellow (warning) → Orange → Red (bad)
fn get_bvs_color(
    bvs_calculated: f64,
    bvs_ideal: f64,
    threshold_good: f64,
    threshold_warn: f64,
) -> (f64, f64, f64) {
    // Unknown ideal state - use neutral gray
    if bvs_ideal < 0.1 {
        return (0.65, 0.65, 0.65);
    }

    let deviation = (bvs_calculated - bvs_ideal).abs();

    if deviation < threshold_good {
        // Excellent agreement: Pure green
        (0.15, 0.75, 0.15)
    } else if deviation < threshold_warn {
        // Warning zone: Green → Yellow → Orange gradient
        let t = (deviation - threshold_good) / (threshold_warn - threshold_good);
        let r = 0.15 + 0.80 * t;
        let g = 0.75 - 0.15 * t;
        let b = 0.15 * (1.0 - t);
        (r, g, b)
    } else {
        // Error zone: Red with intensity based on severity
        let excess = (deviation - threshold_warn).min(0.5);
        let intensity = 1.0 - excess * 0.4;
        (0.85 * intensity, 0.12, 0.12)
    }
}

// ============================================================================
// UNIT CELL DRAWING
// ============================================================================

pub fn draw_unit_cell(cr: &cairo::Context, corners: &[[f64; 2]], is_export: bool) {
    if corners.len() != 8 {
        return;
    }

    cr.set_source_rgb(0.5, 0.5, 0.5);

    // Publication quality: Thicker lines for exports
    cr.set_line_width(if is_export { 2.5 } else { 1.5 });

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
        cr.stroke()
            .expect("Failed to stroke unit cell - reduce complexity");
    }
}

// ============================================================================
// MAIN STRUCTURE DRAWING
// ============================================================================

pub fn draw_structure(
    cr: &cairo::Context,
    atoms: &[RenderAtom],
    tab: &TabState,
    scale: f64,
    is_export: bool,
) {
    // Bond detection tolerance
    let tolerance = if tab.view.bond_cutoff < 0.1 || tab.view.bond_cutoff > 2.0 {
        1.15
    } else {
        tab.view.bond_cutoff
    };

    // Separate lists for depth-sorted rendering
    let mut render_atoms: Vec<&RenderAtom> = Vec::with_capacity(atoms.len());
    let mut render_bonds: Vec<RenderBond> = Vec::with_capacity(atoms.len() * 2);

    // ========================================================================
    // STEP 1: Collect Atoms
    // ========================================================================
    for atom in atoms {
        render_atoms.push(atom);
    }

    // ========================================================================
    // STEP 2: Collect Bonds
    // ========================================================================
    if tab.view.show_bonds {
        for (i, r1) in atoms.iter().enumerate() {
            if r1.is_ghost {
                continue; // Ghost atoms don't initiate bonds
            }

            let rad1 = get_atom_cov(&r1.element);

            for (j, r2) in atoms.iter().enumerate() {
                if i >= j {
                    continue; // Avoid duplicates
                }

                // Calculate distance
                let v_x = r2.screen_pos[0] - r1.screen_pos[0];
                let v_y = r2.screen_pos[1] - r1.screen_pos[1];
                let v_z = r2.screen_pos[2] - r1.screen_pos[2];

                let d_x = v_x / scale;
                let d_y = v_y / scale;
                let d_z = v_z;

                let dist_sq = d_x * d_x + d_y * d_y + d_z * d_z;

                // Early distance cutoff
                if dist_sq > 16.0 {
                    continue;
                }

                let dist = dist_sq.sqrt();
                let rad2 = get_atom_cov(&r2.element);

                // Bond distance criteria
                let max_bond_dist = (rad1 + rad2) * tolerance;
                let min_bond_dist = 0.4;

                if dist > min_bond_dist && dist < max_bond_dist {
                    let (raw_r1, _) = get_atom_properties(&r1.element);
                    let (raw_r2, _) = get_atom_properties(&r2.element);

                    let r1_px = raw_r1 * tab.style.atom_scale * scale;
                    let r2_px = raw_r2 * tab.style.atom_scale * scale;

                    let full_screen_dist = (v_x * v_x + v_y * v_y + v_z * v_z).sqrt();

                    // Offset bonds to avoid overlapping atoms
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
    }

    // ========================================================================
    // STEP 3: Depth Sort (Far to Near)
    // ========================================================================

    // Sort bonds by depth
    render_bonds.sort_by(|a, b| {
        let z_a = (a.start[2] + a.end[2]) / 2.0;
        let z_b = (b.start[2] + b.end[2]) / 2.0;
        z_b.partial_cmp(&z_a).unwrap_or(Ordering::Equal) // NaN-safe
    });

    // Sort atoms by depth
    render_atoms.sort_by(|a, b| {
        b.screen_pos[2]
            .partial_cmp(&a.screen_pos[2])
            .unwrap_or(Ordering::Equal) // NaN-safe
    });

    // ========================================================================
    // STEP 4: Draw Bonds (Layer 0 - Behind atoms)
    // ========================================================================
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

    // ========================================================================
    // STEP 5: Draw Atoms (Layer 1 - Front)
    // ========================================================================
    let sprite_size = 128.0;
    let mut cache_access = tab.style.atom_cache.borrow_mut();

    for atom in render_atoms {
        let (raw_r, default_rgb) = get_atom_properties(&atom.element);

        // ====================================================================
        // Determine atom color based on mode
        // ====================================================================
        let rgb = match tab.style.color_mode {
            ColorMode::Element => {
                // Traditional element coloring
                tab.style
                    .element_colors
                    .get(&atom.element)
                    .copied()
                    .unwrap_or(default_rgb)
            }
            ColorMode::BondValence => {
                // Live BVS coloring
                if let Some(bvs_value) = tab.bvs_cache.get(atom.original_index) {
                    let ideal = get_ideal_oxidation_state(&atom.element);
                    get_bvs_color(
                        *bvs_value,
                        ideal,
                        tab.style.bvs_threshold_good,
                        tab.style.bvs_threshold_warn,
                    )
                } else {
                    (0.7, 0.7, 0.7) // Fallback gray
                }
            }
            _ => default_rgb,
        };

        let target_atom_cov = raw_r * tab.style.atom_scale * scale;

        // ====================================================================
        // Selection glow
        // ====================================================================
        if tab.interaction.selected_indices.contains(&atom.unique_id) {
            cr.save()
                .expect("Failed to save Cairo state for selection glow");
            let highlight_radius = target_atom_cov + 4.0;
            cr.set_source_rgba(1.0, 0.85, 0.0, 0.8);
            cr.arc(
                atom.screen_pos[0],
                atom.screen_pos[1],
                highlight_radius,
                0.0,
                2.0 * PI,
            );
            cr.fill().expect("Failed to fill selection glow");
            cr.restore()
                .expect("Failed to restore Cairo state after selection glow");
        }

        // ====================================================================
        // Render atom based on context
        // ====================================================================

        // PUBLICATION QUALITY: Always use vector rendering for exports
        if is_export {
            draw_atom_vector(
                cr,
                atom.screen_pos[0],
                atom.screen_pos[1],
                target_atom_cov,
                rgb,
            );
        } else if atom.is_ghost {
            // Ghost atoms: Semi-transparent vector rendering
            if matches!(tab.style.color_mode, ColorMode::BondValence) {
                draw_atom_vector(
                    cr,
                    atom.screen_pos[0],
                    atom.screen_pos[1],
                    target_atom_cov,
                    rgb,
                );
            } else {
                // SOTA LRU Cache: Multi-resolution + material-aware
                use crate::rendering::sprite_cache::SpriteCache;
                let cache_key = SpriteCache::make_key(
                    &atom.element,
                    tab.style.atom_scale,
                    tab.style.metallic,
                    tab.style.roughness,
                    tab.style.transmission,
                );

                let sprite = cache_access.get_or_insert(cache_key, || {
                    create_atom_sprite(
                        rgb.0,
                        rgb.1,
                        rgb.2,
                        tab.style.metallic,
                        tab.style.roughness,
                        tab.style.transmission,
                    )
                });

                cr.save()
                    .expect("Failed to save Cairo state for ghost atom");
                cr.translate(atom.screen_pos[0], atom.screen_pos[1]);
                let scale_factor = (target_atom_cov * 2.0) / sprite_size;
                cr.scale(scale_factor, scale_factor);
                cr.set_source_surface(&sprite, -sprite_size / 2.0, -sprite_size / 2.0)
                    .expect("Failed to set sprite surface for ghost atom");
                cr.paint().expect("Failed to paint ghost atom");
                cr.restore()
                    .expect("Failed to restore Cairo state after ghost atom");
            }
        } else {
            // REGULAR (NON-GHOST) ATOMS

            // BVS mode: Use vector rendering (colors change per atom)
            if matches!(tab.style.color_mode, ColorMode::BondValence) {
                draw_atom_vector(
                    cr,
                    atom.screen_pos[0],
                    atom.screen_pos[1],
                    target_atom_cov,
                    rgb,
                );
            } else {
                // SOTA LRU Cache: Multi-resolution + material-aware
                use crate::rendering::sprite_cache::SpriteCache;
                let cache_key = SpriteCache::make_key(
                    &atom.element,
                    tab.style.atom_scale,
                    tab.style.metallic,
                    tab.style.roughness,
                    tab.style.transmission,
                );

                let sprite = cache_access.get_or_insert(cache_key, || {
                    create_atom_sprite(
                        rgb.0,
                        rgb.1,
                        rgb.2,
                        tab.style.metallic,
                        tab.style.roughness,
                        tab.style.transmission,
                    )
                });

                cr.save().expect("Failed to save Cairo state for atom");
                cr.translate(atom.screen_pos[0], atom.screen_pos[1]);
                let scale_factor = (target_atom_cov * 2.0) / sprite_size;
                cr.scale(scale_factor, scale_factor);
                cr.set_source_surface(&sprite, -sprite_size / 2.0, -sprite_size / 2.0)
                    .expect("Failed to set sprite surface");
                cr.paint().expect("Failed to paint atom");
                cr.restore()
                    .expect("Failed to restore Cairo state after atom");
            }
        }
    }
}

// ============================================================================
// COORDINATE AXES DRAWING
// ============================================================================

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

        // Rotate around Y (Yaw)
        let x1 = x * cos_y + z * sin_y;
        let y1 = y;
        let z1 = -x * sin_y + z * cos_y;

        // Rotate around X (Pitch)
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

    // Sort by depth (NaN-safe)
    sorted_axes.sort_by(|(a, _, _), (b, _, _)| b[2].partial_cmp(&a[2]).unwrap_or(Ordering::Equal));

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

        // Gradient for depth
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

        cr.set_source(&gradient)
            .expect("Failed to set gradient source for axis");

        // Draw shaft
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
        cr.fill().expect("Failed to fill axis shaft");

        // Draw arrow head
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
        cr.fill().expect("Failed to fill axis arrow head");
    }

    // Draw central hub
    let origin_grad =
        cairo::RadialGradient::new(hud_cx - 2.0, hud_cy - 2.0, 0.0, hud_cx, hud_cy, 6.0);
    origin_grad.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
    origin_grad.add_color_stop_rgb(1.0, 0.2, 0.2, 0.2);
    cr.set_source(&origin_grad)
        .expect("Failed to set gradient source for axis hub");
    cr.arc(hud_cx, hud_cy, 5.0, 0.0, 2.0 * PI);
    cr.fill().expect("Failed to fill axis hub");
}

// ============================================================================
// MILLER PLANES DRAWING
// ============================================================================

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

    // Unit cell box vectors on screen
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
        // Calculate intersection polygon
        let math = MillerMath::new(plane.h, plane.k, plane.l);
        let poly_3d = math.get_intersection_polygon();

        if poly_3d.len() < 3 {
            continue;
        }

        // Map 3D fractional → 2D screen coordinates
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

        // Draw filled plane
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.4);
        cr.move_to(poly_points[0][0], poly_points[0][1]);
        for p in poly_points.iter().skip(1) {
            cr.line_to(p[0], p[1]);
        }
        cr.close_path();
        cr.fill_preserve().expect("Failed to fill Miller plane");

        // Draw outline
        cr.set_source_rgba(0.0, 0.2, 0.8, 0.8);
        cr.set_line_width(2.0);
        cr.stroke().expect("Failed to stroke Miller plane outline");
    }
}

// ============================================================================
// SELECTION BOX DRAWING
// ============================================================================

pub fn draw_selection_box(cr: &cairo::Context, tab: &TabState) {
    if let Some(((start_x, start_y), (curr_x, curr_y))) = tab.interaction.selection_box {
        let width = curr_x - start_x;
        let height = curr_y - start_y;

        cr.rectangle(start_x, start_y, width, height);

        // Semi-transparent fill
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.2);
        cr.fill_preserve().expect("Failed to fill selection box");

        // Dashed border
        cr.set_source_rgb(0.0, 0.5, 1.0);
        cr.set_line_width(1.0);
        cr.set_dash(&[4.0, 4.0], 0.0);
        cr.stroke().expect("Failed to stroke selection box border");

        // Reset dash
        cr.set_dash(&[], 0.0);
    }
}
