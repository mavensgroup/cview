// src/menu/actions_file.rs

use crate::io;
use crate::rendering::export_image;
use crate::state::{AppState, ExportFormat};
use crate::ui::preferences::show_preferences_window;
use gtk4::prelude::*;
use gtk4::{
  Application, ApplicationWindow, DrawingArea, FileChooserAction, FileChooserNative, FileFilter,
  ResponseType, TextView,
};
use std::cell::RefCell;
use std::rc::Rc;

// Local helper to append text to a TextView
fn log_msg(view: &TextView, text: &str) {
  let buffer = view.buffer();
  let mut end = buffer.end_iter();
  buffer.insert(&mut end, &format!("{}\n", text));

  // Auto-scroll
  let mark = buffer.create_mark(None, &buffer.end_iter(), false);
  view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
  buffer.delete_mark(&mark);
}

// 7 Arguments to match main.rs
pub fn setup(
  app: &Application,
  window: &ApplicationWindow,
  state: Rc<RefCell<AppState>>,
  drawing_area: &DrawingArea,
  system_log_view: &TextView,   // Arg 5
  interactions_view: &TextView, // Arg 6
  atom_list_box: &gtk4::Box,    // Arg 7
) {
  // --- OPEN ACTION ---
  let open_action = gtk4::gio::SimpleAction::new("open", None);
  let win_weak = window.downgrade();
  let state_weak = Rc::downgrade(&state);
  let da_weak = drawing_area.downgrade();

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
    let sys_log_inner = sys_log_weak.clone();
    let interact_inner = interact_weak.clone();
    let atom_box_inner = atom_box_weak.clone();

    dialog.connect_response(move |d, response| {
      if response == ResponseType::Accept {
        if let Some(file) = d.file() {
          if let Some(path) = file.path() {
            let path_str = path.to_string_lossy().to_string();

            if let Some(log_v) = sys_log_inner.upgrade() {
              log_msg(&log_v, &format!("Loading file: {}", path_str));
            }

            if let Some(st) = state_weak_inner.upgrade() {
              match crate::io::load_structure(&path_str) {
                Ok(structure) => {
                  {
                    let mut s = st.borrow_mut();
                    s.original_structure = Some(structure.clone());
                    s.structure = Some(structure);
                    s.file_name = path
                      .file_name()
                      .unwrap_or_default()
                      .to_string_lossy()
                      .to_string();
                    s.selected_indices.clear();

                    if let Some(interact_v) = interact_inner.upgrade() {
                      let report = s.get_structure_report();
                      log_msg(&interact_v, &report);
                    }
                  }

                  if let Some(log_v) = sys_log_inner.upgrade() {
                    log_msg(&log_v, "File loaded successfully.\n");
                  }

                  if let Some(da) = da_weak_inner.upgrade() {
                    if let Some(ab) = atom_box_inner.upgrade() {
                      crate::panels::sidebar::refresh_atom_list(&ab, st.clone(), &da);
                    }
                    da.queue_draw();
                  }
                }
                Err(e) => {
                  let err = format!("Error loading file: {}", e);
                  if let Some(log_v) = sys_log_inner.upgrade() {
                    log_msg(&log_v, &err);
                  }
                  eprintln!("{}", err);
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

  // --- SAVE AS ACTION ---
  let save_action = gtk4::gio::SimpleAction::new("save_as", None);
  let win_weak_s = window.downgrade();
  let state_weak_s = Rc::downgrade(&state);
  let sys_log_weak_s = system_log_view.downgrade();

  save_action.connect_activate(move |_, _| {
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

    let filter_cif = FileFilter::new();
    filter_cif.set_name(Some("CIF File (*.cif)"));
    filter_cif.add_pattern("*.cif");
    dialog.add_filter(&filter_cif);
    let filter_vasp = FileFilter::new();
    filter_vasp.set_name(Some("VASP POSCAR"));
    filter_vasp.add_pattern("POSCAR");
    dialog.add_filter(&filter_vasp);
    let filter_pot = FileFilter::new();
    filter_pot.set_name(Some("SPRKKR Potential (*.pot)"));
    filter_pot.add_pattern("*.pot");
    dialog.add_filter(&filter_pot);

    dialog.set_current_name("structure.cif");
    let state_weak_inner = state_weak_s.clone();
    let sys_log_inner = sys_log_weak_s.clone();

    dialog.connect_response(move |d, response| {
      if response == ResponseType::Accept {
        if let Some(file) = d.file() {
          if let Some(path) = file.path() {
            let path_str = path.to_string_lossy().to_string();
            if let Some(st) = state_weak_inner.upgrade() {
              let s = st.borrow();
              if let Some(structure) = &s.structure {
                if let Err(e) = io::save_structure(&path_str, structure) {
                  if let Some(log) = sys_log_inner.upgrade() {
                    log_msg(&log, &format!("Failed to save: {}", e));
                  }
                } else {
                  if let Some(log) = sys_log_inner.upgrade() {
                    log_msg(&log, &format!("Saved to {}", path_str));
                  }
                  println!("Saved to {}", path_str);
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
  app.add_action(&save_action);

  // --- EXPORT IMAGE ACTION ---
  let export_action = gtk4::gio::SimpleAction::new("export", None);
  let win_weak_e = window.downgrade();
  let state_weak_e = Rc::downgrade(&state);
  let sys_log_weak_e = system_log_view.downgrade();

  export_action.connect_activate(move |_, _| {
    let win = match win_weak_e.upgrade() {
      Some(w) => w,
      None => return,
    };
    let st_rc = match state_weak_e.upgrade() {
      Some(s) => s,
      None => return,
    };
    let state_weak_inner = state_weak_e.clone();
    let sys_log_inner = sys_log_weak_e.clone();

    let dialog = FileChooserNative::new(
      Some("Export Image"),
      Some(&win),
      FileChooserAction::Save,
      Some("Export"),
      Some("Cancel"),
    );

    let filter_png = FileFilter::new();
    filter_png.set_name(Some("PNG Image (*.png)"));
    filter_png.add_pattern("*.png");
    dialog.add_filter(&filter_png);
    let filter_pdf = FileFilter::new();
    filter_pdf.set_name(Some("PDF Document (*.pdf)"));
    filter_pdf.add_pattern("*.pdf");
    dialog.add_filter(&filter_pdf);

    let format = st_rc.borrow().default_export_format;
    match format {
      ExportFormat::Png => {
        dialog.set_filter(&filter_png);
        dialog.set_current_name("snapshot.png");
      }
      ExportFormat::Pdf => {
        dialog.set_filter(&filter_pdf);
        dialog.set_current_name("snapshot.pdf");
      }
    }

    dialog.connect_response(move |d, response| {
      if response == ResponseType::Accept {
        if let Some(file) = d.file() {
          if let Some(path) = file.path() {
            let path_str = path.to_string_lossy().to_string();
            let is_pdf = path_str.to_lowercase().ends_with(".pdf");
            if let Some(st) = state_weak_inner.upgrade() {
              let s = st.borrow();
              // Assuming export_image returns unit
              let _ = export_image(&s, &path_str, 2000.0, 1500.0, is_pdf);

              if let Some(log) = sys_log_inner.upgrade() {
                log_msg(&log, &format!("Exported to {}", path_str));
              }
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
  let pref_action = gtk4::gio::SimpleAction::new("preferences", None);
  let win_weak_p = window.downgrade();
  let state_weak_p = Rc::downgrade(&state);

  // CAPTURE DRAWING AREA FOR PREFERENCES
  let da_weak_p = drawing_area.downgrade();

  pref_action.connect_activate(move |_, _| {
    if let Some(win) = win_weak_p.upgrade() {
      if let Some(st) = state_weak_p.upgrade() {
        if let Some(da) = da_weak_p.upgrade() {
          // PASS 3 ARGS (window, state, drawing_area)
          show_preferences_window(&win, st, da.clone());
        }
      }
    }
  });
  app.add_action(&pref_action);

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
