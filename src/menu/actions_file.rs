use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, DrawingArea, FileChooserAction, FileChooserNative,
    ResponseType, FileFilter, gio
};
use std::rc::Rc;
use std::cell::RefCell;

use crate::state::{AppState, ExportFormat};
use crate::io;
use crate::rendering::export_image;
use crate::preferences;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    let da_clone = drawing_area.clone();
    let queue_draw = move || da_clone.queue_draw();

    // --- OPEN ---
    let action_open = gio::SimpleAction::new("open", None);
    let win_c = window.clone();
    let state_c = state.clone();
    let q_c = queue_draw.clone();

    action_open.connect_activate(move |_, _| {
        let dialog = FileChooserNative::new(
            Some("Open Structure"),
            Some(&win_c),
            FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );
        dialog.set_modal(true);

        let filter = FileFilter::new();
        filter.set_name(Some("Structure Files"));
        for pat in &["*.xyz", "*.XYZ", "*.cif", "*.CIF", "*.vasp", "*.VASP",
                     "*POSCAR*", "*poscar*", "*CONTCAR*", "*contcar*", "*.pot", "*.inp"] {
            filter.add_pattern(pat);
        }
        dialog.add_filter(&filter);

        let s = state_c.clone();
        let q = q_c.clone();
        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        if let Some(path_str) = path.to_str() {
                            match io::load_structure(path_str) {
                                Ok(structure) => {
                                    let mut st = s.borrow_mut();
                                    st.structure = Some(structure);
                                    // Just save the name so we can suggest it later
                                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                        st.file_name = stem.to_string();
                                    }
                                    q();
                                }
                                Err(e) => eprintln!("Error loading: {}", e),
                            }
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    window.add_action(&action_open);
    app.set_accels_for_action("win.open", &["<Ctrl>o"]);

    // --- SAVE AS (Minimal) ---
    let action_save_as = gio::SimpleAction::new("save_as", None);
    let win_c = window.clone();
    let state_c = state.clone();

    action_save_as.connect_activate(move |_, _| {
        let dialog = FileChooserNative::new(
            Some("Save Structure As"),
            Some(&win_c),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );
        dialog.set_modal(true);

        // Filters (Just for convenience)
        let filter_cif = FileFilter::new();
        filter_cif.set_name(Some("CIF File (*.cif)"));
        filter_cif.add_pattern("*.cif");

        let filter_vasp = FileFilter::new();
        filter_vasp.set_name(Some("VASP POSCAR (*.vasp)"));
        filter_vasp.add_pattern("*.vasp");

        let filter_spr = FileFilter::new();
        filter_spr.set_name(Some("SPR-KKR Potential (*.pot)"));
        filter_spr.add_pattern("*.pot");

        dialog.add_filter(&filter_cif);
        dialog.add_filter(&filter_vasp);
        dialog.add_filter(&filter_spr);

        // Simple Default: "Filename.cif"
        let basename = state_c.borrow().file_name.clone();
        dialog.set_current_name(&format!("{}.cif", basename));

        let s = state_c.clone();
        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        if let Some(path_str) = path.to_str() {
                            let st = s.borrow();
                            if let Some(structure) = &st.structure {
                                if let Err(e) = io::save_structure(path_str, structure) {
                                    eprintln!("Error saving structure: {}", e);
                                } else {
                                    println!("Saved structure to {}", path_str);
                                }
                            }
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    window.add_action(&action_save_as);
    app.set_accels_for_action("win.save_as", &["<Ctrl><Shift>s"]);

    // --- EXPORT IMAGE (Minimal) ---
    let action_export = gio::SimpleAction::new("export", None);
    let win_c = window.clone();
    let state_c = state.clone();

    action_export.connect_activate(move |_, _| {
        let dialog = FileChooserNative::new(
            Some("Export Screenshot"),
            Some(&win_c),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );
        dialog.set_modal(true);

        let filter_png = FileFilter::new();
        filter_png.set_name(Some("PNG Image (*.png)"));
        filter_png.add_pattern("*.png");

        let filter_pdf = FileFilter::new();
        filter_pdf.set_name(Some("PDF Document (*.pdf)"));
        filter_pdf.add_pattern("*.pdf");

        dialog.add_filter(&filter_png);
        dialog.add_filter(&filter_pdf);

        let is_pdf = matches!(state_c.borrow().default_export_format, ExportFormat::Pdf);
        let ext = if is_pdf { "pdf" } else { "png" };
        dialog.set_current_name(&format!("screenshot.{}", ext));

        let s = state_c.clone();
        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        if let Some(path_str) = path.to_str() {
                            let pdf_mode = path_str.to_lowercase().ends_with(".pdf");
                            let _ = export_image(&s.borrow(), path_str, 2048.0, 2048.0, pdf_mode);
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    window.add_action(&action_export);
    app.set_accels_for_action("win.export", &["<Ctrl>e"]);

    // --- CLOSE ---
    let action_close = gio::SimpleAction::new("close", None);
    let win_c = window.clone();
    action_close.connect_activate(move |_, _| { win_c.close(); });
    window.add_action(&action_close);
    app.set_accels_for_action("win.close", &["<Ctrl>q", "<Ctrl>w"]);

    // --- PREFERENCES ---
    let action_prefs = gio::SimpleAction::new("preferences", None);
    let win_c = window.clone();
    let state_c = state.clone();
    let da_c = drawing_area.clone();
    action_prefs.connect_activate(move |_, _| {
        preferences::show_preferences_window(&win_c, state_c.clone(), da_c.clone());
    });
    window.add_action(&action_prefs);
}
