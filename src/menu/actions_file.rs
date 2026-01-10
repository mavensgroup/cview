use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, DrawingArea, FileChooserNative, FileChooserAction, ResponseType, TextView, FileFilter};
use std::rc::Rc;
use std::cell::RefCell;
use crate::state::{AppState, ExportFormat};
use crate::io;
use crate::rendering::export_image;
// use crate::ui::preferences::show_preferences_window;

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    drawing_area: &DrawingArea,
    console_view: &TextView,
    atom_list_box: &gtk4::Box, // <--- Argument for Sidebar update
) {

    // --- OPEN ACTION ---
    let open_action = gtk4::gio::SimpleAction::new("open", None);
    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();
    let console_weak = console_view.downgrade();

    // 1. Create weak reference to the atom container (Sidebar)
    let atom_box_weak = atom_list_box.downgrade();

    open_action.connect_activate(move |_, _| {
        let win = match win_weak.upgrade() { Some(w) => w, None => return };

        let dialog = FileChooserNative::new(
            Some("Open Structure File"),
            Some(&win),
            FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );

        let filter_any = FileFilter::new();
        filter_any.set_name(Some("All Supported Files"));
        filter_any.add_pattern("*.cif");
        filter_any.add_pattern("POSCAR*");
        filter_any.add_pattern("CONTCAR*");
        filter_any.add_pattern("*.vasp");
        filter_any.add_pattern("*.pot");
        filter_any.add_pattern("*.sys");
        filter_any.add_pattern("*.out");
        filter_any.add_pattern("*.in");
        filter_any.add_pattern("*.pwi");
        filter_any.add_pattern("*.pwo");
        filter_any.add_pattern("*.qe");
        filter_any.add_pattern("*.xyz");
        dialog.add_filter(&filter_any);

        let state_weak_inner = state_weak.clone();
        let da_weak_inner = da_weak.clone();
        let console_weak_inner = console_weak.clone();

        // 2. Clone weak ref for the inner closure
        let atom_box_inner = atom_box_weak.clone();

        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();

                        // We need the state to exist
                        if let Some(st) = state_weak_inner.upgrade() {
                            match crate::io::load_structure(&path_str) {
                                Ok(structure) => {
                                    {
                                        let mut s = st.borrow_mut();
                                        s.original_structure = Some(structure.clone());
                                        s.structure = Some(structure);
                                        s.file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                                        s.selected_indices.clear();

                                        if let Some(con) = console_weak_inner.upgrade() {
                                            let report = s.get_structure_report();
                                            crate::ui::log_to_console(&con, &report);
                                        }
                                    } // Drop RefMut borrow here so we can use 'st' again below

                                    // 3. REFRESH SIDEBAR AND DRAW
                                    if let Some(da) = da_weak_inner.upgrade() {
                                        // Refresh the Atom List in Sidebar
                                        if let Some(ab) = atom_box_inner.upgrade() {
                                            // Call the public helper function we created in sidebar.rs
                                            crate::panels::sidebar::refresh_atom_list(&ab, st.clone(), &da);
                                        }
                                        da.queue_draw();
                                    }
                                },
                                Err(e) => eprintln!("Error loading file: {}", e),
                            }
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    app.add_action(&open_action);


    // --- SAVE AS ACTION ---
    let save_action = gtk4::gio::SimpleAction::new("save_as", None);
    let win_weak_s = window.downgrade();
    let state_weak_s = Rc::downgrade(&state);

    save_action.connect_activate(move |_, _| {
        let win = match win_weak_s.upgrade() { Some(w) => w, None => return };
        let dialog = FileChooserNative::new(Some("Save Structure As"), Some(&win), FileChooserAction::Save, Some("Save"), Some("Cancel"));

        let filter_cif = FileFilter::new(); filter_cif.set_name(Some("CIF File (*.cif)")); filter_cif.add_pattern("*.cif"); dialog.add_filter(&filter_cif);
        let filter_vasp = FileFilter::new(); filter_vasp.set_name(Some("VASP POSCAR")); filter_vasp.add_pattern("POSCAR"); dialog.add_filter(&filter_vasp);
        let filter_pot = FileFilter::new(); filter_pot.set_name(Some("SPRKKR Potential (*.pot)")); filter_pot.add_pattern("*.pot"); dialog.add_filter(&filter_pot);

        dialog.set_current_name("structure.cif");
        let state_weak_inner = state_weak_s.clone();

        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        if let Some(st) = state_weak_inner.upgrade() {
                            let s = st.borrow();
                            if let Some(structure) = &s.structure {
                                if let Err(e) = io::save_structure(&path_str, structure) { eprintln!("Failed: {}", e); }
                                else { println!("Saved to {}", path_str); }
                            }
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    app.add_action(&save_action);


    // --- EXPORT IMAGE ACTION ---
    let export_action = gtk4::gio::SimpleAction::new("export", None);
    let win_weak_e = window.downgrade();
    let state_weak_e = Rc::downgrade(&state);

    export_action.connect_activate(move |_, _| {
        let win = match win_weak_e.upgrade() { Some(w) => w, None => return };
        let st_rc = match state_weak_e.upgrade() { Some(s) => s, None => return };
        let state_weak_inner = state_weak_e.clone();

        let dialog = FileChooserNative::new(Some("Export Image"), Some(&win), FileChooserAction::Save, Some("Export"), Some("Cancel"));

        let filter_png = FileFilter::new(); filter_png.set_name(Some("PNG Image (*.png)")); filter_png.add_pattern("*.png"); dialog.add_filter(&filter_png);
        let filter_pdf = FileFilter::new(); filter_pdf.set_name(Some("PDF Document (*.pdf)")); filter_pdf.add_pattern("*.pdf"); dialog.add_filter(&filter_pdf);

        let format = st_rc.borrow().default_export_format;
        match format {
            ExportFormat::Png => { dialog.set_filter(&filter_png); dialog.set_current_name("snapshot.png"); },
            ExportFormat::Pdf => { dialog.set_filter(&filter_pdf); dialog.set_current_name("snapshot.pdf"); }
        }

        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                 if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        let is_pdf = path_str.to_lowercase().ends_with(".pdf");
                        if let Some(st) = state_weak_inner.upgrade() {
                             let s = st.borrow();
                             let _ = export_image(&s, &path_str, 2000.0, 1500.0, is_pdf);
                             println!("Exported to {}", path_str);
                        }
                    }
                 }
            }
            d.destroy();
        });
        dialog.show();
    });
    app.add_action(&export_action);


    // --- PREFERENCES ACTION ---
    // (Note: Since we moved appearance to the Sidebar, this might be redundant,
    // but we keep it here to avoid breaking your existing logic).
    // let pref_action = gtk4::gio::SimpleAction::new("preferences", None);
    // let win_weak_p = window.downgrade();
    // let state_weak_p = Rc::downgrade(&state);
    // let da_weak_p = drawing_area.downgrade();

    // pref_action.connect_activate(move |_, _| {
        // if let Some(win) = win_weak_p.upgrade() {
            // if let Some(st) = state_weak_p.upgrade() {
                // if let Some(da) = da_weak_p.upgrade() {
                    // show_preferences_window(&win, st);
                // }
            // }
        // }
    // });
    // app.add_action(&pref_action);


    // --- QUIT ACTION ---
    let quit_action = gtk4::gio::SimpleAction::new("quit", None);
    let win_weak_q = window.downgrade();

    quit_action.connect_activate(move |_, _| {
        if let Some(win) = win_weak_q.upgrade() {
            win.close();
        }
    });
    app.add_action(&quit_action);
}
