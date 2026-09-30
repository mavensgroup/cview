// src/ui/analysis/window.rs
use super::charge_density_tab;
use super::kpath_tab;
use super::slab_tab;
use super::symmetry_tab;
use super::voids_tab;
use super::xrd_tab;
use crate::state::AppState;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Label, Notebook, Window};
use std::cell::RefCell;
use std::rc::Rc;

/// Tabs of the Analysis window, in notebook order.
#[derive(Clone, Copy)]
pub enum AnalysisTab {
    Symmetry = 0,
    Xrd = 1,
    BandPath = 2,
    Voids = 3,
}

/// Opens the Analysis window (Symmetry, XRD, Band Path, Voids) on `start`.
/// These are read-only computations; tools that change the structure live in
/// the Structure menu as their own windows.
pub fn show_analysis_window(
    parent: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    main_notebook: &Notebook,
    start: AnalysisTab,
) {
    let window = Window::builder()
        .title("Analysis")
        .transient_for(parent)
        .default_width(950)
        .default_height(650)
        .modal(false)
        .build();

    let notebook = Notebook::new();

    let sym_page = symmetry_tab::build(state.clone());
    notebook.append_page(&sym_page, Some(&Label::new(Some("Symmetry"))));

    let xrd_page = xrd_tab::build(state.clone());
    notebook.append_page(&xrd_page, Some(&Label::new(Some("XRD"))));

    let kpath_page = kpath_tab::build(state.clone());
    notebook.append_page(&kpath_page, Some(&Label::new(Some("Band Path"))));

    let voids_page = voids_tab::build(state.clone(), main_notebook);
    notebook.append_page(&voids_page, Some(&Label::new(Some("Void Analysis"))));

    window.set_child(Some(&notebook));
    notebook.set_current_page(Some(start as u32));
    window.present();
}

/// Opens the Slab generator in its own non-modal window: it has a live
/// preview and edits the structure, so it stays beside the main view.
pub fn show_slab_window(parent: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    let window = Window::builder()
        .title("Slab Generator")
        .transient_for(parent)
        .default_width(960)
        .default_height(520)
        .modal(false)
        .build();

    let slab_page = slab_tab::build(state);
    window.set_child(Some(&slab_page));
    window.present();
}

/// Opens a standalone Charge Density window (CHGCAR only, no notebook).
/// Accepts AppState so export settings (font sizes, colormap) are read from config.
pub fn show_charge_density_window(parent: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    let window = Window::builder()
        .title("Charge Density Visualization")
        .transient_for(parent)
        .default_width(1000)
        .default_height(700)
        .modal(false)
        .build();

    let cd_page = charge_density_tab::build(Some(state));
    window.set_child(Some(&cd_page));
    window.present();
}
