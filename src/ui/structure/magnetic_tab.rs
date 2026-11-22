// src/ui/structure/magnetic_tab.rs
//
// Magnetic moments and magnetic order. A table holds one moment and one
// phase per atom of the current cell; rows are picked (by hand, from the
// main-view selection, or by element) and given a moment. A propagation
// vector k then spreads that onto the magnetic cell:
//
//     m_j(R) = m_j · cos(2π k·R + φ_j)
//
// k = 0 keeps the cell and only sets the moments. The preview shows the
// result with its arrows; the DFT card shows the INCAR tags (POSCAR atom
// order) for it. Apply is one step of Edit → Undo.

use super::preview::Preview;
use super::TabParts;
use crate::model::structure::Structure;
use crate::physics::operations::magnetic_order::{self, PRESETS};
use crate::physics::operations::spacegroup_build::parse_coord;
use crate::state::{AppState, Edit};
use crate::ui::style::{captioned, card_body, group};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DropDown, Entry, Label, ListBox, Notebook, Orientation, ScrolledWindow,
    SelectionMode,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

/// Applies an edit to every picked row, then redraws the list and preview.
type EditPicked = Rc<dyn Fn(&dyn Fn(&mut Site))>;

/// One row of the table: the moment (μB, Cartesian) and phase (degrees).
#[derive(Clone, Copy, Default)]
struct Site {
    moment: Option<[f64; 3]>,
    phase: f64,
}

/// What the table describes: the structure it was filled from (by atom
/// count and a cheap hash), so an edit elsewhere refills it.
#[derive(Default)]
struct Model {
    sites: Vec<Site>,
    source: u64,
}

fn structure_key(s: &Structure) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.atoms.len().hash(&mut h);
    for a in &s.atoms {
        a.element.hash(&mut h);
        a.position.map(f64::to_bits).hash(&mut h);
        a.magmom.map(|m| m.map(f64::to_bits)).hash(&mut h);
    }
    h.finish()
}

fn fmt_num(x: f64) -> String {
    let s = format!("{:.3}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn row_text(i: usize, element: &str, site: &Site) -> String {
    match site.moment {
        Some(m) if site.phase.abs() > 1e-9 => format!(
            "{:>4}  {:<3}  ({}, {}, {}) μB   φ {}°",
            i,
            element,
            fmt_num(m[0]),
            fmt_num(m[1]),
            fmt_num(m[2]),
            fmt_num(site.phase)
        ),
        Some(m) => format!("{:>4}  {:<3}  ({}, {}, {}) μB", i, element, fmt_num(m[0]), fmt_num(m[1]), fmt_num(m[2])),
        None => format!("{:>4}  {:<3}  —", i, element),
    }
}

/// Main-view selection as base atom indices.
fn main_selection(st: &AppState) -> HashSet<usize> {
    st.active_tab().interaction.selected.values().map(|a| a.original_index).collect()
}

fn small_caption(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.add_css_class("cview-caption");
    l
}

pub fn build(state: Rc<RefCell<AppState>>, main_notebook: &Notebook, preview: Preview) -> TabParts {
    let controls = GtkBox::new(Orientation::Vertical, 10);
    let model: Rc<RefCell<Model>> = Rc::default();
    // Set while widgets are filled in by code.
    let busy: Rc<Cell<bool>> = Rc::default();

    // --- Site moments ---
    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Multiple);
    list.add_css_class("cview-list");
    let list_scroll = ScrolledWindow::builder()
        .child(&list)
        .min_content_height(180)
        .max_content_height(260)
        .propagate_natural_height(true)
        .build();

    let dd_pick = DropDown::from_strings(&["—"]);
    let pick_names: Rc<RefCell<Vec<String>>> = Rc::default();

    let mxyz: [Entry; 3] = ["x", "y", "z"].map(|_| {
        let e = Entry::new();
        e.set_width_chars(5);
        e.set_text("0");
        e
    });
    mxyz[2].set_text("1");
    let entry_phase = Entry::new();
    entry_phase.set_width_chars(5);
    entry_phase.set_text("0");
    let m_row = GtkBox::new(Orientation::Horizontal, 8);
    m_row.set_homogeneous(true);
    for (c, e) in ["mx (μB)", "my (μB)", "mz (μB)"].iter().zip(&mxyz) {
        m_row.append(&captioned(c, e));
    }
    m_row.append(&captioned("φ (°)", &entry_phase));

    let btn_set = Button::with_label("Set on picked rows");
    btn_set.add_css_class("suggested-action");
    let btn_clear = Button::with_label("No moment");
    let btn_flip = Button::with_label("Reverse");
    btn_flip.set_tooltip_text(Some("Reverse the moments of the picked rows"));
    let set_row = GtkBox::new(Orientation::Horizontal, 8);
    set_row.set_homogeneous(true);
    set_row.append(&btn_clear);
    set_row.append(&btn_flip);
    set_row.append(&btn_set);
    let lbl_picked = Label::new(None);
    lbl_picked.set_xalign(0.0);
    lbl_picked.add_css_class("cview-caption");

    let sites_body = card_body(8);
    sites_body.append(&captioned("Pick rows", &dd_pick));
    sites_body.append(&list_scroll);
    sites_body.append(&lbl_picked);
    sites_body.append(&m_row);
    sites_body.append(&set_row);
    sites_body.append(&small_caption(
        "Moments are Cartesian, in μB. Pick rows in the list (Ctrl/Shift for \
         more), or from the main-view selection or by element above. φ is a \
         per-site phase for the propagation vector: 180° reverses the site.",
    ));
    controls.append(&group("Site moments", &sites_body));

    // --- Propagation vector ---
    let mut preset_names: Vec<&str> = vec!["Custom"];
    preset_names.extend(PRESETS.iter().map(|p| p.0));
    let dd_preset = DropDown::from_strings(&preset_names);
    let kxyz: [Entry; 3] = ["a*", "b*", "c*"].map(|_| {
        let e = Entry::new();
        e.set_width_chars(5);
        e.set_text("0");
        e
    });
    let k_row = GtkBox::new(Orientation::Horizontal, 8);
    k_row.set_homogeneous(true);
    for (c, e) in ["k₁", "k₂", "k₃"].iter().zip(&kxyz) {
        k_row.append(&captioned(c, e));
    }
    let k_body = card_body(8);
    k_body.append(&captioned("Preset", &dd_preset));
    k_body.append(&k_row);
    k_body.append(&small_caption(
        "k in reciprocal-lattice units (fractions such as 1/2 or 1/3). Each copy \
         of site j, moved by the lattice vector R, gets m_j·cos(2π k·R + φ_j); the \
         new cell is the denominator of each kᵢ times the old one. The presets \
         assume one magnetic atom per (pseudo)cubic cell, as on the B site of a \
         perovskite; for several magnetic sites per cell, set their phases.",
    ));
    controls.append(&group("Propagation vector", &k_body));

    // --- Result and apply ---
    let lbl_result = small_caption("");
    let btn_reload = Button::with_label("Reload from structure");
    let btn_apply = Button::with_label("Apply");
    btn_apply.add_css_class("suggested-action");
    let apply_row = GtkBox::new(Orientation::Horizontal, 8);
    apply_row.set_homogeneous(true);
    apply_row.append(&btn_reload);
    apply_row.append(&btn_apply);
    controls.append(&lbl_result);
    controls.append(&apply_row);

    // --- DFT input ---
    let lbl_incar = Label::new(None);
    lbl_incar.set_selectable(true);
    lbl_incar.set_wrap(true);
    lbl_incar.set_xalign(0.0);
    lbl_incar.add_css_class("monospace");
    let btn_copy = Button::with_label("Copy INCAR tags");
    btn_copy.set_halign(Align::End);
    let dft_body = card_body(8);
    dft_body.append(&lbl_incar);
    dft_body.append(&btn_copy);
    dft_body.append(&small_caption(
        "For the result above, in the atom order CView writes a POSCAR in. \
         File → Save As POSCAR also writes these into an INCAR beside it when \
         there is none. Saving as QE input (.in) splits each element into one \
         species per moment (Fe1, Fe2) with nspin = 2 and starting_magnetization \
         (non-collinear: noncolin with angle1/angle2).",
    ));
    controls.append(&group("DFT input", &dft_body));

    let read_k = {
        let kxyz = kxyz.clone();
        move || -> Option<[f64; 3]> {
            let mut k = [0.0; 3];
            for (i, e) in kxyz.iter().enumerate() {
                k[i] = parse_coord(&e.text())?;
            }
            Some(k)
        }
    };

    // Fill the list from the model.
    let fill_list: Rc<dyn Fn()> = {
        let (state, model, list) = (state.clone(), model.clone(), list.clone());
        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let st = state.borrow();
            let Some(s) = st.active_tab().structure.as_ref() else { return };
            let m = model.borrow();
            for (i, (a, site)) in s.atoms.iter().zip(&m.sites).enumerate() {
                let l = Label::new(Some(&row_text(i, &a.element, site)));
                l.set_xalign(0.0);
                l.add_css_class("monospace");
                list.append(&l);
            }
        })
    };

    // Refill the model (and the list) from the structure when it changed.
    let sync_model: Rc<dyn Fn(bool)> = {
        let (state, model, fill_list) = (state.clone(), model.clone(), fill_list.clone());
        let (dd_pick, pick_names, busy) = (dd_pick.clone(), pick_names.clone(), busy.clone());
        Rc::new(move |force: bool| {
            let key;
            let mut elements: Vec<String> = Vec::new();
            {
                let st = state.borrow();
                let Some(s) = st.active_tab().structure.as_ref() else {
                    model.borrow_mut().sites.clear();
                    return;
                };
                key = structure_key(s);
                if !force && model.borrow().source == key {
                    return;
                }
                let mut m = model.borrow_mut();
                m.sites = s.atoms.iter().map(|a| Site { moment: a.magmom, phase: 0.0 }).collect();
                m.source = key;
                for a in &s.atoms {
                    if !elements.contains(&a.element) {
                        elements.push(a.element.clone());
                    }
                }
            }
            let mut names = vec!["—".to_string(), "Main-view selection".into(), "All atoms".into()];
            names.extend(elements.iter().map(|e| format!("All {e}")));
            busy.set(true);
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            dd_pick.set_model(Some(&gtk4::StringList::new(&refs)));
            dd_pick.set_selected(0);
            busy.set(false);
            *pick_names.borrow_mut() = elements;
            fill_list();
        })
    };

    // Recompute the candidate, the result line, the preview and the tags.
    let refresh: Rc<dyn Fn()> = {
        let (state, model, preview) = (state.clone(), model.clone(), preview.clone());
        let (lbl_result, lbl_incar, btn_apply, btn_copy) =
            (lbl_result.clone(), lbl_incar.clone(), btn_apply.clone(), btn_copy.clone());
        let (list, lbl_picked) = (list.clone(), lbl_picked.clone());
        let read_k = read_k.clone();
        Rc::new(move || {
            lbl_picked.set_text(&format!("{} row(s) picked", list.selected_rows().len()));
            let st = state.borrow();
            let Some(s) = st.active_tab().structure.as_ref() else {
                lbl_result.set_text("No structure loaded.");
                btn_apply.set_sensitive(false);
                preview.set_candidate(None);
                return;
            };
            let Some(k) = read_k() else {
                lbl_result.set_text("k: enter three numbers or fractions.");
                btn_apply.set_sensitive(false);
                preview.set_candidate(None);
                return;
            };
            let m = model.borrow();
            if m.sites.len() != s.atoms.len() {
                return;
            }
            let moments: Vec<_> = m.sites.iter().map(|x| x.moment).collect();
            let phases: Vec<_> = m.sites.iter().map(|x| x.phase).collect();
            match magnetic_order::build(s, k, &moments, &phases) {
                Ok(c) => {
                    let n = magnetic_order::cell_multiples(k).unwrap_or([1; 3]);
                    let net = magnetic_order::net_moment(&c);
                    let norm = (net[0] * net[0] + net[1] * net[1] + net[2] * net[2]).sqrt();
                    let with = c.atoms.iter().filter(|a| a.magmom.is_some()).count();
                    let cell = if n == [1, 1, 1] {
                        "Same cell".to_string()
                    } else {
                        format!("Magnetic cell {}×{}×{} the current one", n[0], n[1], n[2])
                    };
                    lbl_result.set_text(&format!(
                        "{}: {} atoms, {} with a moment. Net moment ({}, {}, {}) μB, |M| = {} μB.",
                        cell,
                        c.atoms.len(),
                        with,
                        fmt_num(net[0]),
                        fmt_num(net[1]),
                        fmt_num(net[2]),
                        fmt_num(norm)
                    ));
                    match crate::io::magmom::vasp_incar_tags(&c) {
                        Some(t) => {
                            lbl_incar.set_text(t.trim_end());
                            btn_copy.set_sensitive(true);
                        }
                        None => {
                            lbl_incar.set_text("No moments.");
                            btn_copy.set_sensitive(false);
                        }
                    }
                    btn_apply.set_sensitive(true);
                    preview.set_candidate(Some(c));
                }
                Err(e) => {
                    lbl_result.set_text(&e);
                    btn_apply.set_sensitive(false);
                    preview.set_candidate(None);
                }
            }
        })
    };

    // Picked rows → model edits.
    let edit_picked: EditPicked = {
        let (model, list, fill_list, refresh) = (model.clone(), list.clone(), fill_list.clone(), refresh.clone());
        Rc::new(move |f: &dyn Fn(&mut Site)| {
            let picked: Vec<usize> = list.selected_rows().iter().map(|r| r.index() as usize).collect();
            {
                let mut m = model.borrow_mut();
                for i in &picked {
                    if let Some(s) = m.sites.get_mut(*i) {
                        f(s);
                    }
                }
            }
            fill_list();
            for i in picked {
                if let Some(r) = list.row_at_index(i as i32) {
                    list.select_row(Some(&r));
                }
            }
            refresh();
        })
    };
    {
        let (edit, mxyz, entry_phase, lbl) = (edit_picked.clone(), mxyz.clone(), entry_phase.clone(), lbl_result.clone());
        btn_set.connect_clicked(move |_| {
            let v: Option<Vec<f64>> = mxyz.iter().map(|e| parse_coord(&e.text())).collect();
            let (Some(v), Some(phase)) = (v, parse_coord(&entry_phase.text())) else {
                lbl.set_text("Moment and φ: enter numbers.");
                return;
            };
            let m = [v[0], v[1], v[2]];
            let moment = (m.iter().any(|x| x.abs() > 1e-9)).then_some(m);
            edit(&move |s: &mut Site| {
                s.moment = moment;
                s.phase = phase;
            });
        });
    }
    {
        let edit = edit_picked.clone();
        btn_clear.connect_clicked(move |_| edit(&|s: &mut Site| *s = Site::default()));
    }
    {
        let edit = edit_picked.clone();
        btn_flip.connect_clicked(move |_| edit(&|s: &mut Site| s.moment = s.moment.map(|m| m.map(|x| -x))));
    }

    // Pick rows: main-view selection, all, or one element.
    {
        let (state, list, busy, names) = (state.clone(), list.clone(), busy.clone(), pick_names.clone());
        let refresh = refresh.clone();
        dd_pick.connect_selected_notify(move |dd| {
            if busy.get() || dd.selected() == 0 {
                return;
            }
            let choice = dd.selected() as usize;
            let picks: Vec<usize> = {
                let st = state.borrow();
                let Some(s) = st.active_tab().structure.as_ref() else { return };
                match choice {
                    1 => {
                        let sel = main_selection(&st);
                        (0..s.atoms.len()).filter(|i| sel.contains(i)).collect()
                    }
                    2 => (0..s.atoms.len()).collect(),
                    n => {
                        let el = names.borrow().get(n - 3).cloned().unwrap_or_default();
                        (0..s.atoms.len()).filter(|&i| s.atoms[i].element == el).collect()
                    }
                }
            };
            list.unselect_all();
            for i in picks {
                if let Some(r) = list.row_at_index(i as i32) {
                    list.select_row(Some(&r));
                }
            }
            busy.set(true);
            dd.set_selected(0);
            busy.set(false);
            refresh();
        });
    }
    {
        let refresh = refresh.clone();
        list.connect_selected_rows_changed(move |_| refresh());
    }

    // k: presets fill the entries; typing goes to Custom.
    {
        let (kxyz, busy, refresh) = (kxyz.clone(), busy.clone(), refresh.clone());
        dd_preset.connect_selected_notify(move |dd| {
            let Some((_, k)) = (dd.selected() as usize).checked_sub(1).and_then(|i| PRESETS.get(i)) else {
                return;
            };
            busy.set(true);
            for (e, v) in kxyz.iter().zip(k) {
                e.set_text(if *v == 0.5 { "1/2" } else { "0" });
            }
            busy.set(false);
            refresh();
        });
    }
    for e in &kxyz {
        let (busy, dd, refresh) = (busy.clone(), dd_preset.clone(), refresh.clone());
        e.connect_changed(move |_| {
            if busy.get() {
                return;
            }
            dd.set_selected(0);
            refresh();
        });
    }

    // Copy the INCAR tags.
    {
        let lbl = lbl_incar.clone();
        btn_copy.connect_clicked(move |b| {
            b.clipboard().set_text(&format!("{}\n", lbl.text()));
            crate::utils::console::log_info("Copied the INCAR magnetic tags to the clipboard");
        });
    }

    // Reload and apply.
    {
        let (sync, refresh) = (sync_model.clone(), refresh.clone());
        btn_reload.connect_clicked(move |_| {
            sync(true);
            refresh();
        });
    }
    {
        let (state, model, nb) = (state.clone(), model.clone(), main_notebook.downgrade());
        let (kxyz, dd_preset, busy) = (kxyz.clone(), dd_preset.clone(), busy.clone());
        let (read_k, sync, refresh) = (read_k.clone(), sync_model.clone(), refresh.clone());
        btn_apply.connect_clicked(move |_| {
            let Some(k) = read_k() else { return };
            {
                let mut st = state.borrow_mut();
                let tab = st.active_tab_mut();
                let Some(s) = tab.structure.as_ref() else { return };
                let m = model.borrow();
                let moments: Vec<_> = m.sites.iter().map(|x| x.moment).collect();
                let phases: Vec<_> = m.sites.iter().map(|x| x.phase).collect();
                let new = match magnetic_order::build(s, k, &moments, &phases) {
                    Ok(n) => n,
                    Err(e) => {
                        crate::utils::console::log_warn(&format!("Magnetic order: {e}"));
                        return;
                    }
                };
                let same_cell = magnetic_order::cell_multiples(k).map(|n| n == [1, 1, 1]).unwrap_or(false);
                let ks: Vec<String> = kxyz.iter().map(|e| e.text().trim().to_string()).collect();
                if same_cell {
                    // Same atoms in the same places: selection and overrides stay.
                    tab.commit("Set magnetic moments", new, Edit::Species);
                } else {
                    tab.commit(&format!("Magnetic order, k = ({})", ks.join(", ")), new, Edit::Reindexed);
                }
            }
            if let Some(da) = nb.upgrade().and_then(|nb| crate::ui::get_active_drawing_area(&nb)) {
                da.queue_draw();
            }
            // Applied: back to k = 0, with the table refilled from the result,
            // so the preview shows the structure as it is now.
            busy.set(true);
            for e in &kxyz {
                e.set_text("0");
            }
            dd_preset.set_selected(0);
            busy.set(false);
            sync(true);
            refresh();
        });
    }

    // Follow edits made elsewhere while the tab is open (undo, another tab).
    {
        let (sync, refresh) = (sync_model.clone(), refresh.clone());
        let weak = controls.downgrade();
        let state = Rc::downgrade(&state);
        let last = Cell::new(0u64);
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let (Some(w), Some(st)) = (weak.upgrade(), state.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if !w.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            let key = st.borrow().active_tab().structure.as_ref().map(structure_key).unwrap_or(0);
            if key != last.get() {
                last.set(key);
                sync(false);
                refresh();
            }
            glib::ControlFlow::Continue
        });
    }

    let on_enter = {
        let (sync, refresh, preview) = (sync_model.clone(), refresh.clone(), preview.clone());
        move || {
            preview.clear_highlight();
            sync(false);
            refresh();
        }
    };
    TabParts { controls, on_enter: Box::new(on_enter) }
}
