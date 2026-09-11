pub mod charge_density_tab;
pub mod kpath_tab;
pub mod slab_tab;
pub mod symmetry_tab;
pub mod voids_tab;
pub mod window;
pub mod xrd_tab;

/// Width of the control column on every analysis tab that pairs a viewport
/// with controls (XRD, Band Path, Void Analysis, Slab). The window opens at
/// 950 px and each tab keeps 15 px margins plus 15 px between the panes, so
/// 280 px here leaves roughly a 70:30 split. Fixed rather than proportional
/// on purpose: width gained by resizing the window belongs to the viewport,
/// not to a column of spin buttons.
pub const CONTROL_PANE_WIDTH: i32 = 280;
