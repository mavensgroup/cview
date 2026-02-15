// src/menu/actions_file.rs

use crate::io;
use crate::rendering::export::{export_pdf, export_png};
use crate::state::AppState;
use crate::ui::create_tab_content;
use crate::ui::preferences::show_preferences_window;
use crate::utils::report; // <--- ADDED: Needed for structure_summary
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
        filter_struct.set_name(Some("All Supported Formats"));
        filter_struct.add_pattern("*.cif");
        filter_struct.add_pattern("*.xyz");
        filter_struct.add_pattern("POSCAR*");
        filter_struct.add_pattern("CONTCAR*");
        filter_struct.add_pattern("*.vasp");
        filter_struct.add_pattern("*.pot");
        filter_struct.add_pattern("*.sys");
        filter_struct.add_pattern("*.in");
        filter_struct.add_pattern("*.pwi");
        filter_struct.add_pattern("*.qe");
        filter_struct.add_pattern("*.out");
        dialog.add_filter(&filter_struct);

        // Individual Filters
        let f_cif = FileFilter::new();
        f_cif.set_name(Some("CIF (*.cif)"));
        f_cif.add_pattern("*.cif");
        dialog.add_filter(&f_cif);

        let f_vasp = FileFilter::new();
        f_vasp.set_name(Some("VASP (POSCAR, *.vasp)"));
        f_vasp.add_pattern("POSCAR*");
        f_vasp.add_pattern("*.vasp");
        dialog.add_filter(&f_vasp);

        let f_spr = FileFilter::new();
        f_spr.set_name(Some("SPR-KKR (*.pot, *.sys)"));
        f_spr.add_pattern("*.pot");
        f_spr.add_pattern("*.sys");
        dialog.add_filter(&f_spr);

        let f_qe = FileFilter::new();
        f_qe.set_name(Some("Quantum Espresso (*.in, *.out)"));
        f_qe.add_pattern("*.in");
        f_qe.add_pattern("*.out");
        dialog.add_filter(&f_qe);

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
                                        let is_replace_mode = if s.tabs.is_empty() {
                                            false
                                        } else {
                                            let t = s.active_tab();
                                            t.structure.is_none() && t.file_name == "Untitled"
                                        };

                                        if is_replace_mode {
                                            let tab = s.active_tab_mut();
                                            tab.original_structure = Some(structure.clone());
                                            tab.structure = Some(structure);
                                            tab.file_name = filename.clone();
                                            replace_current_tab = true;
                                        } else {
                                            s.add_tab(structure, filename.clone());
                                            new_tab_index = Some(s.tabs.len() - 1);
                                        }
                                    }

                                    // --- 2. UI UPDATE BLOCK ---
                                    if let Some(nb) = nb_inner.upgrade() {
                                        if replace_current_tab {
                                            if let Some(page) = nb.nth_page(nb.current_page()) {
                                                if let Some(lbl_box) = nb.tab_label(&page) {
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
                                                    } else if let Some(l) =
                                                        lbl_box.downcast_ref::<Label>()
                                                    {
                                                        l.set_text(&filename);
                                                    }
                                                }
                                            }
                                            if let Some(da) = da_inner.upgrade() {
                                                da.queue_draw();
                                            }
                                        } else if let Some(idx) = new_tab_index {
                                            let (new_da, container) =
                                                create_tab_content(st_rc.clone(), idx);
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
                                        crate::panels::sidebar::refresh_atom_list(
                                            &ab,
                                            st_rc.clone(),
                                            &nb,
                                        );
                                    }
                                    if let Some(iv) = interact_inner.upgrade() {
                                        log_msg(&iv, &format!("Loaded: {}", filename));

                                        // --- ADDED: Print Structure Summary Report ---
                                        // This ensures the table appears for Menu loads, not just CLI.
                                        let s = st_rc.borrow();
                                        // Use active_tab() because we just set/added it above
                                        let tab = s.active_tab();
                                        if let Some(strc) = &tab.structure {
                                            let report_text =
                                                report::structure_summary(strc, &filename);
                                            log_msg(&iv, &report_text);
                                        }
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

        // Setup Save Dialog
        let dialog = FileChooserNative::new(
            Some("Save Structure As"),
            Some(&win),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );

        // --- ADD SAVE FILTERS HERE ---

        // 1. CIF
        let f_cif = FileFilter::new();
        f_cif.set_name(Some("CIF File (*.cif)"));
        f_cif.add_pattern("*.cif");
        dialog.add_filter(&f_cif);

        // 2. VASP / POSCAR
        let f_vasp = FileFilter::new();
        f_vasp.set_name(Some("VASP POSCAR"));
        f_vasp.add_pattern("POSCAR");
        f_vasp.add_pattern("*.vasp");
        dialog.add_filter(&f_vasp);

        // 3. SPR-KKR
        let f_pot = FileFilter::new();
        f_pot.set_name(Some("SPR-KKR Potential (*.pot)"));
        f_pot.add_pattern("*.pot");
        dialog.add_filter(&f_pot);

        // 4. Quantum Espresso
        let f_qe = FileFilter::new();
        f_qe.set_name(Some("Quantum Espresso Input (*.in)"));
        f_qe.add_pattern("*.in");
        f_qe.add_pattern("*.qe");
        dialog.add_filter(&f_qe);

        // 5. XYZ
        let f_xyz = FileFilter::new();
        f_xyz.set_name(Some("XYZ File (*.xyz)"));
        f_xyz.add_pattern("*.xyz");
        dialog.add_filter(&f_xyz);

        // Default name suggestion
        dialog.set_current_name("structure.cif");

        let state_inner = state_weak_s.clone();
        dialog.connect_response(move |d, r| {
            if r == ResponseType::Accept {
                if let Some(f) = d.file() {
                    if let Some(p) = f.path() {
                        if let Some(st) = state_inner.upgrade() {
                            let s = st.borrow();
                            if !s.tabs.is_empty() {
                                if let Some(strc) = &s.active_tab().structure {
                                    // io::save_structure determines format by extension
                                    let path_str = p.to_string_lossy();
                                    match io::save_structure(&path_str, strc) {
                                        Ok(_) => println!("Saved to {}", path_str),
                                        Err(e) => println!("Error saving: {}", e),
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
    app.add_action(&act_save);

    // // --- EXPORT ---
    // --- EXPORT ACTION (ADVANCED DIALOG) ---
    let act_export = gtk4::gio::SimpleAction::new("export", None);
    let win_weak_e = window.downgrade();
    let state_weak_e = Rc::downgrade(&state);

    act_export.connect_activate(move |_, _| {
        let win = match win_weak_e.upgrade() {
            Some(w) => w,
            None => return,
        };

        let state = match state_weak_e.upgrade() {
            Some(s) => s,
            None => return,
        };

        // Check if there's a structure loaded
        if state.borrow().tabs.is_empty() {
            return;
        }

        // Show the advanced export dialog
        crate::ui::export_dialog::show_export_dialog(&win, state);
    });
    app.add_action(&act_export);

    // let act_export = gtk4::gio::SimpleAction::new("export", None);
    // let win_weak_e = window.downgrade();
    // let state_weak_e = Rc::downgrade(&state);

    // act_export.connect_activate(move |_, _| {
    // let win = match win_weak_e.upgrade() {
    // Some(w) => w,
    // None => return,
    // };

    // if let Some(st) = state_weak_e.upgrade() {
    // if st.borrow().tabs.is_empty() {
    // return;
    // }
    // }

    // let dialog = FileChooserNative::new(
    // Some("Export Image/PDF"),
    // Some(&win),
    // FileChooserAction::Save,
    // Some("Export"),
    // Some("Cancel"),
    // );

    // let f_png = FileFilter::new();
    // f_png.set_name(Some("PNG Image (*.png)"));
    // f_png.add_pattern("*.png");
    // dialog.add_filter(&f_png);
    // let f_pdf = FileFilter::new();
    // f_pdf.set_name(Some("PDF Document (*.pdf)"));
    // f_pdf.add_pattern("*.pdf");
    // dialog.add_filter(&f_pdf);

    // let state_inner = state_weak_e.clone();

    // dialog.connect_response(move |d, r| {
    // if r == ResponseType::Accept {
    // if let Some(f) = d.file() {
    // if let Some(p) = f.path() {
    // let path = p.to_string_lossy().to_string();
    // if let Some(st) = state_inner.upgrade() {
    // if !st.borrow().tabs.is_empty() {
    // if path.to_lowercase().ends_with(".pdf") {
    // let _ = export_pdf(st, &path);
    // } else {
    // let _ = export_png(st, 2000.0, 1500.0, &path);
    // }
    // }
    // }
    // }
    // }
    // }
    // d.destroy();
    // });
    // dialog.show();
    // });
    // app.add_action(&act_export);

    // --- PREFS & QUIT ---
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

    let act_quit = gtk4::gio::SimpleAction::new("quit", None);
    let win_weak_q = window.downgrade();
    act_quit.connect_activate(move |_, _| {
        if let Some(w) = win_weak_q.upgrade() {
            w.close();
        }
    });
    app.add_action(&act_quit);
}
