// src/menu/actions_file.rs

use crate::io;
use crate::panels::sidebar::SidebarHandles;
use crate::state::AppState;
use crate::ui::create_tab_content;
use crate::ui::preferences::show_preferences_window;
use crate::utils::{console, report};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, DrawingArea, FileChooserAction, FileChooserNative, FileFilter,
    Label, Notebook, ResponseType,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Everything `open_path` needs to put a loaded file on screen. All weak, so
/// a load that finishes after its window closed does nothing.
#[derive(Clone)]
pub struct OpenContext {
    pub state: std::rc::Weak<RefCell<AppState>>,
    pub notebook: gtk4::glib::WeakRef<Notebook>,
    pub atom_box: gtk4::glib::WeakRef<gtk4::Box>,
    pub window: gtk4::glib::WeakRef<ApplicationWindow>,
    pub handles: Rc<SidebarHandles>,
}

/// Open `path`: read it on a worker thread (an MD trajectory can be hundreds
/// of MB), then show it in the current tab if that is the empty placeholder,
/// otherwise in a new tab.
pub fn open_path(ctx: &OpenContext, path_str: &str) {
    let Some(st_rc) = ctx.state.upgrade() else { return };
    let filename = std::path::Path::new(path_str)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let choice = st_rc.borrow().config.open_frame;
    let worker_path = path_str.to_string();
    let ctx = ctx.clone();
    let started = std::time::Instant::now();
    let job = crate::utils::task::spawn(
        move |_cancel| Some(io::load_document(&worker_path, choice)),
        move |result| match result {
            Ok(doc) => show_document(&ctx, &filename, doc, started.elapsed()),
            Err(e) => console::log_error(&format!("Error loading '{}': {}", filename, e)),
        },
    );
    st_rc.borrow_mut().load_jobs.push(job);
}

fn show_document(ctx: &OpenContext, filename: &str, doc: io::Document, took: std::time::Duration) {
    let Some(st_rc) = ctx.state.upgrade() else { return };
    let io::Document { structure, trajectory, frame } = doc;
    let trajectory = trajectory.map(Rc::new);
    let tab_index: usize;
    let replace_current_tab;

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
            tab.file_name = filename.to_string();
            // Replacing the structure in-place must reset every per-tab piece
            // of state that referred to the previous structure — otherwise old
            // selections appear as "pre-highlighted" atoms on the new one.
            tab.interaction.selected.clear();
            tab.interaction.undo_stack.clear();
            tab.miller_planes.clear();
            tab.kpath_result = None;
            tab.void_result = None;
            tab.invalidate_derived();
            tab.style_generic_species();
            replace_current_tab = true;
            tab_index = s.active_tab_index;
        } else {
            s.add_tab(structure, filename.to_string());
            replace_current_tab = false;
            tab_index = s.tabs.len() - 1;
        }
        let tab = &mut s.tabs[tab_index];
        tab.trajectory = trajectory.clone();
        tab.trajectory_frame = frame;
    }

    if let Some(nb) = ctx.notebook.upgrade() {
        if replace_current_tab {
            if let Some(page) = nb.nth_page(nb.current_page()) {
                if let Some(lbl_box) = nb.tab_label(&page) {
                    if let Some(bx) = lbl_box.downcast_ref::<gtk4::Box>() {
                        if let Some(first_child) = bx.first_child() {
                            if let Some(l) = first_child.downcast_ref::<Label>() {
                                l.set_text(filename);
                            }
                        }
                    } else if let Some(l) = lbl_box.downcast_ref::<Label>() {
                        l.set_text(filename);
                    }
                }
            }
            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                da.queue_draw();
            }
        } else {
            let (new_da, container) = create_tab_content(st_rc.clone(), tab_index);
            crate::ui::add_closable_tab(&nb, &container, filename, st_rc.clone());
            container.show();
            if let Some(w) = ctx.window.upgrade() {
                crate::ui::setup_interactions(&w, st_rc.clone(), &new_da, ctx.handles.clone());
            }
            nb.set_current_page(Some(tab_index as u32));
        }
    }

    // Refresh sidebar & log
    if let (Some(nb), Some(ab)) = (ctx.notebook.upgrade(), ctx.atom_box.upgrade()) {
        crate::panels::sidebar::refresh_atom_list(&ab, st_rc.clone(), &nb);
    }
    crate::menu::actions_trajectory::refresh_enabled(&st_rc.borrow());

    console::log_info(&format!("Loaded: {} ({:.2} s)", filename, took.as_secs_f64()));
    if let Some(t) = &trajectory {
        console::log_info(&format!(
            "{}: {} frames ({}); showing frame {}. Structure \u{2192} Trajectory Player to play them.",
            filename,
            t.len(),
            t.format,
            frame + 1
        ));
    }
    let s = st_rc.borrow();
    if let Some(strc) = s.tabs.get(tab_index).and_then(|t| t.structure.as_ref()) {
        console::info_report(&report::structure_summary(strc, filename));
    }
}

pub fn setup(
    app: &Application,
    window: &ApplicationWindow,
    state: Rc<RefCell<AppState>>,
    notebook: &Notebook,
    drawing_area: &DrawingArea,
    atom_list_box: &gtk4::Box,
    sidebar_handles: Rc<SidebarHandles>,
) {
    // --- OPEN ACTION ---
    let open_action = gtk4::gio::SimpleAction::new("open", None);

    let win_weak = window.downgrade();
    let state_weak = Rc::downgrade(&state);
    let notebook_weak = notebook.downgrade();
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

        // ── Filters ──
        let filter_struct = FileFilter::new();
        filter_struct.set_name(Some("All Supported Formats"));
        filter_struct.add_pattern("*.cif");
        filter_struct.add_pattern("*.CIF");
        filter_struct.add_pattern("*.xyz");
        filter_struct.add_pattern("*.pdb");
        filter_struct.add_pattern("*.ent");
        filter_struct.add_pattern("POSCAR");
        filter_struct.add_pattern("POSCAR*");
        filter_struct.add_pattern("poscar");
        filter_struct.add_pattern("poscar*");
        filter_struct.add_pattern("CONTCAR");
        filter_struct.add_pattern("CONTCAR*");
        filter_struct.add_pattern("contcar");
        filter_struct.add_pattern("contcar*");
        filter_struct.add_pattern("*.vasp");
        filter_struct.add_pattern("*.VASP");
        filter_struct.add_pattern("*.pot");
        filter_struct.add_pattern("*.pot_*");
        filter_struct.add_pattern("*.sys");
        filter_struct.add_pattern("*.inp");
        filter_struct.add_pattern("*.in");
        filter_struct.add_pattern("*.pwi");
        filter_struct.add_pattern("*.qe");
        filter_struct.add_pattern("*.out");
        filter_struct.add_pattern("*.log");
        filter_struct.add_pattern("*.dump");
        filter_struct.add_pattern("*.lammpstrj");
        filter_struct.add_pattern("*.dat");
        dialog.add_filter(&filter_struct);

        let f_lmp = FileFilter::new();
        f_lmp.set_name(Some("LAMMPS dump (*.dump, *.lammpstrj, *.dat)"));
        f_lmp.add_pattern("*.dump");
        f_lmp.add_pattern("*.lammpstrj");
        f_lmp.add_pattern("*.dat");
        dialog.add_filter(&f_lmp);

        let f_cif = FileFilter::new();
        f_cif.set_name(Some("CIF (*.cif)"));
        f_cif.add_pattern("*.cif");
        f_cif.add_pattern("*.CIF");
        dialog.add_filter(&f_cif);

        let f_vasp = FileFilter::new();
        f_vasp.set_name(Some("VASP (POSCAR, CONTCAR, *.vasp)"));
        f_vasp.add_pattern("POSCAR");
        f_vasp.add_pattern("POSCAR*");
        f_vasp.add_pattern("poscar");
        f_vasp.add_pattern("poscar*");
        f_vasp.add_pattern("CONTCAR");
        f_vasp.add_pattern("CONTCAR*");
        f_vasp.add_pattern("contcar");
        f_vasp.add_pattern("contcar*");
        f_vasp.add_pattern("*.vasp");
        f_vasp.add_pattern("*.VASP");
        dialog.add_filter(&f_vasp);

        let f_xyz = FileFilter::new();
        f_xyz.set_name(Some("XYZ (*.xyz)"));
        f_xyz.add_pattern("*.xyz");
        dialog.add_filter(&f_xyz);

        let f_pdb = FileFilter::new();
        f_pdb.set_name(Some("PDB (*.pdb, *.ent)"));
        f_pdb.add_pattern("*.pdb");
        f_pdb.add_pattern("*.ent");
        dialog.add_filter(&f_pdb);

        let f_spr = FileFilter::new();
        f_spr.set_name(Some("SPR-KKR (*.pot, *.sys, *.inp)"));
        f_spr.add_pattern("*.pot");
        f_spr.add_pattern("*.sys");
        f_spr.add_pattern("*.inp");
        dialog.add_filter(&f_spr);

        let f_qe = FileFilter::new();
        f_qe.set_name(Some("Quantum ESPRESSO (*.in, *.pwi, *.qe, *.out)"));
        f_qe.add_pattern("*.in");
        f_qe.add_pattern("*.pwi");
        f_qe.add_pattern("*.qe");
        f_qe.add_pattern("*.out");
        f_qe.add_pattern("*.log");
        dialog.add_filter(&f_qe);

        let filter_any = FileFilter::new();
        filter_any.set_name(Some("All Files"));
        filter_any.add_pattern("*");
        dialog.add_filter(&filter_any);

        let state_inner = state_weak.clone();
        let nb_inner = notebook_weak.clone();
        let atom_box_inner = atom_box_weak.clone();
        let win_weak_inner = win.downgrade();
        let handles_inner = sidebar_handles.clone();

        dialog.connect_response(move |d, response| {
            if response == ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        open_path(
                            &OpenContext {
                                state: state_inner.clone(),
                                notebook: nb_inner.clone(),
                                atom_box: atom_box_inner.clone(),
                                window: win_weak_inner.clone(),
                                handles: handles_inner.clone(),
                            },
                            &path_str,
                        );
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
            Some("Save Structure As"),
            Some(&win),
            FileChooserAction::Save,
            Some("Save"),
            Some("Cancel"),
        );

        let f_cif = FileFilter::new();
        f_cif.set_name(Some("CIF File (*.cif)"));
        f_cif.add_pattern("*.cif");
        dialog.add_filter(&f_cif);

        let f_vasp = FileFilter::new();
        f_vasp.set_name(Some("VASP POSCAR (*.vasp)"));
        f_vasp.add_pattern("POSCAR");
        f_vasp.add_pattern("*.vasp");
        dialog.add_filter(&f_vasp);

        let f_pot = FileFilter::new();
        f_pot.set_name(Some("SPR-KKR Potential (*.pot)"));
        f_pot.add_pattern("*.pot");
        dialog.add_filter(&f_pot);

        let f_qe = FileFilter::new();
        f_qe.set_name(Some("Quantum ESPRESSO Input (*.in)"));
        f_qe.add_pattern("*.in");
        f_qe.add_pattern("*.qe");
        dialog.add_filter(&f_qe);

        let f_xyz = FileFilter::new();
        f_xyz.set_name(Some("XYZ File (*.xyz)"));
        f_xyz.add_pattern("*.xyz");
        dialog.add_filter(&f_xyz);

        let f_pdb = FileFilter::new();
        f_pdb.set_name(Some("PDB File (*.pdb)"));
        f_pdb.add_pattern("*.pdb");
        dialog.add_filter(&f_pdb);

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
                                    let path_str = p.to_string_lossy();
                                    match io::save_structure(&path_str, strc) {
                                        Ok(_) => {
                                            console::log_info(&format!("Saved to {}", path_str));
                                        }
                                        Err(e) => {
                                            console::log_error(&format!("Error saving: {}", e));
                                        }
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

        if state.borrow().tabs.is_empty() {
            return;
        }

        crate::ui::export_dialog::show_export_dialog(&win, state);
    });
    app.add_action(&act_export);

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
