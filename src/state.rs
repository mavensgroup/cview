// src/state.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap; // Import HashMap

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RotationCenter { Centroid, UnitCell }

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat { Png, Pdf }

#[derive(Clone, Debug, Serialize, Deserialize)] // Removed Copy (HashMap is not Copy)
pub struct RenderStyle {
    pub atom_scale: f64,
    pub bond_radius: f64,
    pub bond_color: (f64, f64, f64),

    // Principled BSDF
    pub metallic: f64,
    pub roughness: f64,
    pub transmission: f64,

    // NEW: Per-Element Color Map (e.g., "C" -> (0.1, 0.1, 0.1))
    pub element_colors: HashMap<String, (f64, f64, f64)>,
}

impl Default for RenderStyle {
    fn default() -> Self {
        Self {
            atom_scale: 0.4,
            bond_radius: 0.12,
            bond_color: (0.5, 0.5, 0.5),
            metallic: 0.0,
            roughness: 0.3,
            transmission: 0.0,

            // Start empty. If missing, we use the hardcoded defaults.
            element_colors: HashMap::new(),
        }
    }
}

pub struct AppState {
    pub structure: Option<crate::structure::Structure>,
    pub rot_x: f64,
    pub rot_y: f64,
    pub zoom: f64,
    pub bond_cutoff: f64,
    pub show_axis_x: bool,
    pub show_axis_y: bool,
    pub show_axis_z: bool,
    pub rotation_mode: RotationCenter,
    pub load_conventional: bool,
    pub default_export_format: ExportFormat,
    pub style: RenderStyle,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            structure: None,
            rot_x: 0.0,
            rot_y: 0.0,
            zoom: 1.0,
            bond_cutoff: 2.8,
            show_axis_x: true,
            show_axis_y: true,
            show_axis_z: true,
            rotation_mode: RotationCenter::Centroid,
            load_conventional: false,
            default_export_format: ExportFormat::Png,
            style: RenderStyle::default(),
        }
    }
}
