// src/state.rs
// Updated to use Config defaults

use crate::config::{Config, RenderStyle};
use crate::model::miller::MillerPlane;
use crate::model::structure::Structure;
use crate::physics::analysis::{
    interstitial::InterstitialSite, kpath::KPathResult, voids::VoidResult,
};
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
    /// Whether the interstitial site overlay is drawn. Defaults on, so a
    /// freshly computed screen is visible without a second action.
    pub show_interstitial_sites: bool,
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
            show_interstitial_sites: true,
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
            show_interstitial_sites: true,
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
    /// Pick order, stamped by `InteractionState::select`. Measurements read
    /// atoms in this order, so the second atom clicked is the angle vertex
    /// whatever its index happens to be.
    pub seq: u64,
}

/// Per-atom render override. Purely cosmetic — never written to any IO format.
/// Keyed by `Atom` index in `Structure.atoms`. Lives only in the session;
/// reload from file resets it. Use the Structure → Atom Instances dialog to edit.
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
    pub is_shift_pressed: bool,
    pub selection_box: Option<((f64, f64), (f64, f64))>,
    /// Cumulative drag offset reported on the previous drag-update event.
    /// Reset to (0, 0) on drag-begin. Used to derive per-frame deltas for
    /// trackball rotation, since GTK's GestureDrag reports cumulative offset.
    pub drag_prev_offset: (f64, f64),
    /// Counter behind `SelectedAtom::seq`. Only ever increases, so the order
    /// survives deselecting an atom in the middle of a pick sequence.
    pub selection_seq: u64,
}

impl InteractionState {
    /// Add `atom` to the selection, stamped with the next pick number.
    /// Returns false (and leaves the existing entry alone) if that instance
    /// is already selected.
    pub fn select(&mut self, mut atom: SelectedAtom) -> bool {
        if self.selected.contains_key(&atom.unique_id) {
            return false;
        }
        self.selection_seq += 1;
        atom.seq = self.selection_seq;
        self.selected.insert(atom.unique_id, atom);
        true
    }

    /// Selected atoms in the order they were picked.
    pub fn selected_in_order(&self) -> Vec<&SelectedAtom> {
        let mut v: Vec<&SelectedAtom> = self.selected.values().collect();
        v.sort_by_key(|a| a.seq);
        v
    }
}

/// How the atom list after an edit relates to the one before it. Decides
/// which per-atom state (the selection, cosmetic overrides) still applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Same atoms at the same places; only species or occupancy changed.
    /// The selection and the overrides still point at the right atoms.
    Species,
    /// Existing atoms keep their indices, but positions or the cell changed,
    /// or atoms were appended at the end. Overrides still apply; the
    /// selection does not (it is keyed by rendered instance, which moves).
    Geometry,
    /// Atoms removed, inserted or reordered, or a new cell altogether.
    /// Nothing keyed by atom index survives.
    Reindexed,
}

/// The structure as it was before one edit.
#[derive(Clone)]
pub struct Snapshot {
    pub structure: Structure,
    /// What the edit was called ("Supercell", "Deleted 2 atom(s)").
    pub op: String,
    pub edit: Edit,
}

/// Undo and redo for one tab's structure. Every edit is recorded through
/// `TabState::commit`; the inverse of an edit has the same `Edit` kind, so a
/// snapshot carries it both ways.
#[derive(Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl History {
    /// Steps kept. A snapshot is a whole structure, and large MD cells are
    /// megabytes each.
    pub const LIMIT: usize = 50;

    fn record(&mut self, snap: Snapshot) {
        self.redo.clear();
        self.undo.push(snap);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Interstitial sites found for one candidate ion, ready to draw.
///
/// Held per tab rather than globally: two tabs can be screening different
/// ions in different structures at once, and the overlay has to follow the
/// structure it was computed from.
#[derive(Clone, Debug)]
pub struct InterstitialOverlay {
    /// Display label of the ion screened, e.g. "Li\u{207a}".
    pub ion: String,
    /// Shannon radius used as the fit threshold, in Å.
    pub ion_radius: f64,
    pub sites: Vec<InterstitialSite>,
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
    /// Interstitial site overlay, when one has been computed for this tab.
    pub interstitial: Option<InterstitialOverlay>,
    pub bvs_cache: Vec<f64>,
    pub bvs_cache_valid: bool,
    /// Per-atom cosmetic overrides keyed by index into `structure.atoms`.
    /// Indices that aren't present here render with element defaults.
    pub overrides: HashMap<usize, AtomOverride>,
    /// Covalent-equivalent radius per species the element table does not
    /// know (LAMMPS `Type n`); everything else uses the table.
    pub species_radius: HashMap<String, f64>,
    /// All frames of the file this tab was opened from, when it had more
    /// than one (relaxation, MD). Played by the Trajectory Player window.
    pub trajectory: Option<std::rc::Rc<crate::model::trajectory::Trajectory>>,
    /// Frame of `trajectory` the structure was taken from.
    pub trajectory_frame: usize,
    /// Bond rules the user changed, by element pair. Every other pair follows
    /// the defaults (`model::bonds::default_rules`) for the elements present,
    /// so substituting an element brings in its default rules.
    pub bond_overrides: HashMap<(String, String), crate::model::bonds::BondRule>,
    /// Undo/redo of structure edits. Written only by `commit`, `undo`, `redo`.
    pub history: History,
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
            interstitial: None,
            bvs_cache: Vec::new(),
            bvs_cache_valid: false,
            overrides: HashMap::new(),
            species_radius: HashMap::new(),
            trajectory: None,
            trajectory_frame: 0,
            bond_overrides: HashMap::new(),
            history: History::default(),
        }
    }

    /// Replace the structure with `new` as one undoable step called `op`.
    ///
    /// Every operation that changes the structure goes through here: it keeps
    /// the old structure for Undo, drops the Redo history, clears whatever
    /// per-atom state `edit` says no longer applies, drops derived analysis,
    /// and records the change in the console.
    pub fn commit(&mut self, op: &str, new: Structure, edit: Edit) {
        if let Some(old) = self.structure.take() {
            self.history.record(Snapshot { structure: old, op: op.to_string(), edit });
        }
        crate::utils::console::structure_changed(op, &new);
        self.structure = Some(new);
        self.after_replace(edit);
    }

    /// Step back one edit. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.history.undo.pop() else { return false };
        let Snapshot { structure, op, edit } = snap;
        if let Some(cur) = self.structure.replace(structure) {
            self.history.redo.push(Snapshot { structure: cur, op: op.clone(), edit });
        }
        if let Some(s) = &self.structure {
            crate::utils::console::structure_changed(&format!("Undo: {}", op), s);
        }
        self.after_replace(edit);
        true
    }

    /// Re-apply the last undone edit. Returns false when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.history.redo.pop() else { return false };
        let Snapshot { structure, op, edit } = snap;
        if let Some(cur) = self.structure.replace(structure) {
            // Not `record`: that would clear the rest of the redo stack.
            self.history.undo.push(Snapshot { structure: cur, op: op.clone(), edit });
        }
        if let Some(s) = &self.structure {
            crate::utils::console::structure_changed(&format!("Redo: {}", op), s);
        }
        self.after_replace(edit);
        true
    }

    fn after_replace(&mut self, edit: Edit) {
        if edit != Edit::Species {
            self.interaction.selected.clear();
        }
        match edit {
            Edit::Reindexed => self.overrides.clear(),
            _ => {
                // Undoing an append leaves overrides on atoms that are gone.
                let n = self.structure.as_ref().map_or(0, |s| s.atoms.len());
                self.overrides.retain(|&i, _| i < n);
            }
        }
        self.invalidate_derived();
    }

    /// Tolerance on covalent radii for the pairs that use them.
    pub fn bond_tolerance(&self) -> f64 {
        if (0.1..=2.0).contains(&self.view.bond_cutoff) {
            self.view.bond_cutoff
        } else {
            1.15
        }
    }

    /// The bond rule of every element pair in the structure: the user's
    /// where they changed one, the default otherwise.
    pub fn bond_rules(&self) -> Vec<crate::model::bonds::BondRule> {
        use crate::model::bonds::{default_rules, pair_key};
        let Some(s) = &self.structure else { return Vec::new() };
        let mut seen: std::collections::HashSet<&str> = Default::default();
        let elements: Vec<String> = s
            .atoms
            .iter()
            .filter(|a| seen.insert(a.element.as_str()))
            .map(|a| a.element.clone())
            .collect();
        let mut rules = default_rules(&elements, self.bond_tolerance());
        for r in &mut rules {
            if let Some(o) = self.bond_overrides.get(&pair_key(&r.a, &r.b)) {
                *r = o.clone();
            }
        }
        rules
    }

    /// Lookup table of the enabled bond rules, for bond searches.
    pub fn bond_table(&self) -> crate::model::bonds::BondTable {
        crate::model::bonds::BondTable::from_rules(&self.bond_rules())
    }

    /// Base (covalent-equivalent) radius of a species, before atom scale and
    /// per-atom overrides.
    pub fn base_radius(&self, element: &str) -> f64 {
        self.species_radius
            .get(element)
            .copied()
            .unwrap_or_else(|| crate::model::elements::get_covalent_radius(element))
    }

    /// Give generic `Type n` species (files with no element names) distinct
    /// colours and a diameter of about one length unit, and drop bonds, which
    /// the element table cannot judge for them.
    pub fn style_generic_species(&mut self) {
        const PALETTE: [(f64, f64, f64); 8] = [
            (0.93, 0.36, 0.34), (0.36, 0.52, 0.93), (0.95, 0.78, 0.25), (0.40, 0.78, 0.50),
            (0.70, 0.45, 0.85), (0.95, 0.60, 0.30), (0.40, 0.80, 0.85), (0.85, 0.50, 0.65),
        ];
        let Some(s) = &self.structure else { return };
        let mut any = false;
        for a in &s.atoms {
            let Some(n) = a.element.strip_prefix("Type ").and_then(|n| n.parse::<usize>().ok()) else {
                continue;
            };
            any = true;
            self.style
                .element_colors
                .entry(a.element.clone())
                .or_insert(PALETTE[n.max(1).saturating_sub(1) % PALETTE.len()]);
            self.species_radius.entry(a.element.clone()).or_insert(1.25);
        }
        if any {
            self.view.show_bonds = false;
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

    /// Drop everything derived from the current structure.
    ///
    /// Call after ANY mutation — delete, undo, supercell, cell conversion,
    /// element substitution. Cached analysis outlives the structure it was
    /// computed from otherwise, and a stale interstitial overlay is worse
    /// than none: it draws sites that look authoritative on a framework that
    /// no longer has them.
    pub fn invalidate_derived(&mut self) {
        self.invalidate_bvs_cache();
        self.kpath_result = None;
        self.void_result = None;
        self.interstitial = None;
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
    /// Files being read on worker threads. Dropping a handle cancels its job,
    /// so they are held for the session (a handle is one flag; finished
    /// ones cost nothing).
    pub load_jobs: Vec<crate::utils::task::JobHandle>,
    pub tabs: Vec<TabState>,
    pub active_tab_index: usize,
    pub config: Config,
}

impl AppState {
    pub fn new() -> Self {
        let (config, msg) = Config::load();
        crate::utils::console::log_info(&msg);
        Self {
            load_jobs: vec![],
            tabs: vec![],
            active_tab_index: 0,
            config,
        }
    }

    pub fn new_with_log() -> (Self, String) {
        let (config, log) = Config::load();
        let initial_tab = TabState::new(&config);
        let state = Self {
            load_jobs: vec![],
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
        new_tab.style_generic_species();
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
            tab.interaction.select(atom);
        }
    }

    /// Delete the selected atoms (every rendered copy of them) as one
    /// undoable step.
    pub fn delete_selected(&mut self) {
        let tab = self.active_tab_mut();
        if tab.interaction.selected.is_empty() {
            crate::utils::console::log_info("No atoms selected.");
            return;
        }
        let Some(structure) = &tab.structure else {
            crate::utils::console::log_info("No structure loaded.");
            return;
        };
        // Ghost copies share an original_index: delete each atom once.
        let mut indices: Vec<usize> =
            tab.interaction.selected.values().map(|s| s.original_index).collect();
        indices.sort_unstable();
        indices.dedup();
        let new = crate::physics::operations::basis::remove_selection(structure, &indices);
        let op = format!("Deleted {} atom(s)", indices.len());
        tab.commit(&op, new, Edit::Reindexed);
    }

    /// Select the in-cell instance of each atom in `indices`, in that pick
    /// order, replacing the selection. Used after an edit that clears the
    /// selection (add, move) so the edited atoms stay picked.
    pub fn select_atoms(&mut self, indices: &[usize]) {
        // Instance ids depend only on the structure, not the canvas size.
        let (atoms, _, _) = crate::rendering::scene::calculate_scene(
            self.active_tab(),
            &self.config,
            1.0,
            1.0,
            false,
            None,
            None,
        );
        let interaction = &mut self.active_tab_mut().interaction;
        interaction.selected.clear();
        for &i in indices {
            if let Some(a) = atoms.iter().find(|a| a.original_index == i && !a.is_ghost) {
                interaction.select(SelectedAtom {
                    unique_id: a.unique_id,
                    original_index: i,
                    cart_pos: a.cart_pos,
                    element: a.element.clone(),
                    seq: 0,
                });
            }
        }
    }

    pub fn undo(&mut self) {
        if !self.active_tab_mut().undo() {
            crate::utils::console::log_info("Nothing to undo.");
        }
    }

    pub fn redo(&mut self) {
        if !self.active_tab_mut().redo() {
            crate::utils::console::log_info("Nothing to redo.");
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::structure::Atom;

    fn chain(elements: &[&str]) -> Structure {
        let atoms = elements
            .iter()
            .enumerate()
            .map(|(i, el)| Atom {
                element: el.to_string(),
                position: [i as f64, 0.0, 0.0],
                original_index: i,
                oxidation: None,
                occupancy: 1.0,
            })
            .collect();
        Structure {
            lattice: [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            atoms,
            formula: String::new(),
            is_periodic: true,
        }
    }

    fn elements(tab: &TabState) -> Vec<String> {
        tab.structure.as_ref().unwrap().atoms.iter().map(|a| a.element.clone()).collect()
    }

    fn tab_with(s: Structure) -> TabState {
        let mut tab = TabState::new(&Config::default());
        tab.structure = Some(s);
        tab
    }

    #[test]
    fn undo_redo_walk_the_history() {
        let mut tab = tab_with(chain(&["Fe", "O"]));
        tab.commit("a", chain(&["Co", "O"]), Edit::Species);
        tab.commit("b", chain(&["Co"]), Edit::Reindexed);
        assert!(tab.undo());
        assert_eq!(elements(&tab), ["Co", "O"]);
        assert!(tab.undo());
        assert_eq!(elements(&tab), ["Fe", "O"]);
        assert!(!tab.undo());
        assert!(tab.redo());
        assert!(tab.redo());
        assert_eq!(elements(&tab), ["Co"]);
        assert!(!tab.redo());
    }

    #[test]
    fn a_new_edit_drops_the_redo_history() {
        let mut tab = tab_with(chain(&["Fe"]));
        tab.commit("a", chain(&["Co"]), Edit::Species);
        tab.undo();
        tab.commit("b", chain(&["Ni"]), Edit::Species);
        assert!(!tab.redo());
        tab.undo();
        assert_eq!(elements(&tab), ["Fe"]);
    }

    #[test]
    fn history_is_bounded() {
        let mut tab = tab_with(chain(&["Fe"]));
        for _ in 0..History::LIMIT + 10 {
            tab.commit("x", chain(&["Fe"]), Edit::Species);
        }
        let mut n = 0;
        while tab.undo() {
            n += 1;
        }
        assert_eq!(n, History::LIMIT);
    }

    #[test]
    fn edit_kind_decides_what_per_atom_state_survives() {
        let sel = SelectedAtom {
            unique_id: 0,
            original_index: 0,
            cart_pos: [0.0; 3],
            element: "Fe".into(),
            seq: 0,
        };
        let mut tab = tab_with(chain(&["Fe", "O"]));
        tab.interaction.select(sel.clone());
        tab.overrides.insert(1, AtomOverride { color: Some((1.0, 0.0, 0.0)), ..Default::default() });

        tab.commit("species", chain(&["Co", "O"]), Edit::Species);
        assert_eq!(tab.interaction.selected.len(), 1);
        assert!(tab.overrides.contains_key(&1));

        // Appending keeps overrides; undoing the append prunes the ones
        // past the end.
        tab.commit("append", chain(&["Co", "O", "H"]), Edit::Geometry);
        assert!(tab.interaction.selected.is_empty());
        tab.overrides.insert(2, AtomOverride::default());
        tab.undo();
        assert!(tab.overrides.contains_key(&1));
        assert!(!tab.overrides.contains_key(&2));

        tab.commit("delete", chain(&["Co"]), Edit::Reindexed);
        assert!(tab.overrides.is_empty());
    }

    #[test]
    fn delete_selected_is_undoable_and_updates_the_formula() {
        let mut app = AppState {
            load_jobs: vec![],
            tabs: vec![tab_with(chain(&["Fe", "O", "O"]))],
            active_tab_index: 0,
            config: Config::default(),
        };
        for (uid, idx) in [(0, 1), (7, 1)] {
            // Two rendered copies of the same atom delete it once.
            app.active_tab_mut().interaction.select(SelectedAtom {
                unique_id: uid,
                original_index: idx,
                cart_pos: [0.0; 3],
                element: "O".into(),
                seq: 0,
            });
        }
        app.delete_selected();
        assert_eq!(elements(app.active_tab()), ["Fe", "O"]);
        assert_eq!(app.active_tab().structure.as_ref().unwrap().formula, "FeO");
        app.undo();
        assert_eq!(elements(app.active_tab()), ["Fe", "O", "O"]);
    }
}
