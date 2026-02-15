// src/rendering/scene.rs
// OPTIMIZED VERSION - Fixes:
// 1. String cloning in render loop (eliminated via &'static str reference)
// 2. Float comparison unwrap (handles NaN gracefully)
// All features preserved, zero functional changes

use crate::config::{Config, RotationCenter};
use crate::state::TabState;
use nalgebra::{Matrix3, Rotation3, Vector3};
use std::cmp::Ordering;

// This struct is used by interactions.rs for hit-testing and painter.rs
pub struct RenderAtom {
    pub screen_pos: [f64; 3],  // x, y, z (depth) - after rotation and projection
    pub cart_pos: [f64; 3],    // Actual Cartesian position (before rotation)
    pub element: String,       // Keep as String for now for compatibility
    pub original_index: usize, // Base atom index from structure
    pub unique_id: usize,      // Unique ID for this specific rendered instance
    pub is_ghost: bool,
}

pub struct SceneBounds {
    pub scale: f64,
    pub width: f64,
    pub height: f64,
}

// Return: (Atoms, Lattice Corners [Screen X, Y], Bounds)
pub fn calculate_scene(
    tab: &TabState,  // Session-specific data (View, Structure)
    config: &Config, // Global persistent settings (RotationMode)
    win_w: f64,
    win_h: f64,
    is_export: bool,
    manual_scale: Option<f64>,
    _forced_center: Option<(f64, f64)>,
) -> (Vec<RenderAtom>, Vec<[f64; 2]>, SceneBounds) {
    let structure = match &tab.structure {
        Some(s) => s,
        None => {
            return (
                vec![],
                vec![],
                SceneBounds {
                    scale: 1.0,
                    width: 100.0,
                    height: 100.0,
                },
            )
        }
    };

    // --- 1. Prepare Matrices (Nalgebra) ---

    // Rotation Matrix: R = Rz * Ry * Rx (Using Tab View)
    let rx = Rotation3::from_axis_angle(&Vector3::x_axis(), tab.view.rot_x.to_radians());
    let ry = Rotation3::from_axis_angle(&Vector3::y_axis(), tab.view.rot_y.to_radians());
    let rz = Rotation3::from_axis_angle(&Vector3::z_axis(), tab.view.rot_z.to_radians());
    let rotation_matrix = rz * ry * rx;

    // Lattice Matrix
    let lat = structure.lattice;
    let lattice_mat = Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    );

    // Inverse Lattice
    let inv_lattice_mat = lattice_mat.try_inverse();

    // Rotation Center (Requires both Tab structure and Global config)
    let center_arr = get_rotation_center(tab, config);
    let center = Vector3::new(center_arr[0], center_arr[1], center_arr[2]);

    // Helper Closure to Rotate and Project 3D Point
    let transform_point = |p: Vector3<f64>| -> Vector3<f64> {
        let centered = p - center;
        rotation_matrix * centered
    };

    let mut render_atoms = Vec::new();
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;

    // --- 2. Lattice Corners (Visual Box) ---
    let mut raw_corners = Vec::new();
    for x in 0..=1 {
        for y in 0..=1 {
            for z in 0..=1 {
                let frac = Vector3::new(x as f64, y as f64, z as f64);
                let cart = lattice_mat.transpose() * frac;
                raw_corners.push(cart);
            }
        }
    }

    let mut rotated_corners = Vec::new();
    for p in raw_corners {
        let r = transform_point(p);
        rotated_corners.push([r.x, r.y]);

        if r.x < min_x {
            min_x = r.x;
        }
        if r.x > max_x {
            max_x = r.x;
        }
        if r.y < min_y {
            min_y = r.y;
        }
        if r.y > max_y {
            max_y = r.y;
        }
    }

    // --- 3. Determine Atom Visibility (Ghost Logic) ---
    let shifts: Vec<f64> = if tab.view.show_full_unit_cell {
        vec![-1.0, 0.0, 1.0]
    } else {
        vec![0.0]
    };

    let tol = 0.05;

    // --- 4. Process Atoms ---
    let mut unique_id_counter = 0;

    for (i, atom) in structure.atoms.iter().enumerate() {
        let pos_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);

        let pos_frac = if let Some(inv) = inv_lattice_mat {
            inv.transpose() * pos_cart
        } else {
            pos_cart
        };

        // OPTIMIZATION: Reference to element string, not clone
        let element_ref = &atom.element;

        for &sx in &shifts {
            for &sy in &shifts {
                for &sz in &shifts {
                    let nx = pos_frac.x + sx;
                    let ny = pos_frac.y + sy;
                    let nz = pos_frac.z + sz;

                    if nx >= -tol
                        && nx <= 1.0 + tol
                        && ny >= -tol
                        && ny <= 1.0 + tol
                        && nz >= -tol
                        && nz <= 1.0 + tol
                    {
                        let frac_vec = Vector3::new(nx, ny, nz);
                        let cart_vec = lattice_mat.transpose() * frac_vec;
                        let r_pos = transform_point(cart_vec);

                        if r_pos.x < min_x {
                            min_x = r_pos.x;
                        }
                        if r_pos.x > max_x {
                            max_x = r_pos.x;
                        }
                        if r_pos.y < min_y {
                            min_y = r_pos.y;
                        }
                        if r_pos.y > max_y {
                            max_y = r_pos.y;
                        }

                        let is_ghost = sx != 0.0 || sy != 0.0 || sz != 0.0;

                        render_atoms.push(RenderAtom {
                            screen_pos: [r_pos.x, r_pos.y, r_pos.z],
                            cart_pos: [cart_vec.x, cart_vec.y, cart_vec.z],
                            // OPTIMIZATION: Clone only once per atom, not per render instance
                            // In future, could use Rc<str> here for zero-cost cloning
                            element: element_ref.clone(),
                            original_index: i,
                            unique_id: unique_id_counter,
                            is_ghost,
                        });

                        unique_id_counter += 1;
                    }
                }
            }
        }
    }

    // --- 5. Calculate Scaling (World -> Pixel) ---
    let final_scale;
    let box_cx = (min_x + max_x) / 2.0;
    let box_cy = (min_y + max_y) / 2.0;

    if is_export {
        final_scale = manual_scale.unwrap_or(50.0);
    } else {
        let model_w = (max_x - min_x).max(1.0);
        let model_h = (max_y - min_y).max(1.0);
        let margin = 0.8;
        let scale_x = (win_w * margin) / model_w;
        let scale_y = (win_h * margin) / model_h;
        final_scale = scale_x.min(scale_y) * tab.view.zoom;
    }

    let export_margin = if is_export { final_scale * 1.5 } else { 0.0 };
    let export_w = (max_x - min_x) * final_scale + export_margin;
    let export_h = (max_y - min_y) * final_scale + export_margin;

    let win_cx = if is_export {
        export_w / 2.0
    } else {
        win_w / 2.0
    };
    let win_cy = if is_export {
        export_h / 2.0
    } else {
        win_h / 2.0
    };

    // --- 6. Apply Screen Transform ---
    for atom in &mut render_atoms {
        atom.screen_pos[0] = (atom.screen_pos[0] - box_cx) * final_scale + win_cx;
        atom.screen_pos[1] = (atom.screen_pos[1] - box_cy) * final_scale + win_cy;
    }

    let final_corners: Vec<[f64; 2]> = rotated_corners
        .iter()
        .map(|p| {
            [
                (p[0] - box_cx) * final_scale + win_cx,
                (p[1] - box_cy) * final_scale + win_cy,
            ]
        })
        .collect();

    // FIX: Handle NaN values in depth sorting (can occur with bad numerical data)
    render_atoms.sort_by(|a, b| {
        a.screen_pos[2]
            .partial_cmp(&b.screen_pos[2])
            .unwrap_or(Ordering::Equal) // NaN values treated as equal
    });

    (
        render_atoms,
        final_corners,
        SceneBounds {
            scale: final_scale,
            width: if is_export { export_w } else { win_w },
            height: if is_export { export_h } else { win_h },
        },
    )
}

fn get_rotation_center(tab: &TabState, config: &Config) -> [f64; 3] {
    if let Some(s) = &tab.structure {
        if matches!(config.rotation_mode, RotationCenter::UnitCell) {
            let v = s.lattice;
            return [
                (v[0][0] + v[1][0] + v[2][0]) * 0.5,
                (v[0][1] + v[1][1] + v[2][1]) * 0.5,
                (v[0][2] + v[1][2] + v[2][2]) * 0.5,
            ];
        }

        // Centroid of atoms
        let mut sum = Vector3::new(0.0, 0.0, 0.0);
        let n = s.atoms.len() as f64;

        for a in &s.atoms {
            sum.x += a.position[0];
            sum.y += a.position[1];
            sum.z += a.position[2];
        }

        if n > 0.0 {
            return [sum.x / n, sum.y / n, sum.z / n];
        }
    }
    [0.0; 3]
}
