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

/// Opens the main Analysis Tools window: Symmetry, XRD, Band Path, Voids, Slab.
pub fn show_analysis_window(parent: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    let window = Window::builder()
        .title("Analysis Tools")
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

    let voids_page = voids_tab::build(state.clone());
    notebook.append_page(&voids_page, Some(&Label::new(Some("Void Analysis"))));

    let slab_page = slab_tab::build(state.clone());
    notebook.append_page(&slab_page, Some(&Label::new(Some("Slab"))));

    window.set_child(Some(&notebook));
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
