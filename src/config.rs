// src/config.rs

use directories::ProjectDirs;
use gtk4::cairo::ImageSurface;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::rc::Rc;

// --- Enums ---

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

// --- RenderStyle ---
// Moved here so Config can own it

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

  // Helper cache (skipped in serialization)
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
      atom_cache: Rc::new(RefCell::new(HashMap::new())),
    }
  }
}

// --- Main Config Struct ---

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
  #[serde(default)]
  pub load_conventional: bool,

  #[serde(default)]
  pub show_full_unit_cell: bool,
  pub rotation_mode: RotationCenter,
  pub default_export_format: ExportFormat,

  #[serde(default)]
  pub style: RenderStyle,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      rotation_mode: RotationCenter::Centroid,
      load_conventional: false,
      show_full_unit_cell: true,
      default_export_format: ExportFormat::Png,
      style: RenderStyle::default(),
    }
  }
}

impl Config {
  /// Loads config from standard OS location (e.g., ~/.config/cview/settings.json)
  pub fn load() -> (Self, String) {
    let path = Self::get_path();
    if path.exists() {
      match File::open(&path) {
        Ok(file) => {
          let reader = BufReader::new(file);
          match serde_json::from_reader(reader) {
            Ok(cfg) => (cfg, format!("Config loaded from {:?}", path)),
            Err(e) => (Self::default(), format!("Error parsing config: {}", e)),
          }
        }
        Err(e) => (Self::default(), format!("Error opening config: {}", e)),
      }
    } else {
      (
        Self::default(),
        "No config found. Using defaults.".to_string(),
      )
    }
  }

  /// Saves config to standard OS location
  pub fn save(&self) -> String {
    let path = Self::get_path();
    if let Some(parent) = path.parent() {
      let _ = fs::create_dir_all(parent);
    }

    match File::create(&path) {
      Ok(file) => {
        let writer = BufWriter::new(file);
        match serde_json::to_writer_pretty(writer, self) {
          Ok(_) => format!("Config saved to {:?}", path),
          Err(e) => format!("Failed to save config: {}", e),
        }
      }
      Err(e) => format!("Could not create config file: {}", e),
    }
  }

  fn get_path() -> PathBuf {
    // "com.example.cview" should match your Application ID in main.rs
    if let Some(proj) = ProjectDirs::from("com", "example", "cview") {
      proj.config_dir().join("settings.json")
    } else {
      PathBuf::from("settings.json")
    }
  }
}
