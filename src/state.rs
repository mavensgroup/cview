// src/state.rs

use std::collections::HashSet;

use crate::config::{Config, RenderStyle};
use crate::model::miller::MillerPlane;
use crate::model::structure::Structure;
use crate::physics::analysis::{kpath::KPathResult, voids::VoidResult};

// --- Sub-structs ---

// Volatile View State (Camera, toggles)
#[derive(Debug, Clone)]
pub struct ViewState {
    pub rot_x: f64,
    pub rot_y: f64,
    pub rot_z: f64,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub show_bonds: bool,
    pub show_axes: [bool; 3],
    pub bond_cutoff: f64,
    pub scale: f64,

    // MOVED FROM GLOBAL CONFIG:
    // This allows each tab to independently toggle between Asymmetric vs Full Unit Cell
    pub show_full_unit_cell: bool,
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
            bond_cutoff: 1.1,           // Default bond tolerance
            scale: 30.0,                // Default visual scale
            show_full_unit_cell: false, // Default to Asymmetric Unit
        }
    }
}

// Interaction State
#[derive(Default)]
pub struct InteractionState {
    pub selected_indices: HashSet<usize>,
    pub undo_stack: Vec<Structure>,
    pub is_shift_pressed: bool,
    pub selection_box: Option<((f64, f64), (f64, f64))>,
}

// --- Tab State (One per open file) ---
pub struct TabState {
    // Content
    pub structure: Option<Structure>,
    pub original_structure: Option<Structure>,
    pub miller_planes: Vec<MillerPlane>,
    pub file_name: String,

    // Volatile Settings (Independent per tab)
    pub view: ViewState,
    pub interaction: InteractionState,

    // The Local Style Override (Volatile)
    // Initialized from Config, but changes here DO NOT affect Config
    pub style: RenderStyle,

    // Analysis
    pub kpath_result: Option<KPathResult>,
    pub void_result: Option<VoidResult>,
}

impl TabState {
    pub fn new(global_config: &Config) -> Self {
        Self {
            structure: None,
            original_structure: None,
            miller_planes: Vec::new(),
            file_name: String::from("Untitled"),
            view: ViewState::default(),
            interaction: InteractionState::default(),
            // Create a session copy so we don't share the atom sprite cache across tabs
            style: global_config.style.create_session_copy(),
            kpath_result: None,
            void_result: None,
        }
    }
}

// --- Main AppState ---

pub struct AppState {
    // Persistent Global Settings
    pub config: Config,

    // Tabs
    pub tabs: Vec<TabState>,
    pub active_tab_index: usize,
}

impl AppState {
    pub fn new_with_log() -> (Self, String) {
        let (config, log) = Config::load();

        // Create initial empty tab
        let initial_tab = TabState::new(&config);

        let state = Self {
            config,
            tabs: vec![initial_tab],
            active_tab_index: 0,
        };
        (state, log)
    }

    pub fn save_config(&self) -> String {
        self.config.save()
    }

    // --- Tab Management ---

    pub fn remove_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);

            // Adjust active index safely
            if self.tabs.is_empty() {
                self.active_tab_index = 0;
            } else if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
            } else if index < self.active_tab_index {
                // If we closed a tab *before* the current one, shift index down
                self.active_tab_index -= 1;
            }
        }
    }

    pub fn active_tab(&self) -> &TabState {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab_index]
    }

    /// Adds a new tab with the loaded structure and switches to it.
    pub fn add_tab(&mut self, structure: Structure, filename: String) {
        // Create a new tab based on global config
        let mut new_tab = TabState::new(&self.config);

        // Populate it with the loaded data
        new_tab.original_structure = Some(structure.clone());
        new_tab.structure = Some(structure);
        new_tab.file_name = filename;

        // Add to list
        self.tabs.push(new_tab);

        // Switch to the new tab
        self.active_tab_index = self.tabs.len() - 1;
    }

    // --- Helpers (Proxied to Active Tab) ---

    pub fn push_undo(&mut self) {
        let tab = self.active_tab_mut();
        if let Some(s) = &tab.structure {
            if tab.interaction.undo_stack.len() >= 20 {
                tab.interaction.undo_stack.remove(0);
            }
            tab.interaction.undo_stack.push(s.clone());
        }
    }

    pub fn undo(&mut self) -> String {
        let tab = self.active_tab_mut();
        if let Some(prev_struct) = tab.interaction.undo_stack.pop() {
            tab.structure = Some(prev_struct);
            tab.interaction.selected_indices.clear();
            "Undo successful.".to_string()
        } else {
            "Nothing to undo.".to_string()
        }
    }

    pub fn delete_selected(&mut self) -> String {
        let tab = self.active_tab_mut();

        if tab.interaction.selected_indices.is_empty() {
            return "No atoms selected.".to_string();
        }

        // Push undo logic (duplicated locally to avoid double borrow)
        if let Some(s) = &tab.structure {
            if tab.interaction.undo_stack.len() >= 20 {
                tab.interaction.undo_stack.remove(0);
            }
            tab.interaction.undo_stack.push(s.clone());
        }

        if let Some(s) = &mut tab.structure {
            let initial = s.atoms.len();
            let mut new_atoms = Vec::new();
            for (i, atom) in s.atoms.drain(..).enumerate() {
                if !tab.interaction.selected_indices.contains(&i) {
                    new_atoms.push(atom);
                }
            }
            s.atoms = new_atoms;
            tab.interaction.selected_indices.clear();
            format!("Deleted {} atoms.", initial - s.atoms.len())
        } else {
            "No structure loaded.".to_string()
        }
    }

    pub fn toggle_selection(&mut self, index: usize) {
        let tab = self.active_tab_mut();
        if tab.interaction.selected_indices.contains(&index) {
            tab.interaction.selected_indices.remove(&index);
        } else {
            tab.interaction.selected_indices.insert(index);
        }
    }
}
