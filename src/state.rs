// src/state.rs

use directories::ProjectDirs;
use gtk4::cairo::ImageSurface;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::rc::Rc; // <--- Handling OS paths

use crate::model::miller::MillerPlane;
use crate::model::structure::Structure;
use crate::utils::geometry;
// We remove 'crate::config' because we handle it here now
use crate::physics::analysis::kpath::KPathResult;
use crate::physics::analysis::voids::VoidResult;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderStyle {
  pub atom_scale: f64,
  pub bond_radius: f64,
  pub bond_color: (f64, f64, f64),
  pub background_color: (f64, f64, f64),
  pub metallic: f64,
  pub roughness: f64,
  pub transmission: f64,
  pub element_colors: HashMap<String, (f64, f64, f64)>,

  // --- The Sprite Cache ---
  #[serde(skip)]
  pub atom_cache: Rc<RefCell<HashMap<String, ImageSurface>>>,
}

impl Default for RenderStyle {
  fn default() -> Self {
    Self {
      atom_scale: 0.4,
      bond_radius: 0.12,
      bond_color: (0.5, 0.5, 0.5),
      background_color: (0.9, 0.9, 0.9),
      metallic: 0.0,
      roughness: 0.3,
      transmission: 0.0,
      element_colors: HashMap::new(),
      // Initialize empty cache
      atom_cache: Rc::new(RefCell::new(HashMap::new())),
    }
  }
}

// Helper struct to serialize only the config parts of AppState
#[derive(Serialize, Deserialize)]
struct AppConfig {
  style: RenderStyle,
  rotation_mode: RotationCenter,
  load_conventional: bool,
  default_export_format: ExportFormat,
}

pub struct AppState {
  pub structure: Option<Structure>,
  pub original_structure: Option<Structure>,
  pub miller_planes: Vec<MillerPlane>,
  pub file_name: String,
  pub rot_x: f64,
  pub rot_y: f64,
  pub rot_z: f64,
  pub zoom: f64,
  pub rotation_mode: RotationCenter,
  pub selected_indices: Vec<usize>,
  pub show_bonds: bool,
  pub bond_cutoff: f64,
  pub show_axis_x: bool,
  pub show_axis_y: bool,
  pub show_axis_z: bool,
  pub style: RenderStyle,
  pub load_conventional: bool,
  pub default_export_format: ExportFormat,
  pub scale: f64,
  // --- Physics Results (Transient) ---
  pub kpath_result: Option<KPathResult>,
  pub void_result: Option<VoidResult>,
  // Panning (Transient)
  pub pan_x: f64,
  pub pan_y: f64,
}

impl Default for AppState {
  fn default() -> Self {
    let mut state = Self::new();
    state.load_config(); // Try to load from OS default path
    state
  }
}

impl AppState {
  pub fn new() -> Self {
    Self {
      structure: None,
      original_structure: None,
      miller_planes: Vec::new(),
      file_name: String::from("Untitled"),
      rot_x: 0.0,
      rot_y: 0.0,
      rot_z: 0.0,
      zoom: 1.0,
      rotation_mode: RotationCenter::Centroid,
      selected_indices: Vec::new(),
      show_bonds: true,
      bond_cutoff: 2.8,
      show_axis_x: true,
      show_axis_y: true,
      show_axis_z: true,
      style: RenderStyle::default(),
      load_conventional: false,
      default_export_format: ExportFormat::Png,
      scale: 1.0,
      // Initialize new fields for Tools
      kpath_result: None,
      void_result: None,
      pan_x: 0.0,
      pan_y: 0.0,
    }
  }

  /// Helper: Get the platform-specific config file path
  /// Linux:   ~/.config/cview/settings.json
  /// Windows: %APPDATA%\Rudra\CView\config\settings.json
  /// Mac:     ~/Library/Application Support/com.rudra.cview/settings.json
  fn get_config_path() -> PathBuf {
    if let Some(proj_dirs) = ProjectDirs::from("com", "rudra", "cview") {
      let config_dir = proj_dirs.config_dir();
      if !config_dir.exists() {
        let _ = fs::create_dir_all(config_dir);
      }
      config_dir.join("settings.json")
    } else {
      PathBuf::from("settings.json") // Fallback
    }
  }

  pub fn load_config(&mut self) {
    let path = Self::get_config_path();
    if path.exists() {
      match File::open(&path) {
        Ok(file) => {
          let reader = BufReader::new(file);
          match serde_json::from_reader::<_, AppConfig>(reader) {
            Ok(cfg) => {
              // Success (Silent)
              self.rotation_mode = cfg.rotation_mode;
              self.load_conventional = cfg.load_conventional;
              self.default_export_format = cfg.default_export_format;

              // Handle Style & Cache
              let mut loaded_style = cfg.style;
              // Ensure cache is initialized (defensive check)
              if Rc::strong_count(&loaded_style.atom_cache) == 0 {
                loaded_style.atom_cache = Rc::new(RefCell::new(HashMap::new()));
              }
              self.style = loaded_style;
            }
            Err(e) => {
              eprintln!("Failed to parse config: {}", e);
            }
          }
        }
        Err(e) => {
          eprintln!("Failed to open config file: {}", e);
        }
      }
    }
  }

  pub fn save_config(&self) {
    let path = Self::get_config_path();

    // Construct the subset struct
    let cfg = AppConfig {
      style: self.style.clone(),
      rotation_mode: self.rotation_mode,
      load_conventional: self.load_conventional,
      default_export_format: self.default_export_format,
    };

    if let Ok(file) = File::create(&path) {
      let writer = BufWriter::new(file);
      if let Err(e) = serde_json::to_writer_pretty(writer, &cfg) {
        eprintln!("Failed to save config: {}", e);
      }
      // Success case is now silent
    } else {
      eprintln!("Could not create config file at: {:?}", path);
    }
  }

  pub fn toggle_selection(&mut self, index: usize) {
    if let Some(pos) = self.selected_indices.iter().position(|&i| i == index) {
      self.selected_indices.remove(pos);
    } else {
      self.selected_indices.push(index);
    }
  }

  // --- REPORT GENERATION ---

  pub fn get_structure_report(&self) -> String {
    let s = match &self.structure {
      Some(s) => s,
      None => return "No structure loaded.".to_string(),
    };

    let mut counts: HashMap<String, usize> = HashMap::new();
    for atom in &s.atoms {
      *counts.entry(atom.element.clone()).or_insert(0) += 1;
    }
    let mut parts: Vec<_> = counts.into_iter().collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));

    let formula_str: String = parts
      .iter()
      .map(|(el, count)| format!("{}{}", el, count))
      .collect::<Vec<_>>()
      .join(" ");

    let mut out = String::new();
    out.push_str(&format!("File: {}\n", self.file_name));
    out.push_str(&format!("Formula: {}\n", formula_str));
    out.push_str("--------------------------------------------------\n");
    out.push_str(&format!(
      "{:<8} {:<8} {:<10} {:<10} {:<10}\n",
      "Index", "Element", "X", "Y", "Z"
    ));
    out.push_str("--------------------------------------------------\n");

    for (i, atom) in s.atoms.iter().take(20).enumerate() {
      out.push_str(&format!(
        "{:<8} {:<8} {:<10.4} {:<10.4} {:<10.4}\n",
        i, atom.element, atom.position[0], atom.position[1], atom.position[2]
      ));
    }
    if s.atoms.len() > 20 {
      out.push_str(&format!("... and {} more atoms.\n", s.atoms.len() - 20));
    }
    out
  }

  pub fn get_geometry_report(&self) -> String {
    let s = match &self.structure {
      Some(s) => s,
      None => return "No structure loaded.".to_string(),
    };

    let sel = &self.selected_indices;
    if sel.is_empty() {
      return "Select atoms to measure.".to_string();
    }

    let mut out = String::new();
    out.push_str("Selection:\n");

    for (i, &idx) in sel.iter().enumerate() {
      let atom = &s.atoms[idx];
      if i > 0 {
        out.push_str(" - ");
      }
      out.push_str(&format!("Atom (#{}, {})", idx, atom.element));
    }
    out.push_str("\n\n");

    match sel.len() {
      2 => {
        let p1 = s.atoms[sel[0]].position;
        let p2 = s.atoms[sel[1]].position;
        let d = geometry::calculate_distance(p1, p2);
        out.push_str(&format!("Distance: {:.5} Å", d));
      }
      3 => {
        let p1 = s.atoms[sel[0]].position;
        let p2 = s.atoms[sel[1]].position;
        let p3 = s.atoms[sel[2]].position;
        let angle = geometry::calculate_angle(p1, p2, p3);
        let d1 = geometry::calculate_distance(p1, p2);
        let d2 = geometry::calculate_distance(p2, p3);

        out.push_str(&format!("Angle (A-B-C): {:.2}°\n", angle));
        out.push_str(&format!("Dist (A-B):    {:.4} Å\n", d1));
        out.push_str(&format!("Dist (B-C):    {:.4} Å", d2));
      }
      4 => {
        let p1 = s.atoms[sel[0]].position;
        let p2 = s.atoms[sel[1]].position;
        let p3 = s.atoms[sel[2]].position;
        let p4 = s.atoms[sel[3]].position;
        let dihedral = geometry::calculate_dihedral(p1, p2, p3, p4);
        let angle = geometry::calculate_angle(p1, p2, p3);

        out.push_str(&format!("Dihedral:      {:.2}°\n", dihedral));
        out.push_str(&format!("Angle (A-B-C): {:.2}°", angle));
      }
      _ => {
        out.push_str("Select 2-4 atoms for geometric calculations.");
      }
    }
    out
  }
}
