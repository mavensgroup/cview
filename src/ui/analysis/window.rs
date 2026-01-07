// src/ui/analysis/window.rs
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Notebook, Window, Label};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::AppState;
use super::xrd_tab;
use super::symmetry_tab; // <--- Import the new tab

pub fn show_analysis_window(parent: &ApplicationWindow, state: Rc<RefCell<AppState>>) {
    let window = Window::builder()
        .title("Analysis Tools")
        .transient_for(parent)
        .default_width(900)
        .default_height(600)
        .modal(false)
        .build();

    let notebook = Notebook::new();

    // --- Tab 1: Symmetry & Lattice (First Page) ---
    let sym_page = symmetry_tab::build(state.clone());
    let sym_label = Label::new(Some("Symmetry"));
    notebook.append_page(&sym_page, Some(&sym_label));

    // --- Tab 2: XRD Simulation (Second Page) ---
    let xrd_page = xrd_tab::build(state.clone());
    let xrd_label = Label::new(Some("XRD"));
    notebook.append_page(&xrd_page, Some(&xrd_label));

    window.set_child(Some(&notebook));
    window.present();
}
