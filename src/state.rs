// src/state.rs
// Updated to use Config defaults

use crate::config::{Config, RenderStyle};
use crate::model::miller::MillerPlane;
use crate::model::structure::Structure;
use crate::physics::analysis::{kpath::KPathResult, voids::VoidResult};
use nalgebra::{Rotation3, UnitQuaternion, Vector3};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ViewState {
    /// Camera orientation as a unit quaternion. Mouse drag composes screen-space
    /// deltas onto this (trackball behavior); presets and Euler sliders set it
    /// from absolute angles. Quaternion form keeps composition numerically stable
    /// across many incremental drags.
    pub rotation: UnitQuaternion<f64>,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub show_bonds: bool,
    pub show_axes: [bool; 3],
    pub bond_cutoff: f64,
    pub scale: f64,
    pub show_full_unit_cell: bool,
}

impl ViewState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            rotation: UnitQuaternion::identity(),
            zoom: config.default_zoom,
            pan_x: 0.0,
            pan_y: 0.0,
            show_bonds: config.default_show_bonds,
            show_axes: [config.default_show_axes; 3],
            bond_cutoff: config.default_bond_tolerance,
            scale: 30.0,
            show_full_unit_cell: config.default_show_full_cell,
        }
    }

    /// Rotation matrix the renderer applies to world-space points.
    pub fn rotation_matrix(&self) -> Rotation3<f64> {
        self.rotation.to_rotation_matrix()
    }

    /// Set absolute orientation from XYZ-intrinsic Euler angles in degrees
    /// (composition order `Rz * Ry * Rx`, matching the previous behavior).
    pub fn set_euler_xyz_deg(&mut self, rx_deg: f64, ry_deg: f64, rz_deg: f64) {
        self.rotation = UnitQuaternion::from_euler_angles(
            rx_deg.to_radians(),
            ry_deg.to_radians(),
            rz_deg.to_radians(),
        );
    }

    /// Decompose to XYZ-intrinsic Euler angles in degrees. Used to seed the
    /// sidebar sliders; values may be negative or wrap, and gimbal-lock cases
    /// produce one of many equivalent decompositions.
    pub fn euler_xyz_deg(&self) -> (f64, f64, f64) {
        let (rx, ry, rz) = self.rotation.euler_angles();
        (rx.to_degrees(), ry.to_degrees(), rz.to_degrees())
    }

    /// Apply a screen-space rotation increment (trackball). `yaw_deg` rotates
    /// around the screen's vertical axis (horizontal drag), `pitch_deg` around
    /// the screen's horizontal axis (vertical drag). Left-multiplied so the
    /// delta is interpreted in camera space regardless of current orientation.
    pub fn apply_screen_rotation_deg(&mut self, yaw_deg: f64, pitch_deg: f64) {
        let yaw = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw_deg.to_radians());
        let pitch = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch_deg.to_radians());
        self.rotation = yaw * pitch * self.rotation;
    }

    pub fn reset_rotation(&mut self) {
        self.rotation = UnitQuaternion::identity();
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            rotation: UnitQuaternion::identity(),
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            show_bonds: true,
            show_axes: [true, true, true],
            bond_cutoff: 1.15,
            scale: 30.0,
            show_full_unit_cell: true,
        }
    }
}

/// One picked atom instance. Selection is keyed by `unique_id` from the
/// renderer's scene pass, so each ghost copy at the cell boundary is a
/// distinct selectable target. `cart_pos` is the actual world position of
/// the clicked instance — required because ghost copies sit at cartesian
/// offsets that differ from the underlying `structure.atoms[original_index]`.
#[derive(Debug, Clone)]
pub struct SelectedAtom {
    pub unique_id: usize,
    pub original_index: usize,
    pub cart_pos: [f64; 3],
    pub element: String,
}

/// Per-atom render override. Purely cosmetic — never written to any IO format.
/// Keyed by `Atom` index in `Structure.atoms`. Lives only in the session;
/// reload from file resets it. Use the Tools → Atom Instances dialog to edit.
#[derive(Debug, Clone, Default)]
pub struct AtomOverride {
    /// Display label shown in tooltips/dialog (e.g. "Fe1", "Fe_oct"). Element
    /// identity stays in `Atom.element`; this is just a tag for the user.
    pub display_label: Option<String>,
    /// RGB in 0..=1. When set, replaces the element's color at draw time.
    pub color: Option<(f64, f64, f64)>,
    /// Multiplier on the element's covalent radius. None ⇒ 1.0.
    pub radius_scale: Option<f64>,
}

impl AtomOverride {
    pub fn is_empty(&self) -> bool {
        self.display_label.is_none() && self.color.is_none() && self.radius_scale.is_none()
    }
}

#[derive(Default)]
pub struct InteractionState {
    pub selected: HashMap<usize, SelectedAtom>,
    pub undo_stack: Vec<Structure>,
    pub is_shift_pressed: bool,
    pub selection_box: Option<((f64, f64), (f64, f64))>,
    /// Cumulative drag offset reported on the previous drag-update event.
    /// Reset to (0, 0) on drag-begin. Used to derive per-frame deltas for
    /// trackball rotation, since GTK's GestureDrag reports cumulative offset.
    pub drag_prev_offset: (f64, f64),
}

pub struct TabState {
    pub structure: Option<Structure>,
    pub original_structure: Option<Structure>,
    pub miller_planes: Vec<MillerPlane>,
    pub file_name: String,
    pub view: ViewState,
    pub interaction: InteractionState,
    pub style: RenderStyle,
    pub kpath_result: Option<KPathResult>,
    pub void_result: Option<VoidResult>,
    pub bvs_cache: Vec<f64>,
    pub bvs_cache_valid: bool,
    /// Per-atom cosmetic overrides keyed by index into `structure.atoms`.
    /// Indices that aren't present here render with element defaults.
    pub overrides: HashMap<usize, AtomOverride>,
}

impl TabState {
    pub fn new(global_config: &Config) -> Self {
        Self {
            structure: None,
            original_structure: None,
            miller_planes: Vec::new(),
            file_name: String::from("Untitled"),
            view: ViewState::from_config(global_config),
            interaction: InteractionState::default(),
            style: global_config.style.create_session_copy(),
            kpath_result: None,
            void_result: None,
            bvs_cache: Vec::new(),
            bvs_cache_valid: false,
            overrides: HashMap::new(),
        }
    }

    /// Color to use for atom at `index`: override if present, else None
    /// (caller falls back to element/BVS color).
    pub fn override_color(&self, index: usize) -> Option<(f64, f64, f64)> {
        self.overrides.get(&index).and_then(|o| o.color)
    }

    /// Multiplier on covalent radius for atom at `index` (defaults to 1.0).
    pub fn override_radius_scale(&self, index: usize) -> f64 {
        self.overrides
            .get(&index)
            .and_then(|o| o.radius_scale)
            .unwrap_or(1.0)
    }

    /// Display label for atom at `index`, or `None` if unset.
    pub fn override_label(&self, index: usize) -> Option<&str> {
        self.overrides
            .get(&index)
            .and_then(|o| o.display_label.as_deref())
    }

    pub fn invalidate_bvs_cache(&mut self) {
        self.bvs_cache_valid = false;
    }

    pub fn get_bvs_values(&mut self) -> &[f64] {
        if !self.bvs_cache_valid {
            if let Some(ref structure) = self.structure {
                use crate::physics::bond_valence::calculator::calculate_bvs_all_auto;
                self.bvs_cache = calculate_bvs_all_auto(structure);
                self.bvs_cache_valid = true;
            }
        }
        &self.bvs_cache
    }
}

pub struct AppState {
    pub tabs: Vec<TabState>,
    pub active_tab_index: usize,
    pub config: Config,
}

impl AppState {
    pub fn new() -> Self {
        let (config, msg) = Config::load();
        crate::utils::console::log_info(&msg);
        Self {
            tabs: vec![],
            active_tab_index: 0,
            config,
        }
    }

    pub fn new_with_log() -> (Self, String) {
        let (config, log) = Config::load();
        let initial_tab = TabState::new(&config);
        let state = Self {
            config,
            tabs: vec![initial_tab],
            active_tab_index: 0,
        };
        (state, log)
    }

    pub fn active_tab(&self) -> &TabState {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn save_config(&self) -> String {
        self.config.save()
    }

    pub fn add_tab(&mut self, structure: Structure, filename: String) {
        let mut new_tab = TabState::new(&self.config);
        new_tab.original_structure = Some(structure.clone());
        new_tab.structure = Some(structure);
        new_tab.file_name = filename;
        if let Some(ref s) = new_tab.structure {
            new_tab.bvs_cache.resize(s.atoms.len(), 0.0);
        }
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
    }

    pub fn remove_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.active_tab_index = 0;
            } else if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
            } else if index < self.active_tab_index {
                self.active_tab_index -= 1;
            }
        }
    }

    pub fn toggle_selection(&mut self, atom: SelectedAtom) {
        let tab = self.active_tab_mut();
        if tab.interaction.selected.remove(&atom.unique_id).is_none() {
            tab.interaction.selected.insert(atom.unique_id, atom);
        }
    }

    pub fn delete_selected(&mut self) -> String {
        let tab = self.active_tab_mut();
        if tab.interaction.selected.is_empty() {
            return "No atoms selected.".to_string();
        }
        if let Some(ref mut structure) = tab.structure {
            tab.interaction.undo_stack.push(structure.clone());

            // Multiple ghost copies share an original_index — dedupe before deleting,
            // otherwise we'd try to remove the same atom multiple times.
            let mut indices: Vec<usize> = tab
                .interaction
                .selected
                .values()
                .map(|s| s.original_index)
                .collect();
            indices.sort_unstable();
            indices.dedup();
            let count = indices.len();
            // Remove from highest index first to keep earlier indices valid.
            indices.sort_by(|a, b| b.cmp(a));
            for &idx in &indices {
                if idx < structure.atoms.len() {
                    structure.atoms.remove(idx);
                }
            }
            tab.interaction.selected.clear();
            tab.invalidate_bvs_cache();
            // Atom indices shifted — overrides keyed on those indices are no
            // longer meaningful. Drop them rather than try to remap.
            tab.overrides.clear();
            format!("Deleted {} atom(s)", count)
        } else {
            "No structure loaded.".to_string()
        }
    }

    pub fn undo(&mut self) -> String {
        let tab = self.active_tab_mut();
        if let Some(prev_structure) = tab.interaction.undo_stack.pop() {
            tab.structure = Some(prev_structure);
            tab.interaction.selected.clear();
            tab.invalidate_bvs_cache();
            // Same reasoning as `delete_selected`: undo can shift atom counts
            // and indices, so any overrides that pointed to the post-delete
            // arrangement are stale.
            tab.overrides.clear();
            "Undo successful.".to_string()
        } else {
            "Nothing to undo.".to_string()
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
