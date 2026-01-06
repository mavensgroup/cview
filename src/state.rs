use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use gtk4::glib;
use crate::structure::Atom;
use crate::geometry;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RotationCenter { Centroid, UnitCell }

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat { Png, Pdf }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderStyle {
    pub atom_scale: f64,
    pub bond_radius: f64,
    pub bond_color: (f64, f64, f64),
    pub metallic: f64,
    pub roughness: f64,
    pub transmission: f64,
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
            element_colors: HashMap::new(),
        }
    }
}

pub struct AppState {
    pub structure: Option<crate::structure::Structure>,

    // --- ADDED FIELD ---
    pub file_name: String,
    // ------------------

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
    pub atoms: Vec<Atom>,
    pub selected_indices: Vec<usize>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            structure: None,

            // --- INITIALIZE FIELD ---
            file_name: "structure".to_string(),
            // ------------------------

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
            atoms: Vec::new(),
            selected_indices: Vec::new(),
        }
    }

    // --- Configuration Persistence ---

    fn get_config_path() -> PathBuf {
        let mut path = glib::user_config_dir();
        path.push("cview");
        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!("Warning: Could not create config directory: {}", e);
        }
        path.push("settings.ini");
        path
    }

    pub fn save_config(&self) {
        let kf = glib::KeyFile::new();

        // 1. Save Export Format
        let format_str = match self.default_export_format {
            ExportFormat::Png => "Png",
            ExportFormat::Pdf => "Pdf",
        };
        kf.set_string("Export", "DefaultFormat", format_str);

        // 2. Save Rotation Mode
        let rot_str = match self.rotation_mode {
            RotationCenter::UnitCell => "UnitCell",
            RotationCenter::Centroid => "Centroid",
        };
        kf.set_string("View", "RotationMode", rot_str);

        // Save to file
        let path = Self::get_config_path();
        if let Err(e) = kf.save_to_file(&path) {
            eprintln!("Failed to save config: {}", e);
        } else {
            println!("Config saved to: {:?}", path);
        }
    }

    pub fn load_config(&mut self) {
        let kf = glib::KeyFile::new();
        let path = Self::get_config_path();

        if let Err(e) = kf.load_from_file(&path, glib::KeyFileFlags::NONE) {
            println!("No config found (using defaults): {}", e);
            return;
        }

        // 1. Load Export Format
        if let Ok(fmt) = kf.string("Export", "DefaultFormat") {
            self.default_export_format = match fmt.as_str() {
                "Pdf" => ExportFormat::Pdf,
                _ => ExportFormat::Png,
            };
        }

        // 2. Load Rotation Mode
        if let Ok(rot) = kf.string("View", "RotationMode") {
            self.rotation_mode = match rot.as_str() {
                "UnitCell" => RotationCenter::UnitCell,
                _ => RotationCenter::Centroid,
            };
        }

        println!("Loaded config: Export={:?}, Rotation={:?}", self.default_export_format, self.rotation_mode);
    }

    // --- Helpers ---

    pub fn toggle_selection(&mut self, index: usize) {
        if let Some(pos) = self.selected_indices.iter().position(|&i| i == index) {
            self.selected_indices.remove(pos);
        } else {
            self.selected_indices.push(index);
            if self.selected_indices.len() > 4 {
                self.selected_indices.remove(0);
            }
        }
    }

    pub fn get_geometry_report(&self) -> String {
        let s = match &self.structure {
            Some(s) => s,
            None => return "No structure loaded.".to_string(),
        };

        let selected = &self.selected_indices;

        match selected.len() {
            0 => "Select 2, 3, or 4 atoms to measure.".to_string(),
            1 => format!("Selected: {} (#{})", s.atoms[selected[0]].element, selected[0]),
            2 => {
                let p1 = s.atoms[selected[0]].position;
                let p2 = s.atoms[selected[1]].position;
                let d = geometry::calculate_distance(p1, p2);
                format!("Distance: {:.4} Å", d)
            },
            3 => {
                let p1 = s.atoms[selected[0]].position;
                let p2 = s.atoms[selected[1]].position; // center
                let p3 = s.atoms[selected[2]].position;
                let angle = geometry::calculate_angle(p1, p2, p3);
                format!("Angle: {:.2}°", angle)
            },
            4 => {
                let p1 = s.atoms[selected[0]].position;
                let p2 = s.atoms[selected[1]].position;
                let p3 = s.atoms[selected[2]].position;
                let p4 = s.atoms[selected[3]].position;
                let dihedral = geometry::calculate_dihedral(p1, p2, p3, p4);
                format!("Dihedral: {:.2}°", dihedral)
            },
            _ => format!("{} atoms selected.", selected.len()),
        }
    }
}
