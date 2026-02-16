// src/config.rs
// COMPREHENSIVE CONFIG - All 31 settings for permanent preferences

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::rc::Rc;

// Import SOTA sprite cache
use crate::rendering::sprite_cache::SpriteCache;

// ============================================================================
// ENUMS
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RotationCenter {
    Centroid,
    UnitCell,
}

impl Default for RotationCenter {
    fn default() -> Self {
        RotationCenter::Centroid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ColorMode {
    Element,
    BondValence,
    Coordination,
    Charge,
}

impl Default for ColorMode {
    fn default() -> Self {
        ColorMode::Element
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RenderQuality {
    Fast,
    High,
}

impl Default for RenderQuality {
    fn default() -> Self {
        RenderQuality::Fast
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AntialiasLevel {
    None,
    Fast,
    Good,
    Best,
}

impl Default for AntialiasLevel {
    fn default() -> Self {
        AntialiasLevel::Good
    }
}

// ============================================================================
// RENDER STYLE
// ============================================================================

#[derive(Clone, Debug)]
pub struct RenderStyle {
    pub atom_scale: f64,
    pub bond_radius: f64,
    pub bond_color: (f64, f64, f64),
    pub background_color: (f64, f64, f64),
    pub metallic: f64,
    pub roughness: f64,
    pub transmission: f64,
    pub element_colors: HashMap<String, (f64, f64, f64)>,
    pub color_mode: ColorMode,
    pub bvs_threshold_good: f64,
    pub bvs_threshold_warn: f64,

    // Polyhedra settings
    pub polyhedra_settings: Option<PolyhedraSettings>,

    // SOTA LRU sprite cache (not serialized)
    pub atom_cache: Rc<RefCell<SpriteCache>>,
}

// Manual Serialize implementation (skip atom_cache)
impl Serialize for RenderStyle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RenderStyle", 8)?;
        state.serialize_field("atom_scale", &self.atom_scale)?;
        state.serialize_field("bond_radius", &self.bond_radius)?;
        state.serialize_field("bond_color", &self.bond_color)?;
        state.serialize_field("background_color", &self.background_color)?;
        state.serialize_field("metallic", &self.metallic)?;
        state.serialize_field("roughness", &self.roughness)?;
        state.serialize_field("transmission", &self.transmission)?;
        state.serialize_field("color_mode", &self.color_mode)?;
        state.serialize_field("bvs_threshold_good", &self.bvs_threshold_good)?;
        state.serialize_field("bvs_threshold_warn", &self.bvs_threshold_warn)?;
        state.end()
    }
}

// Manual Deserialize implementation
impl<'de> Deserialize<'de> for RenderStyle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RenderStyleData {
            atom_scale: f64,
            bond_radius: f64,
            bond_color: (f64, f64, f64),
            background_color: (f64, f64, f64),
            metallic: f64,
            roughness: f64,
            transmission: f64,
            color_mode: ColorMode,
            bvs_threshold_good: f64,
            bvs_threshold_warn: f64,
        }

        let data = RenderStyleData::deserialize(deserializer)?;
        Ok(RenderStyle {
            atom_scale: data.atom_scale,
            bond_radius: data.bond_radius,
            bond_color: data.bond_color,
            background_color: data.background_color,
            metallic: data.metallic,
            roughness: data.roughness,
            transmission: data.transmission,
            element_colors: HashMap::new(),
            color_mode: data.color_mode,
            bvs_threshold_good: data.bvs_threshold_good,
            bvs_threshold_warn: data.bvs_threshold_warn,
            polyhedra_settings: None,
            atom_cache: Rc::new(RefCell::new(SpriteCache::default())),
        })
    }
}

impl RenderStyle {
    pub fn create_session_copy(&self) -> Self {
        let mut copy = self.clone();
        // Create fresh cache for new session
        let cache_size_mb = 200.0; // TODO: Get from config
        copy.atom_cache = Rc::new(RefCell::new(SpriteCache::new(cache_size_mb)));
        copy
    }
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
            color_mode: ColorMode::Element,
            bvs_threshold_good: 0.15,
            bvs_threshold_warn: 0.40,
            polyhedra_settings: None,
            atom_cache: Rc::new(RefCell::new(SpriteCache::default())),
        }
    }
}

// ============================================================================
// MAIN CONFIG - All 31 Settings
// ============================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    // GENERAL (7)
    #[serde(default = "d_true")]
    pub default_show_full_cell: bool,
    #[serde(default = "d_true")]
    pub default_show_bonds: bool,
    #[serde(default = "d_bond_tol")]
    pub default_bond_tolerance: f64,
    #[serde(default)]
    pub rotation_mode: RotationCenter,
    #[serde(default = "d_one")]
    pub default_zoom: f64,
    #[serde(default = "d_true")]
    pub auto_center_structure: bool,
    #[serde(default)]
    pub remember_last_view: bool,

    // APPEARANCE (6 settings) - Colors are in style
    #[serde(default)]
    pub render_quality: RenderQuality,
    #[serde(default = "d_true")]
    pub default_show_axes: bool,
    #[serde(default = "d_true")]
    pub default_show_unit_cell: bool,
    #[serde(default = "d_true")]
    pub show_ghost_atoms: bool,
    #[serde(default = "d_atom_scale")]
    pub default_atom_scale: f64,
    #[serde(default = "d_bond_rad")]
    pub default_bond_radius: f64,

    // BVS (5)
    #[serde(default = "d_bvs_good")]
    pub bvs_threshold_good: f64,
    #[serde(default = "d_bvs_warn")]
    pub bvs_threshold_warn: f64,
    #[serde(default)]
    pub auto_calc_bvs: bool,
    #[serde(default = "d_true")]
    pub show_bvs_report: bool,
    #[serde(default = "d_true")]
    pub warn_poor_bvs: bool,

    // PERFORMANCE (5)
    #[serde(default)]
    pub antialias_level: AntialiasLevel,
    #[serde(default = "d_max_atoms")]
    pub max_atoms_display: usize,
    #[serde(default = "d_true")]
    pub use_hardware_acceleration: bool,
    #[serde(default = "d_true")]
    pub enable_sprite_cache: bool,
    #[serde(default = "d_cache")]
    pub cache_size_mb: usize,

    // ADVANCED (5)
    #[serde(default)]
    pub show_fps: bool,
    #[serde(default)]
    pub verbose_logging: bool,
    #[serde(default)]
    pub enable_experimental: bool,
    #[serde(default)]
    pub show_measurement_labels: bool,
    #[serde(default)]
    pub auto_detect_format: bool,

    // LEGACY
    #[serde(default)]
    pub style: RenderStyle,
    #[serde(default)]
    pub load_conventional: bool,
}

// Defaults
fn d_true() -> bool {
    true
}
fn d_one() -> f64 {
    1.0
}
fn d_bond_tol() -> f64 {
    1.15
}
fn d_atom_scale() -> f64 {
    0.4
}
fn d_bond_rad() -> f64 {
    0.12
}
fn d_bvs_good() -> f64 {
    0.10
}
fn d_bvs_warn() -> f64 {
    0.30
}
fn d_max_atoms() -> usize {
    10000
}
fn d_cache() -> usize {
    200
}
fn d_bg() -> (f64, f64, f64) {
    (0.95, 0.95, 0.95)
}
fn d_gray() -> (f64, f64, f64) {
    (0.5, 0.5, 0.5)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_show_full_cell: true,
            default_show_bonds: true,
            default_bond_tolerance: 1.15,
            rotation_mode: RotationCenter::Centroid,
            default_zoom: 1.0,
            auto_center_structure: true,
            remember_last_view: false,

            render_quality: RenderQuality::Fast,
            default_show_axes: true,
            default_show_unit_cell: true,
            show_ghost_atoms: true,
            default_atom_scale: 0.4,
            default_bond_radius: 0.12,

            bvs_threshold_good: 0.10,
            bvs_threshold_warn: 0.30,
            auto_calc_bvs: false,
            show_bvs_report: true,
            warn_poor_bvs: true,

            antialias_level: AntialiasLevel::Good,
            max_atoms_display: 10000,
            use_hardware_acceleration: true,
            enable_sprite_cache: true,
            cache_size_mb: 200,

            show_fps: false,
            verbose_logging: false,
            enable_experimental: false,
            show_measurement_labels: false,
            auto_detect_format: true,

            style: RenderStyle::default(),
            load_conventional: false,
        }
    }
}

impl Config {
    pub fn load() -> (Self, String) {
        let path = Self::get_path();
        if path.exists() {
            match File::open(&path) {
                Ok(file) => {
                    let reader = BufReader::new(file);
                    match serde_json::from_reader(reader) {
                        Ok(cfg) => (cfg, format!("Config loaded from {:?}", path)),
                        Err(e) => (Self::default(), format!("Error: {}. Using defaults.", e)),
                    }
                }
                Err(e) => (Self::default(), format!("Error: {}. Using defaults.", e)),
            }
        } else {
            (
                Self::default(),
                "No config found. Using defaults.".to_string(),
            )
        }
    }

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
                    Err(e) => format!("Failed: {}", e),
                }
            }
            Err(e) => format!("Could not create config: {}", e),
        }
    }

    fn get_path() -> PathBuf {
        if let Some(proj) = ProjectDirs::from("com", "example", "cview") {
            proj.config_dir().join("settings.json")
        } else {
            PathBuf::from("settings.json")
        }
    }
}

// ============================================================================
// POLYHEDRA SETTINGS
// ============================================================================

#[derive(Debug, Clone)]
pub struct PolyhedraSettings {
    pub show_polyhedra: bool,
    pub enabled_elements: Vec<String>,
    pub transparency: f64,
    pub show_edges: bool,
    pub min_coordination: usize,
    pub max_coordination: usize,
    pub color_mode: PolyhedraColorMode,
}

#[derive(Debug, Clone)]
pub enum PolyhedraColorMode {
    Element,
    Coordination,
    Custom(f64, f64, f64),
}

impl Default for PolyhedraSettings {
    fn default() -> Self {
        Self {
            show_polyhedra: false,
            enabled_elements: vec![],
            transparency: 0.3,
            show_edges: true,
            min_coordination: 4,
            max_coordination: 12,
            color_mode: PolyhedraColorMode::Element,
        }
    }
}
