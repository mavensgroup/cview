// src/menu/actions_file.rs

// use crate::config::ExportFormat;
use crate::io;
use crate::rendering::export::{export_pdf, export_png};
use crate::state::AppState;
use crate::ui::create_tab_content;
use crate::ui::preferences::show_preferences_window;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, DrawingArea, FileChooserAction, FileChooserNative, FileFilter,
    Label, Notebook, ResponseType, TextView,
};
use std::cell::RefCell;
use std::rc::Rc;

// Helper log function
fn log_msg(view: &TextView, text: &str) {
    let buffer = view.buffer();
    let mut end = buffer.end_iter();
    buffer.insert(&mut end, &format!("{}\n", text));
    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
    view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
    buffer.delete_mark(&mark);
}

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,
    drawing_area: &DrawingArea,
    system_log_view: &TextView,
    interactions_view: &TextView,
    atom_list_box: &gtk4::Box,
) {
    // --- OPEN ACTION ---
    let open_action = gtk4::gio::SimpleAction::new("open", None);

    // Weak references
    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let da_weak = drawing_area.downgrade();
    let notebook_weak = notebook.downgrade();
    let sys_log_weak = system_log_view.downgrade();
    let interact_weak = interactions_view.downgrade();
    let atom_box_weak = atom_list_box.downgrade();

    open_action.connect_activate(move |_, _| {
        let win = match win_weak.upgrade() {
            Some(w) => w,
            None => return,
        };

        let dialog = FileChooserNative::new(
            Some("Open Structure File"),
            Some(&win),
            FileChooserAction::Open,
            Some("Open"),
            Some("Cancel"),
        );

        // --- FILTERS ---
        let filter_struct = FileFilter::new();
        filter_struct.set_name(Some("Structure Files"));

        filter_struct.add_pattern("*.cif");
        filter_struct.add_pattern("POSCAR*");
        filter_struct.add_pattern("CONTCAR*");
        filter_struct.add_pattern("*.vasp");
        filter_struct.add_pattern("*.pot");
        filter_struct.add_pattern("*.sys");
        filter_struct.add_pattern("*.out");
        filter_struct.add_pattern("*.in");
        filter_struct.add_pattern("*.pwi");
        filter_struct.add_pattern("*.pwo");
        filter_struct.add_pattern("*.qe");
        filter_struct.add_pattern("*.xyz");

        dialog.add_filter(&filter_struct);

        let filter_any = FileFilter::new();
        filter_any.set_name(Some("All Files"));
        filter_any.add_pattern("*");
        dialog.add_filter(&filter_any);

        // Inner clones
        let state_inner = state_weak.clone();
        let da_inner = da_weak.clone();
        let nb_inner = notebook_weak.clone();
        let sys_log_inner = sys_log_weak.clone();
        let interact_inner = interact_weak.clone();
        let atom_box_inner = atom_box_weak.clone();
        let win_weak_inner = win.downgrade();

        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        let filename = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();

                        if let Some(st_rc) = state_inner.upgrade() {
                            match io::load_structure(&path_str) {
                                Ok(structure) => {
                                    let mut new_tab_index: Option<usize> = None;
                                    let mut replace_current_tab = false;

                                    // --- 1. STATE MUTATION BLOCK ---
                                    {
                                        let mut s = st_rc.borrow_mut();

                                        let is_replace_mode = {
                                            let t = s.active_tab();
                                            t.structure.is_none() && t.file_name == "Untitled"
                                        };

                                        if is_replace_mode {
                                            // REPLACE MODE
                                            let tab = s.active_tab_mut();
                                            tab.original_structure = Some(structure.clone());
                                            tab.structure = Some(structure);
                                            tab.file_name = filename.clone();
                                            replace_current_tab = true;
                                        } else {
                                            // NEW TAB MODE
                                            s.add_tab(structure, filename.clone());
                                            new_tab_index = Some(s.tabs.len() - 1);
                                        }
                                    }
                                    // --- STATE LOCK DROPPED HERE ---

                                    // --- 2. UI UPDATE BLOCK ---
                                    if let Some(nb) = nb_inner.upgrade() {
                                        if replace_current_tab {
                                            if let Some(page) = nb.nth_page(nb.current_page()) {
                                                if let Some(lbl_box) = nb.tab_label(&page) {
                                                    // We need to find the Label inside the Box we created
                                                    if let Some(bx) =
                                                        lbl_box.downcast_ref::<gtk4::Box>()
                                                    {
                                                        if let Some(first_child) = bx.first_child()
                                                        {
                                                            if let Some(l) =
                                                                first_child.downcast_ref::<Label>()
                                                            {
                                                                l.set_text(&filename);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            if let Some(da) = da_inner.upgrade() {
                                                da.queue_draw();
                                            }
                                        } else if let Some(idx) = new_tab_index {
                                            // Create new tab UI
                                            let (new_da, container) =
                                                create_tab_content(st_rc.clone(), idx);

                                            // USE HELPER HERE:
                                            crate::ui::add_closable_tab(
                                                &nb,
                                                &container,
                                                &filename,
                                                st_rc.clone(),
                                            );

                                            container.show();

                                            if let (Some(w), Some(iv)) =
                                                (win_weak_inner.upgrade(), interact_inner.upgrade())
                                            {
                                                crate::ui::setup_interactions(
                                                    &w,
                                                    st_rc.clone(),
                                                    &new_da,
                                                    &iv,
                                                );
                                            }

                                            nb.set_current_page(Some(idx as u32));
                                        }
                                    }
                                    // --- 3. REFRESH SIDEBAR & LOGS ---
                                    if let (Some(nb), Some(ab)) =
                                        (nb_inner.upgrade(), atom_box_inner.upgrade())
                                    {
                                        // FIX: Use 'nb' (notebook) instead of 'da'
                                        crate::panels::sidebar::refresh_atom_list(
                                            &ab,
                                            st_rc.clone(),
                                            &nb,
                                        );
                                    }

                                    if let Some(iv) = interact_inner.upgrade() {
                                        log_msg(&iv, &format!("Loaded: {}", filename));
                                    }
                                }
                                Err(e) => {
                                    if let Some(lv) = sys_log_inner.upgrade() {
                                        log_msg(&lv, &format!("Error loading file: {}", e));
                                    }
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
    app.add_action(&open_action);

    // --- SAVE AS ---
    let act_save = gtk4::gio::SimpleAction::new("save_as", None);
    let win_weak_s = window.downgrade();
    let state_weak_s = Rc::downgrade(&state);

    act_save.connect_activate(move |_, _| {
        let win = match win_weak_s.upgrade() {
            Some(w) => w,
            None => return,
        };
        let dialog = FileChooserNative::new(
            Some("Save As"),
            Some(&win),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );
        let filter = FileFilter::new();
        filter.add_pattern("*.cif");
        dialog.add_filter(&filter);
        dialog.set_current_name("structure.cif");

        let state_inner = state_weak_s.clone();
        dialog.connect_response(move |d, r| {
            if r == ResponseType::Accept {
                if let Some(f) = d.file() {
                    if let Some(p) = f.path() {
                        if let Some(st) = state_inner.upgrade() {
                            let s = st.borrow();
                            if let Some(strc) = &s.active_tab().structure {
                                let _ = io::save_structure(&p.to_string_lossy(), strc);
                            }
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    app.add_action(&act_save);

    // --- EXPORT ---
    let act_export = gtk4::gio::SimpleAction::new("export", None);
    let win_weak_e = window.downgrade();
    let state_weak_e = Rc::downgrade(&state);

    act_export.connect_activate(move |_, _| {
        let win = match win_weak_e.upgrade() {
            Some(w) => w,
            None => return,
        };
        let dialog = FileChooserNative::new(
            Some("Export"),
            Some(&win),
            FileChooserAction::Save,
            Some("Export"),
            Some("Cancel"),
        );
        let state_inner = state_weak_e.clone();

        dialog.connect_response(move |d, r| {
            if r == ResponseType::Accept {
                if let Some(f) = d.file() {
                    if let Some(p) = f.path() {
                        let path = p.to_string_lossy().to_string();
                        if let Some(st) = state_inner.upgrade() {
                            if path.to_lowercase().ends_with(".pdf") {
                                let _ = export_pdf(st, &path);
                            } else {
                                let _ = export_png(st, 2000.0, 1500.0, &path);
                            }
                        }
                    }
                }
            }
            d.destroy();
        });
        dialog.show();
    });
    app.add_action(&act_export);

    // --- PREFS ---
    let act_pref = gtk4::gio::SimpleAction::new("preferences", None);
    let win_weak_p = window.downgrade();
    let state_weak_p = Rc::downgrade(&state);
    let da_weak_p = drawing_area.downgrade();

    act_pref.connect_activate(move |_, _| {
        if let (Some(w), Some(s), Some(d)) = (
            win_weak_p.upgrade(),
            state_weak_p.upgrade(),
            da_weak_p.upgrade(),
        ) {
            show_preferences_window(&w, s, d);
        }
    });
    app.add_action(&act_pref);

    // --- QUIT ---
    let act_quit = gtk4::gio::SimpleAction::new("quit", None);
    let win_weak_q = window.downgrade();
    act_quit.connect_activate(move |_, _| {
        if let Some(w) = win_weak_q.upgrade() {
            w.close();
        }
    });
    app.add_action(&act_quit);
}
