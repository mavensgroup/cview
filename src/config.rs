// src/config.rs
// Persistent preferences — serialized to ~/.config/cview/settings.json
//
// Settings are grouped into:
//   GENERAL     — loaded into ViewState on tab creation (all effective)
//   APPEARANCE  — colors in RenderStyle, display toggles
//   EXPORT/PLOT — font sizes, line widths, default colormap for charge density export
//
// Legacy/aspirational fields are retained for backward-compatible JSON deserialization
// but are NOT exposed in the Preferences UI.  They carry `#[serde(default)]` so
// missing keys in old config files won't cause parse failures.
//
// TODO: Wire default_atom_scale / default_bond_radius into TabState::new()
// TODO: Wire cache_size_mb into RenderStyle::create_session_copy()

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
#[derive(Default)]
pub enum RotationCenter {
    #[default]
    Centroid,
    UnitCell,
}


#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub enum ColorMode {
    #[default]
    Element,
    BondValence,
    Coordination,
    Charge,
}


#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub enum RenderQuality {
    #[default]
    Fast,
    High,
}


#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub enum AntialiasLevel {
    None,
    Fast,
    #[default]
    Good,
    Best,
}


// ============================================================================
// EXPORT / PLOT SETTINGS
// ============================================================================

/// Settings for charge density plot export (font sizes, line widths, default colormap).
/// All values are user-tunable via the Preferences → Export / Plot tab.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExportPlotSettings {
    /// Axis label font size in pt (e.g. "a (Å)")
    #[serde(default = "d_font_axis")]
    pub font_size_axis_label: f64,
    /// Tick number font size in pt
    #[serde(default = "d_font_tick")]
    pub font_size_tick_label: f64,
    /// Plane annotation font size in pt (e.g. "(001) z = 0.500")
    #[serde(default = "d_font_annotation")]
    pub font_size_annotation: f64,
    /// Colorbar value font size in pt
    #[serde(default = "d_font_colorbar")]
    pub font_size_colorbar: f64,
    /// Isoline line width for export
    #[serde(default = "d_isoline_width")]
    pub isoline_line_width: f64,
    /// Default colormap index: 0=Viridis, 1=Plasma, 2=BlueWhiteRed, 3=Grayscale
    #[serde(default = "d_colormap_idx")]
    pub default_colormap: usize,
}

fn d_font_axis() -> f64 {
    14.0
}
fn d_font_tick() -> f64 {
    11.0
}
fn d_font_annotation() -> f64 {
    12.0
}
fn d_font_colorbar() -> f64 {
    11.0
}
fn d_isoline_width() -> f64 {
    1.8
}
fn d_colormap_idx() -> usize {
    0
}

impl Default for ExportPlotSettings {
    fn default() -> Self {
        Self {
            font_size_axis_label: 14.0,
            font_size_tick_label: 11.0,
            font_size_annotation: 12.0,
            font_size_colorbar: 11.0,
            isoline_line_width: 1.8,
            default_colormap: 0,
        }
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
    // BVS thresholds live here (per-tab, tuned live via sidebar sliders).
    // These are NOT exposed in Preferences — they are structure-dependent.
    pub bvs_threshold_good: f64,
    pub bvs_threshold_warn: f64,

    // Polyhedra settings
    pub polyhedra_settings: Option<PolyhedraSettings>,

    // SOTA LRU sprite cache (not serialized)
    pub atom_cache: Rc<RefCell<SpriteCache>>,
    pub show_labels: bool,
}

// Manual Serialize implementation (skip atom_cache)
impl Serialize for RenderStyle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RenderStyle", 10)?;
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
            show_labels: false,
        })
    }
}

impl RenderStyle {
    pub fn create_session_copy(&self) -> Self {
        let mut copy = self.clone();
        // Create fresh cache for new session
        let cache_size_mb = 200.0; // TODO: Wire config.cache_size_mb here
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
            show_labels: false,
        }
    }
}

// ============================================================================
// MAIN CONFIG
// ============================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    // ── GENERAL (7) — all effective via ViewState::from_config ──
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

    // ── APPEARANCE (6) — colors live in style, toggles via ViewState ──
    #[serde(default)]
    pub render_quality: RenderQuality,
    #[serde(default = "d_true")]
    pub default_show_axes: bool,
    #[serde(default = "d_true")]
    pub default_show_unit_cell: bool,
    #[serde(default = "d_true")]
    pub show_ghost_atoms: bool,
    #[serde(default = "d_atom_scale")]
    pub default_atom_scale: f64, // TODO: wire into TabState::new()
    #[serde(default = "d_bond_rad")]
    pub default_bond_radius: f64, // TODO: wire into TabState::new()

    // ── EXPORT / PLOT — charge density export defaults ──
    #[serde(default)]
    pub export_plot: ExportPlotSettings,

    // ── LEGACY — retained for backward-compat JSON deserialization only ──
    // These fields are NOT exposed in the Preferences UI.
    #[serde(default)]
    pub auto_calc_bvs: bool,
    #[serde(default = "d_true")]
    pub show_bvs_report: bool,
    #[serde(default = "d_true")]
    pub warn_poor_bvs: bool,
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
fn d_max_atoms() -> usize {
    10000
}
fn d_cache() -> usize {
    200
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

            export_plot: ExportPlotSettings::default(),

            // Legacy — kept for serde compat
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
    /// Hard distance cap for coordination bonds (Å). Overrides the covalent-radius
    /// formula when set below the formula result. User-tunable via sidebar slider.
    pub max_bond_dist: f64,
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
            max_bond_dist: 3.5,
        }
    }
}
