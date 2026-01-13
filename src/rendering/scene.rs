// src/rendering/scene.rs

use crate::config::RotationCenter;
use crate::state::AppState;

// This struct is used by interactions.rs for hit-testing
// and by painter.rs for drawing.
pub struct RenderAtom {
  pub screen_pos: [f64; 3], // x, y, z (depth)
  pub element: String,
  pub original_index: usize, // Used for selection
  pub is_ghost: bool,
}

pub struct SceneBounds {
  pub scale: f64,
  pub width: f64,
  pub height: f64,
}

// Return: (Atoms, Lattice Corners [Screen X, Y], Bounds)
pub fn calculate_scene(
  state: &AppState,
  win_w: f64,
  win_h: f64,
  is_export: bool,
  manual_scale: Option<f64>,
  _forced_center: Option<(f64, f64)>,
) -> (Vec<RenderAtom>, Vec<[f64; 2]>, SceneBounds) {
  let structure = match &state.structure {
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

  // 1. Setup Rotation (Degrees -> Radians)
  let (sin_x, cos_x) = state.view.rot_x.to_radians().sin_cos();
  let (sin_y, cos_y) = state.view.rot_y.to_radians().sin_cos();
  let (sin_z, cos_z) = state.view.rot_z.to_radians().sin_cos();

  let center = get_rotation_center(state);
  let lattice = structure.lattice;
  let inv_lattice = invert_matrix(lattice);

  // Rotation Closure: X -> Y -> Z
  let rotate = |p: [f64; 3]| -> [f64; 3] {
    // Shift to center
    let x = p[0] - center[0];
    let y = p[1] - center[1];
    let z = p[2] - center[2];

    // Rotate around X
    let y1 = y * cos_x - z * sin_x;
    let z1 = y * sin_x + z * cos_x;

    // Rotate around Y
    let x2 = x * cos_y - z1 * sin_y;
    let z2 = x * sin_y + z1 * cos_y;

    // Rotate around Z
    let x3 = x2 * cos_z - y1 * sin_z;
    let y3 = x2 * sin_z + y1 * cos_z;

    [x3, y3, z2]
  };

  let mut render_atoms = Vec::new();
  let mut min_x = f64::MAX;
  let mut max_x = f64::MIN;
  let mut min_y = f64::MAX;
  let mut max_y = f64::MIN;

  // --- 2. Calculate Lattice Corners ---
  let mut raw_corners = Vec::new();
  for x in 0..=1 {
    for y in 0..=1 {
      for z in 0..=1 {
        let fx = x as f64;
        let fy = y as f64;
        let fz = z as f64;
        let cx = fx * lattice[0][0] + fy * lattice[1][0] + fz * lattice[2][0];
        let cy = fx * lattice[0][1] + fy * lattice[1][1] + fz * lattice[2][1];
        let cz = fx * lattice[0][2] + fy * lattice[1][2] + fz * lattice[2][2];
        raw_corners.push([cx, cy, cz]);
      }
    }
  }

  let mut rotated_corners = Vec::new();
  for &p in &raw_corners {
    let r = rotate(p);
    rotated_corners.push(r);
    if r[0] < min_x {
      min_x = r[0];
    }
    if r[0] > max_x {
      max_x = r[0];
    }
    if r[1] < min_y {
      min_y = r[1];
    }
    if r[1] > max_y {
      max_y = r[1];
    }
  }

  // --- 3. Generate Atoms (Real + Ghosts) ---
  for (i, atom) in structure.atoms.iter().enumerate() {
    // Logic to handle atoms near boundaries (ghosts) if lattice inversion exists
    let positions = if let Some(inv) = inv_lattice {
      let p = atom.position;
      // Fractional coords
      let fx = p[0] * inv[0][0] + p[1] * inv[1][0] + p[2] * inv[2][0];
      let fy = p[0] * inv[0][1] + p[1] * inv[1][1] + p[2] * inv[2][1];
      let fz = p[0] * inv[0][2] + p[1] * inv[1][2] + p[2] * inv[2][2];

      // Simple boundary check for ghost generation
      let dx_list = if fx.abs() < 0.05 {
        vec![0.0, 1.0]
      } else {
        vec![fx]
      };
      let dy_list = if fy.abs() < 0.05 {
        vec![0.0, 1.0]
      } else {
        vec![fy]
      };
      let dz_list = if fz.abs() < 0.05 {
        vec![0.0, 1.0]
      } else {
        vec![fz]
      };

      let mut clones = Vec::new();
      for &dx in &dx_list {
        for &dy in &dy_list {
          for &dz in &dz_list {
            let cx = dx * lattice[0][0] + dy * lattice[1][0] + dz * lattice[2][0];
            let cy = dx * lattice[0][1] + dy * lattice[1][1] + dz * lattice[2][1];
            let cz = dx * lattice[0][2] + dy * lattice[1][2] + dz * lattice[2][2];
            let is_ghost =
              (dx - fx).abs() > 0.01 || (dy - fy).abs() > 0.01 || (dz - fz).abs() > 0.01;
            clones.push(([cx, cy, cz], is_ghost));
          }
        }
      }
      clones
    } else {
      vec![(atom.position, false)]
    };

    for (pos, ghost) in positions {
      let r_pos = rotate(pos);
      if r_pos[0] < min_x {
        min_x = r_pos[0];
      }
      if r_pos[0] > max_x {
        max_x = r_pos[0];
      }
      if r_pos[1] < min_y {
        min_y = r_pos[1];
      }
      if r_pos[1] > max_y {
        max_y = r_pos[1];
      }

      render_atoms.push(RenderAtom {
        screen_pos: r_pos, // This is rotated, but NOT yet scaled to pixels
        element: atom.element.clone(),
        original_index: i,
        is_ghost: ghost,
      });
    }
  }

  // --- 4. Calculate Scaling to Pixels ---
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
    final_scale = scale_x.min(scale_y) * state.view.zoom;
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

  // --- 5. Apply Screen Transform (World -> Pixel) ---
  for atom in &mut render_atoms {
    atom.screen_pos[0] = (atom.screen_pos[0] - box_cx) * final_scale + win_cx;
    atom.screen_pos[1] = (atom.screen_pos[1] - box_cy) * final_scale + win_cy;
    // z remains "depth" for sorting, but we could scale it if needed
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

  // Sort by Depth (Z) for Painter's Algorithm
  render_atoms.sort_by(|a, b| a.screen_pos[2].partial_cmp(&b.screen_pos[2]).unwrap());

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

fn get_rotation_center(state: &AppState) -> [f64; 3] {
  if let Some(s) = &state.structure {
    // Access via state.config based on our refactor
    if matches!(state.config.rotation_mode, RotationCenter::UnitCell) {
      let v = s.lattice;
      return [
        (v[0][0] + v[1][0] + v[2][0]) * 0.5,
        (v[0][1] + v[1][1] + v[2][1]) * 0.5,
        (v[0][2] + v[1][2] + v[2][2]) * 0.5,
      ];
    }
    let mut sum = [0.0; 3];
    for a in &s.atoms {
      sum[0] += a.position[0];
      sum[1] += a.position[1];
      sum[2] += a.position[2];
    }
    let n = s.atoms.len() as f64;
    if n > 0.0 {
      return [sum[0] / n, sum[1] / n, sum[2] / n];
    }
  }
  [0.0; 3]
}

fn invert_matrix(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
  let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
    - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
    + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
  if det.abs() < 1e-6 {
    return None;
  }
  let inv = 1.0 / det;
  Some([
    [
      (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
      (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
      (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
    ],
    [
      (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
      (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
      (m[1][0] * m[0][2] - m[0][0] * m[1][2]) * inv,
    ],
    [
      (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
      (m[2][0] * m[0][1] - m[0][0] * m[2][1]) * inv,
      (m[0][0] * m[1][1] - m[1][0] * m[0][1]) * inv,
    ],
  ])
}
