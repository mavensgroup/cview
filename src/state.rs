// src/state.rs
// Updated to use Config defaults

use crate::config::{Config, RenderStyle};
use crate::model::miller::MillerPlane;
use crate::model::structure::Structure;
use crate::physics::analysis::{kpath::KPathResult, voids::VoidResult};
use std::collections::HashSet;

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
    pub show_full_unit_cell: bool,
}

impl ViewState {
    pub fn from_config(config: &Config) -> Self {
        Self {
            rot_x: 0.0,
            rot_y: 0.0,
            rot_z: 0.0,
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
            bond_cutoff: 1.15,
            scale: 30.0,
            show_full_unit_cell: true,
        }
    }
}

#[derive(Default)]
pub struct InteractionState {
    pub selected_indices: HashSet<usize>,
    pub undo_stack: Vec<Structure>,
    pub is_shift_pressed: bool,
    pub selection_box: Option<((f64, f64), (f64, f64))>,
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
        }
    }

    pub fn invalidate_bvs_cache(&mut self) {
        self.bvs_cache_valid = false;
    }

    pub fn get_bvs_values(&mut self) -> &[f64] {
        if !self.bvs_cache_valid {
            if let Some(ref structure) = self.structure {
                use crate::physics::bond_valence::calculator::calculate_bvs_all_pbc;
                self.bvs_cache = calculate_bvs_all_pbc(structure);
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
        println!("{}", msg);
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

    pub fn toggle_selection(&mut self, atom_index: usize) {
        let tab = self.active_tab_mut();
        if tab.interaction.selected_indices.contains(&atom_index) {
            tab.interaction.selected_indices.remove(&atom_index);
        } else {
            tab.interaction.selected_indices.insert(atom_index);
        }
    }

    pub fn delete_selected(&mut self) -> String {
        let tab = self.active_tab_mut();
        if tab.interaction.selected_indices.is_empty() {
            return "No atoms selected.".to_string();
        }
        if let Some(ref mut structure) = tab.structure {
            tab.interaction.undo_stack.push(structure.clone());
            let mut indices: Vec<usize> =
                tab.interaction.selected_indices.iter().copied().collect();
            indices.sort_by(|a, b| b.cmp(a));
            for &idx in &indices {
                if idx < structure.atoms.len() {
                    structure.atoms.remove(idx);
                }
            }
            tab.interaction.selected_indices.clear();
            tab.invalidate_bvs_cache();
            format!("Deleted {} atom(s)", indices.len())
        } else {
            "No structure loaded.".to_string()
        }
    }

    pub fn undo(&mut self) -> String {
        let tab = self.active_tab_mut();
        if let Some(prev_structure) = tab.interaction.undo_stack.pop() {
            tab.structure = Some(prev_structure);
            tab.interaction.selected_indices.clear();
            tab.invalidate_bvs_cache();
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
