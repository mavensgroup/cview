// src/state.rs

use std::collections::HashSet;

use crate::config::Config;
use crate::model::miller::MillerPlane;
use crate::model::structure::Structure;
use crate::physics::analysis::{kpath::KPathResult, voids::VoidResult};

// --- Sub-structs for organization ---

// Volatile View State (Camera, toggles) - NOT saved to config
#[derive(Debug, Clone)]
pub struct ViewState {
    pub rot_x: f64,
    pub rot_y: f64,
    pub rot_z: f64,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub show_bonds: bool,
    pub show_axes: [bool; 3], // x, y, z
    pub bond_cutoff: f64,
    pub scale: f64,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            rot_x: 0.0,
            rot_y: 0.0,
            rot_z: 0.0,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            show_bonds: true,
            show_axes: [true, true, true],
            bond_cutoff: 2.8,
            scale: 1.0,
        }
    }
}

// Interaction State (Selections, Undo) - NOT saved to config
#[derive(Default)]
pub struct InteractionState {
    pub selected_indices: HashSet<usize>,
    pub undo_stack: Vec<Structure>,
    pub is_shift_pressed: bool,
    pub selection_box: Option<((f64, f64), (f64, f64))>,
}

// --- Main AppState ---

pub struct AppState {
    // Core Data (Model)
    pub structure: Option<Structure>,
    pub original_structure: Option<Structure>,
    pub miller_planes: Vec<MillerPlane>,
    pub file_name: String,

    // Organized Sub-states
    pub config: Config,  // Persistent settings (Style, Rotation Mode)
    pub view: ViewState, // Volatile view settings (Camera positions)
    pub interaction: InteractionState, // Volatile interaction settings

    // Transient Analysis Results
    pub kpath_result: Option<KPathResult>,
    pub void_result: Option<VoidResult>,
}

impl AppState {
    /// Initialize state and try loading config
    pub fn new_with_log() -> (Self, String) {
        let (config, log) = Config::load();

        let state = Self {
            structure: None,
            original_structure: None,
            miller_planes: Vec::new(),
            file_name: String::from("Untitled"),

            config,
            view: ViewState::default(),
            interaction: InteractionState::default(),

            kpath_result: None,
            void_result: None,
        };
        (state, log)
    }

    pub fn save_config(&self) -> String {
        self.config.save()
    }

    // --- Helpers ---

    pub fn push_undo(&mut self) {
        if let Some(s) = &self.structure {
            if self.interaction.undo_stack.len() >= 20 {
                self.interaction.undo_stack.remove(0);
            }
            self.interaction.undo_stack.push(s.clone());
        }
    }

    pub fn undo(&mut self) -> String {
        if let Some(prev_struct) = self.interaction.undo_stack.pop() {
            self.structure = Some(prev_struct);
            self.interaction.selected_indices.clear();
            "Undo successful.".to_string()
        } else {
            "Nothing to undo.".to_string()
        }
    }

    pub fn delete_selected(&mut self) -> String {
        if self.interaction.selected_indices.is_empty() {
            return "No atoms selected.".to_string();
        }
        self.push_undo();

        if let Some(s) = &mut self.structure {
            let initial = s.atoms.len();
            let mut new_atoms = Vec::new();
            for (i, atom) in s.atoms.drain(..).enumerate() {
                if !self.interaction.selected_indices.contains(&i) {
                    new_atoms.push(atom);
                }
            }
            s.atoms = new_atoms;
            self.interaction.selected_indices.clear();
            format!("Deleted {} atoms.", initial - s.atoms.len())
        } else {
            "No structure loaded.".to_string()
        }
    }
    pub fn toggle_selection(&mut self, index: usize) {
        if self.interaction.selected_indices.contains(&index) {
            self.interaction.selected_indices.remove(&index);
        } else {
            self.interaction.selected_indices.insert(index);
        }
    }
}
