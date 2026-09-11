// src/rendering/scene.rs
// OPTIMIZED VERSION - Fixes:
// 1. String cloning in render loop (eliminated via &'static str reference)
// 2. Float comparison unwrap (handles NaN gracefully)
// 3. FIX: Always generate ghost atoms for correct polyhedra/bond detection
// All features preserved, zero functional changes

use crate::config::{Config, RotationCenter};
use crate::state::TabState;
use nalgebra::{Matrix3, Vector3};
use std::cmp::Ordering;

// This struct is used by interactions.rs for hit-testing and painter.rs
#[derive(Clone)]
pub struct RenderAtom {
    pub screen_pos: [f64; 3],  // x, y, z (depth) - after rotation and projection
    pub cart_pos: [f64; 3],    // Actual Cartesian position (before rotation)
    pub element: String,       // Keep as String for now for compatibility
    pub original_index: usize, // Base atom index from structure
    pub unique_id: usize,      // Unique ID for this specific rendered instance
    pub is_ghost: bool,
    /// Coordination-only ghost: participates in bond/polyhedra neighbor detection
    /// but is NEVER drawn and NEVER affects the bounding box / zoom.
    pub is_coord_only: bool,
    pub screen_radius: f64, // Rendered radius in pixels - used for accurate hit-testing
}

pub struct SceneBounds {
    pub scale: f64,
    pub width: f64,
    pub height: f64,
    /// Everything needed to put an arbitrary Cartesian point through the same
    /// transform the atoms took. Kept here rather than recomputed by callers
    /// so overlays (interstitial sites, and anything else drawn in crystal
    /// coordinates) cannot drift out of register with the structure.
    rotation: Matrix3<f64>,
    center: Vector3<f64>,
    box_center: [f64; 2],
    win_center: [f64; 2],
}

impl SceneBounds {
    /// Project a Cartesian point to screen space.
    ///
    /// Returns `[x, y, depth]` — depth in the same units as
    /// `RenderAtom::screen_pos[2]`, so an overlay can be depth-tested against
    /// the atoms.
    pub fn project(&self, cart: [f64; 3]) -> [f64; 3] {
        let p = Vector3::new(cart[0], cart[1], cart[2]) - self.center;
        let r = self.rotation * p;
        [
            (r.x - self.box_center[0]) * self.scale + self.win_center[0],
            (r.y - self.box_center[1]) * self.scale + self.win_center[1],
            r.z,
        ]
    }
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
                    rotation: Matrix3::identity(),
                    center: Vector3::zeros(),
                    box_center: [0.0, 0.0],
                    win_center: [0.0, 0.0],
                    width: 100.0,
                    height: 100.0,
                },
            )
        }
    };

    // --- 1. Prepare Matrices (Nalgebra) ---
    let rotation_matrix = tab.view.rotation_matrix();

    let lat = structure.lattice;
    let lattice_mat = Matrix3::new(
        lat[0][0], lat[0][1], lat[0][2], lat[1][0], lat[1][1], lat[1][2], lat[2][0], lat[2][1],
        lat[2][2],
    );

    let inv_lattice_mat = lattice_mat.try_inverse();

    let center_arr = get_rotation_center(tab, config);
    let center = Vector3::new(center_arr[0], center_arr[1], center_arr[2]);

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

    // --- 3. Ghost Atom Generation ---
    // Two-tier ghost system for correct coordination polyhedra:
    //
    //   VISIBLE ghosts  (narrow range [-tol, 1+tol]):
    //     Atoms at cell boundaries, drawn when "Show Full Unit Cell" is on.
    //
    //   COORDINATION ghosts (wider range [-coord_tol, 1+coord_tol]):
    //     Atoms further outside the cell, NEVER drawn and NEVER affect bounding box.
    //     Needed so atoms at corners/edges see their full coordination shell.
    //     Example: Ti at frac(0,0,0) in BaTiO₃ needs O images at frac(-0.5, 0, 0)
    //     to find all 6 neighbors for a correct octahedron.
    //
    // A non-periodic structure (a molecule from XYZ/PDB) has no periodic
    // images to generate: its "cell" is only a display box. Expanding it
    // anyway multiplied the scene by up to 27x — measured at 9-12x on real
    // molecules, e.g. 4705 PDB atoms becoming 46031 entries, 41326 of them
    // coordination ghosts that are never drawn but still cost projection
    // time and inflate the bond-detection grid.
    let periodic = structure.is_periodic;

    let shifts: Vec<f64> = if periodic {
        vec![-1.0, 0.0, 1.0]
    } else {
        vec![0.0]
    };
    let include_ghosts_in_bounds = tab.view.show_full_unit_cell;

    let tol = 0.05; // Visible boundary ghosts
    let coord_tol = 0.55; // Coordination shell ghosts (covers half-cell images)

    // --- 4. Process Atoms ---
    let mut unique_id_counter = 0;

    for (i, atom) in structure.atoms.iter().enumerate() {
        let pos_cart = Vector3::new(atom.position[0], atom.position[1], atom.position[2]);

        let pos_frac = if let Some(inv) = inv_lattice_mat {
            inv.transpose() * pos_cart
        } else {
            pos_cart
        };

        let element_ref = &atom.element;

        for &sx in &shifts {
            for &sy in &shifts {
                for &sz in &shifts {
                    let nx = pos_frac.x + sx;
                    let ny = pos_frac.y + sy;
                    let nz = pos_frac.z + sz;

                    // Check against the WIDER coordination range first.
                    // Only meaningful for periodic cells: it bounds how far
                    // ghost images may stray. Applying it to a molecule would
                    // silently delete atoms that sit outside their display
                    // box, which is exactly what happens to an XYZ molecule
                    // larger than the default 20 A box.
                    if periodic
                        && (nx < -coord_tol
                            || nx > 1.0 + coord_tol
                            || ny < -coord_tol
                            || ny > 1.0 + coord_tol
                            || nz < -coord_tol
                            || nz > 1.0 + coord_tol)
                    {
                        continue;
                    }

                    let frac_vec = Vector3::new(nx, ny, nz);
                    let cart_vec = lattice_mat.transpose() * frac_vec;
                    let r_pos = transform_point(cart_vec);

                    let is_shift = sx != 0.0 || sy != 0.0 || sz != 0.0;

                    // Is this within the narrow visible-ghost range?
                    let in_narrow = nx >= -tol
                        && nx <= 1.0 + tol
                        && ny >= -tol
                        && ny <= 1.0 + tol
                        && nz >= -tol
                        && nz <= 1.0 + tol;

                    let is_ghost = is_shift;
                    // Coordination-only: outside narrow range, inside wide range.
                    // These are NEVER drawn, NEVER affect bounding box.
                    let is_coord_only = is_shift && !in_narrow;

                    // Bounding box: exclude coord-only ghosts entirely
                    if !is_coord_only
                        && (!is_ghost || include_ghosts_in_bounds) {
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
                        }

                    render_atoms.push(RenderAtom {
                        screen_pos: [r_pos.x, r_pos.y, r_pos.z],
                        cart_pos: [cart_vec.x, cart_vec.y, cart_vec.z],
                        element: element_ref.clone(),
                        original_index: i,
                        unique_id: unique_id_counter,
                        is_ghost,
                        is_coord_only,
                        screen_radius: 0.0,
                    });

                    unique_id_counter += 1;
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

        let raw_r = crate::model::elements::get_covalent_radius(&atom.element);
        let mult = tab.override_radius_scale(atom.original_index);
        atom.screen_radius = raw_r * tab.style.atom_scale * mult * final_scale;
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

    render_atoms.sort_by(|a, b| {
        a.screen_pos[2]
            .partial_cmp(&b.screen_pos[2])
            .unwrap_or(Ordering::Equal)
    });

    (
        render_atoms,
        final_corners,
        SceneBounds {
            scale: final_scale,
            rotation: rotation_matrix.into(),
            center,
            box_center: [box_cx, box_cy],
            win_center: [win_cx, win_cy],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::model::{Atom, Structure};
    use crate::state::TabState;

    fn atom(pos: [f64; 3]) -> Atom {
        Atom {
            element: "C".into(),
            position: pos,
            original_index: 0,
            oxidation: None,
            occupancy: 1.0,
        }
    }

    fn tab_with(structure: Structure, config: &Config) -> TabState {
        let mut tab = TabState::new(config);
        tab.structure = Some(structure);
        tab
    }

    fn cube(edge: f64, atoms: Vec<Atom>, is_periodic: bool) -> Structure {
        Structure {
            lattice: [[edge, 0.0, 0.0], [0.0, edge, 0.0], [0.0, 0.0, edge]],
            atoms,
            formula: String::new(),
            is_periodic,
        }
    }

    #[test]
    fn non_periodic_structures_generate_no_ghosts() {
        let cfg = Config::default();
        // Two atoms mid-cell. Under periodic expansion each would spawn up to
        // 27 images (frac 0.5 +/- 1 lands inside the +/-0.55 coordination
        // window), none of which mean anything for a molecule.
        let s = cube(20.0, vec![atom([10.0, 10.0, 10.0]), atom([11.5, 10.0, 10.0])], false);
        let tab = tab_with(s, &cfg);

        let (entries, _, _) = calculate_scene(&tab, &cfg, 800.0, 600.0, false, None, None);
        assert_eq!(entries.len(), 2, "expected one entry per atom, got {}", entries.len());
        assert!(entries.iter().all(|e| !e.is_ghost && !e.is_coord_only));
    }

    #[test]
    fn periodic_structures_still_generate_coordination_ghosts() {
        let cfg = Config::default();
        let s = cube(20.0, vec![atom([10.0, 10.0, 10.0])], true);
        let tab = tab_with(s, &cfg);

        let (entries, _, _) = calculate_scene(&tab, &cfg, 800.0, 600.0, false, None, None);
        assert!(
            entries.len() > 1,
            "periodic cells must keep their ghost images for coordination/polyhedra"
        );
        assert!(entries.iter().any(|e| e.is_ghost));
    }

    #[test]
    fn molecule_larger_than_its_box_is_not_culled() {
        let cfg = Config::default();
        // An XYZ molecule with no Lattice= gets a fixed 20 A box regardless of
        // its real size. Atoms far outside it used to fail the coordination
        // range check and vanish from the scene entirely.
        let s = cube(
            20.0,
            vec![
                atom([-60.0, 0.0, 0.0]),
                atom([10.0, 10.0, 10.0]),
                atom([80.0, 90.0, 100.0]),
            ],
            false,
        );
        let tab = tab_with(s, &cfg);

        let (entries, _, _) = calculate_scene(&tab, &cfg, 800.0, 600.0, false, None, None);
        assert_eq!(entries.len(), 3, "atoms outside the display box must still render");
    }
}
