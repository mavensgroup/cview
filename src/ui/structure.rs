pub mod basis_tab;
pub mod instances_tab;
pub mod preview;
pub mod slab_tab;
pub mod supercell_tab;
pub mod window;

/// Width of the control column on every tab of the Structure window. Wider
/// than the Analysis column (280 px): the Slab h/k/l row and the Atom
/// Instances list need ~400 px, and one width for all four tabs keeps the
/// preview from resizing as you switch.
pub const CONTROL_PANE_WIDTH: i32 = 400;

/// One tab of the Structure window: its controls, plus a hook that runs each
/// time the tab is shown (refresh anything derived from the active structure).
pub struct TabParts {
    pub controls: gtk4::Box,
    pub on_enter: Box<dyn Fn()>,
}
