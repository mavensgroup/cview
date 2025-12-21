use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RotationCenter {
    Centroid,
    UnitCell,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Pdf,
}

// Updated RenderStyle with Atom Color settings
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RenderStyle {
    pub atom_scale: f64,
    pub bond_radius: f64,
    pub bond_color: (f64, f64, f64),
    pub shine_strength: f64,
    pub shine_hardness: f64,

    // NEW: Atom Color Preferences
    pub use_uniform_atom_color: bool, // Toggle switch
    pub atom_color: (f64, f64, f64),  // The custom color
}

impl Default for RenderStyle {
    fn default() -> Self {
        Self {
            atom_scale: 0.4,
            bond_radius: 0.12,
            bond_color: (0.5, 0.5, 0.5),
            shine_strength: 0.9,
            shine_hardness: 0.05,

            // Default: Use Element Colors (CPK), but if toggled, use Silver
            use_uniform_atom_color: false,
            atom_color: (0.7, 0.7, 0.7),
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
