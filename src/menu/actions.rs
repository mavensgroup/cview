use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, FileChooserAction, FileChooserNative, ResponseType, gio};
use std::rc::Rc;
use std::cell::RefCell;

// Note: These imports work because we made modules 'pub' in main.rs
use crate::state::{AppState, RotationCenter, ExportFormat};
use crate::io;
use crate::rendering::export_image;
use crate::preferences;

pub fn setup_actions(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
) {
    let da_clone = drawing_area.clone();
    let queue_draw = move || da_clone.queue_draw();

    // --- 1. FILE ACTIONS ---

    // Open
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
        let s = state_c.clone();
        let q = q_c.clone();
        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        if let Some(path_str) = path.to_str() {
                            match io::load_structure(path_str) {
                                Ok(structure) => {
                                    s.borrow_mut().structure = Some(structure);
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

    // Export
    let action_export = gio::SimpleAction::new("export", None);
    let win_c = window.clone();
    let state_c = state.clone();
    action_export.connect_activate(move |_, _| {
        let dialog = FileChooserNative::new(
            Some("Export Image"),
            Some(&win_c),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );
        dialog.set_modal(true);
        dialog.set_current_name("structure_export.png");
        let s = state_c.clone();
        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        if let Some(path_str) = path.to_str() {
                            let is_pdf = path_str.to_lowercase().ends_with(".pdf");
                            let _ = export_image(&s.borrow(), path_str, 2048, 2048, is_pdf);
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

    // Close
    let action_close = gio::SimpleAction::new("close", None);
    let win_c = window.clone();
    action_close.connect_activate(move |_, _| {
        win_c.close();
    });
    window.add_action(&action_close);
    app.set_accels_for_action("win.close", &["<Ctrl>q", "<Ctrl>w"]);

    // Preferences
    let action_prefs = gio::SimpleAction::new("preferences", None);
    let win_c = window.clone();
    let state_c = state.clone();
    let da_c = drawing_area.clone();
    action_prefs.connect_activate(move |_, _| {
        preferences::show_preferences_window(&win_c, state_c.clone(), da_c.clone());
    });
    window.add_action(&action_prefs);


    // --- 2. VIEW ACTIONS ---

    // Reset
    let action_reset = gio::SimpleAction::new("reset_view", None);
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_reset.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.rot_x = 0.0;
        st.rot_y = 0.0;
        st.zoom = 1.0;
        q_c();
    });
    window.add_action(&action_reset);
    app.set_accels_for_action("win.reset_view", &["r", "R"]);

    // Toggle Center
    let action_center = gio::SimpleAction::new("toggle_center", None);
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_center.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.rotation_mode = match st.rotation_mode {
            RotationCenter::Centroid => RotationCenter::UnitCell,
            RotationCenter::UnitCell => RotationCenter::Centroid,
        };
        println!("Rotation Center: {:?}", st.rotation_mode);
        q_c();
    });
    window.add_action(&action_center);
    app.set_accels_for_action("win.toggle_center", &["c", "C"]);

    // Toggle Format
    let action_format = gio::SimpleAction::new("toggle_format", None);
    let state_c = state.clone();
    action_format.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.default_export_format = match st.default_export_format {
            ExportFormat::Png => ExportFormat::Pdf,
            ExportFormat::Pdf => ExportFormat::Png,
        };
        println!("Export Format: {:?}", st.default_export_format);
    });
    window.add_action(&action_format);
    app.set_accels_for_action("win.toggle_format", &["f", "F"]);

    // Align X
    let action_view_x = gio::SimpleAction::new("view_x", None);
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_view_x.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.rot_x = 0.0;
        st.rot_y = std::f64::consts::PI / 2.0;
        q_c();
    });
    window.add_action(&action_view_x);

    // Align Y
    let action_view_y = gio::SimpleAction::new("view_y", None);
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_view_y.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.rot_x = std::f64::consts::PI / 2.0;
        st.rot_y = 0.0;
        q_c();
    });
    window.add_action(&action_view_y);

    // Align Z
    let action_view_z = gio::SimpleAction::new("view_z", None);
    let state_c = state.clone();
    let q_c = queue_draw.clone();
    action_view_z.connect_activate(move |_, _| {
        let mut st = state_c.borrow_mut();
        st.rot_x = 0.0;
        st.rot_y = 0.0;
        q_c();
    });
    window.add_action(&action_view_z);
}
