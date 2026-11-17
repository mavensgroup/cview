// src/ui/structure/atoms_tab.rs
//
// Add an atom, or move or replace the atoms selected in the main view.
// Positions are typed as fractional or Cartesian coordinates. The preview shows the result before
// it is applied and rings the atoms that change; the nearest-atom distance
// warns of overlaps. Each change is one step of Edit → Undo.

use super::preview::Preview;
use super::TabParts;
use crate::model::structure::Structure;
use crate::physics::operations::atom_edit;
use crate::physics::operations::spacegroup_build::{normalize_element, parse_coord};
use crate::state::{AppState, Edit};
use crate::ui::style::{captioned, card_body, group};
use crate::utils::linalg::{cart_to_frac, frac_to_cart};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DropDown, Entry, Label, Notebook, Orientation};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Duration;

/// Which card the preview follows: the one edited last.
#[derive(Clone, Copy, PartialEq)]
enum Card {
    Add,
    Move,
    Replace,
}

/// An interstitial site's position: (fractional, Cartesian).
type SitePos = ([f64; 3], [f64; 3]);

/// Position of "Move to" in the Mode dropdown ("Shift by" is 0).
const MOVE_TO: u32 = 1;

/// A number as a user would type it: no trailing zeros, no "-0".
fn fmt_num(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Three captioned entries for x, y, z.
fn coord_row() -> (GtkBox, [Entry; 3]) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_homogeneous(true);
    let entries = ["x", "y", "z"].map(|c| {
        let e = Entry::new();
        e.set_width_chars(6);
        e.set_text("0");
        row.append(&captioned(c, &e));
        e
    });
    (row, entries)
}

/// The three values as typed ("0.25", "1/3", "-2.1"), or None if any is not a
/// number.
fn read(entries: &[Entry; 3]) -> Option<[f64; 3]> {
    let mut v = [0.0; 3];
    for (k, e) in entries.iter().enumerate() {
        v[k] = parse_coord(&e.text())?;
    }
    Some(v)
}

fn write(entries: &[Entry; 3], v: [f64; 3]) {
    for (e, x) in entries.iter().zip(v) {
        e.set_text(&fmt_num(x));
    }
}

/// Selected atoms (base indices) in pick order, each once, and only those
/// the structure still has.
fn selected_indices(st: &AppState) -> Vec<usize> {
    let tab = st.active_tab();
    let n = tab.structure.as_ref().map_or(0, |s| s.atoms.len());
    let mut seen = HashSet::new();
    tab.interaction
        .selected_in_order()
        .into_iter()
        .map(|a| a.original_index)
        .filter(|&i| i < n && seen.insert(i))
        .collect()
}

/// Element symbol from what was typed: a real element ("fe" → "Fe"), or a
/// species name already in the structure ("Type 1" from a LAMMPS dump).
fn element_of(text: &str, s: &Structure) -> Option<String> {
    normalize_element(text).or_else(|| {
        let t = text.trim();
        s.atoms.iter().any(|a| a.element == t).then(|| t.to_string())
    })
}

/// One status line for the atoms at `moved` in the candidate structure `s`:
/// how close they come to an atom that did not move, flagged when that is far
/// shorter than a bond, plus a note when one leaves the drawn range.
fn proximity(s: &Structure, moved: &[usize]) -> String {
    use crate::model::elements::get_covalent_radius;
    let esc = |t: &str| glib::markup_escape_text(t).to_string();
    let mut lines = Vec::new();

    let mut best: Option<(usize, usize, f64)> = None;
    for &i in moved {
        if let Some((j, d)) = atom_edit::nearest_atom(s, s.atoms[i].position, moved) {
            if best.map_or(true, |b| d < b.2) {
                best = Some((i, j, d));
            }
        }
    }
    if let Some((i, j, d)) = best {
        let (a, b) = (&s.atoms[i].element, &s.atoms[j].element);
        let text = format!("Nearest other atom: {b} at {d:.3} Å");
        if d < 0.5 * (get_covalent_radius(a) + get_covalent_radius(b)) {
            lines.push(format!("<span color='red'>{} (overlapping)</span>", esc(&text)));
        } else {
            lines.push(esc(&text));
        }
    }

    // The renderer draws an atom only within ±0.55 of the cell (its periodic
    // images bring it back once it is wrapped).
    if s.is_periodic {
        let outside = moved.iter().any(|&i| {
            cart_to_frac(s.atoms[i].position, s.lattice)
                .is_some_and(|f| f.iter().any(|x| !(-0.55..=1.55).contains(x)))
        });
        if outside {
            lines.push(
                "<span color='red'>Outside the drawn range of the cell; Basis → \
                 Standardize positions wraps it back in.</span>"
                    .into(),
            );
        }
    }
    lines.join("\n")
}

/// "2 Ti, 1 O" for the atoms at `indices`, in order of first appearance.
fn species_counts(s: &Structure, indices: &[usize]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for &i in indices {
        let el = s.atoms[i].element.as_str();
        match counts.iter_mut().find(|(e, _)| *e == el) {
            Some((_, n)) => *n += 1,
            None => counts.push((el, 1)),
        }
    }
    counts.iter().map(|(e, n)| format!("{n} {e}")).collect::<Vec<_>>().join(", ")
}

fn redraw_main(nb: &glib::WeakRef<Notebook>) {
    if let Some(da) = nb.upgrade().and_then(|nb| crate::ui::get_active_drawing_area(&nb)) {
        da.queue_draw();
    }
}

pub fn build(state: Rc<RefCell<AppState>>, main_notebook: &Notebook, preview: Preview) -> TabParts {
    let controls = GtkBox::new(Orientation::Vertical, 10);

    // --- Coordinates, shared by both cards ---
    let dd_coords = DropDown::from_strings(&["Fractional", "Cartesian (Å)"]);
    let coord_body = card_body(8);
    coord_body.append(&captioned("Coordinates", &dd_coords));
    let coord_note = Label::new(Some(
        "Fractional coordinates accept fractions such as 1/3. Each change is \
         one step of Edit → Undo.",
    ));
    coord_note.set_wrap(true);
    coord_note.set_xalign(0.0);
    coord_note.add_css_class("cview-caption");
    coord_body.append(&coord_note);
    controls.append(&group("Coordinates", &coord_body));

    // --- Add an atom ---
    let entry_el = Entry::new();
    entry_el.set_placeholder_text(Some("e.g. Li"));
    let (add_row, add_xyz) = coord_row();
    let dd_site = DropDown::from_strings(&["—"]);
    let site_box = captioned("Interstitial site (from Void Analysis)", &dd_site);
    let lbl_add = Label::new(None);
    lbl_add.set_wrap(true);
    lbl_add.set_xalign(0.0);
    lbl_add.add_css_class("cview-caption");
    let btn_add = Button::with_label("Add atom");
    btn_add.add_css_class("suggested-action");
    btn_add.set_halign(Align::End);

    let add_body = card_body(8);
    add_body.append(&captioned("Element", &entry_el));
    add_body.append(&site_box);
    add_body.append(&add_row);
    add_body.append(&lbl_add);
    add_body.append(&btn_add);
    controls.append(&group("Add an atom", &add_body));

    // --- Move the selected atoms ---
    let lbl_sel = Label::new(None);
    lbl_sel.set_halign(Align::Start);
    let dd_mode = DropDown::from_strings(&["Shift by", "Move to"]);
    let (move_row, move_xyz) = coord_row();
    let lbl_move = Label::new(None);
    lbl_move.set_wrap(true);
    lbl_move.set_xalign(0.0);
    lbl_move.add_css_class("cview-caption");
    let btn_move = Button::with_label("Move");
    btn_move.add_css_class("suggested-action");
    btn_move.set_halign(Align::End);

    let move_body = card_body(8);
    move_body.append(&lbl_sel);
    move_body.append(&captioned("Mode", &dd_mode));
    move_body.append(&move_row);
    move_body.append(&lbl_move);
    move_body.append(&btn_move);
    controls.append(&group("Move the selected atoms", &move_body));

    // --- Replace the selected atoms ---
    let entry_rep = Entry::new();
    entry_rep.set_placeholder_text(Some("e.g. Zr"));
    let lbl_rep = Label::new(None);
    lbl_rep.set_wrap(true);
    lbl_rep.set_xalign(0.0);
    lbl_rep.add_css_class("cview-caption");
    let btn_rep = Button::with_label("Replace");
    btn_rep.add_css_class("suggested-action");
    btn_rep.set_halign(Align::End);
    let rep_note = Label::new(Some(
        "To replace all or some atoms of one element, or to mix an element into \
         a site as a partial occupancy, use the Basis tab.",
    ));
    rep_note.set_wrap(true);
    rep_note.set_xalign(0.0);
    rep_note.add_css_class("cview-caption");

    let rep_body = card_body(8);
    rep_body.append(&captioned("New element", &entry_rep));
    rep_body.append(&lbl_rep);
    rep_body.append(&btn_rep);
    rep_body.append(&rep_note);
    controls.append(&group("Replace the selected atoms", &rep_body));

    let card = Rc::new(Cell::new(Card::Add));
    // Set while fields are filled in by code, so that does not count as the
    // user editing a card.
    let busy: Rc<Cell<bool>> = Rc::default();
    // Interstitial sites offered in the dropdown, fractional and Cartesian.
    let sites: Rc<RefCell<Vec<SitePos>>> = Rc::default();
    let is_frac = {
        let dd = dd_coords.clone();
        move || dd.selected() == 0
    };

    // Recompute both candidates, the status lines and the preview.
    let refresh: Rc<dyn Fn()> = {
        let state = state.clone();
        let preview = preview.clone();
        let (entry_el, add_xyz, move_xyz) = (entry_el.clone(), add_xyz.clone(), move_xyz.clone());
        let (lbl_add, lbl_move, lbl_sel) = (lbl_add.clone(), lbl_move.clone(), lbl_sel.clone());
        let (btn_add, btn_move, dd_mode) = (btn_add.clone(), btn_move.clone(), dd_mode.clone());
        let (entry_rep, lbl_rep, btn_rep) = (entry_rep.clone(), lbl_rep.clone(), btn_rep.clone());
        let card = card.clone();
        let is_frac = is_frac.clone();
        Rc::new(move || {
            let st = state.borrow();
            let Some(s) = st.active_tab().structure.as_ref() else {
                for l in [&lbl_add, &lbl_move, &lbl_rep] {
                    l.set_text("No structure loaded.");
                }
                lbl_sel.set_text("");
                btn_add.set_sensitive(false);
                btn_move.set_sensitive(false);
                btn_rep.set_sensitive(false);
                preview.set_candidate(None);
                return;
            };
            let to_cart = |v: [f64; 3]| if is_frac() { frac_to_cart(v, s.lattice) } else { v };

            // Add
            let add = match (element_of(&entry_el.text(), s), read(&add_xyz)) {
                (Some(el), Some(v)) => {
                    let c = atom_edit::add_atom(s, &el, to_cart(v));
                    lbl_add.set_markup(&proximity(&c, &[s.atoms.len()]));
                    Some((c, vec![s.atoms.len()]))
                }
                (None, _) if entry_el.text().trim().is_empty() => {
                    lbl_add.set_text("Type an element symbol.");
                    None
                }
                (None, _) => {
                    lbl_add.set_text("Not an element symbol.");
                    None
                }
                (_, None) => {
                    lbl_add.set_text("Enter three numbers.");
                    None
                }
            };
            btn_add.set_sensitive(add.is_some());

            // Move
            let sel = selected_indices(&st);
            lbl_sel.set_text(&format!("{} atom(s) selected in the main view", sel.len()));
            let mv = match (sel.len(), dd_mode.selected(), read(&move_xyz)) {
                (0, _, _) => {
                    lbl_move.set_text("Select the atoms to move in the main view.");
                    None
                }
                (_, _, None) => {
                    lbl_move.set_text("Enter three numbers.");
                    None
                }
                (1, MOVE_TO, Some(v)) => Some(atom_edit::place_atom(s, sel[0], to_cart(v))),
                (n, MOVE_TO, _) => {
                    lbl_move.set_text(&format!(
                        "Move to places one atom; {n} are selected. Use Shift by to move them together."
                    ));
                    None
                }
                (_, _, Some(v)) => Some(atom_edit::translate_atoms(s, &sel, to_cart(v))),
            };
            if let Some(c) = &mv {
                lbl_move.set_markup(&proximity(c, &sel));
            }
            btn_move.set_sensitive(mv.is_some());

            // Replace
            let rep_text = entry_rep.text();
            let rep = match (sel.is_empty(), element_of(&rep_text, s)) {
                (true, _) => {
                    lbl_rep.set_text("Select the atoms to replace in the main view.");
                    None
                }
                (false, None) => {
                    lbl_rep.set_text(if rep_text.trim().is_empty() {
                        "Type an element symbol."
                    } else {
                        "Not an element symbol."
                    });
                    None
                }
                (false, Some(el)) => {
                    let changing: Vec<usize> =
                        sel.iter().copied().filter(|&i| s.atoms[i].element != el).collect();
                    if changing.is_empty() {
                        lbl_rep.set_text(&format!("The selected atoms are already {el}."));
                        None
                    } else {
                        lbl_rep.set_text(&format!(
                            "{} → {el}",
                            species_counts(s, &changing)
                        ));
                        Some(crate::physics::operations::basis::modify_selection(s, &changing, &el))
                    }
                }
            };
            btn_rep.set_sensitive(rep.is_some());

            let (cand, hl) = match card.get() {
                Card::Add => add.map_or((None, vec![]), |(c, hl)| (Some(c), hl)),
                Card::Move => (mv, sel),
                Card::Replace => (rep, sel),
            };
            preview.set_candidate(cand);
            preview.set_highlight(hl.into_iter().collect(), false, None);
        })
    };

    // Fill the Move fields: the atom's position for Move to with one atom,
    // otherwise a zero shift.
    let prefill_move: Rc<dyn Fn()> = {
        let state = state.clone();
        let (move_xyz, dd_mode, busy) = (move_xyz.clone(), dd_mode.clone(), busy.clone());
        let is_frac = is_frac.clone();
        Rc::new(move || {
            let st = state.borrow();
            let sel = selected_indices(&st);
            let pos = match (st.active_tab().structure.as_ref(), sel.as_slice()) {
                (Some(s), [i]) if dd_mode.selected() == MOVE_TO => {
                    let p = s.atoms[*i].position;
                    Some(if is_frac() { cart_to_frac(p, s.lattice).unwrap_or(p) } else { p })
                }
                _ => None,
            };
            busy.set(true);
            write(&move_xyz, pos.unwrap_or([0.0; 3]));
            busy.set(false);
        })
    };

    // --- Signals ---
    let touched = |which: Card| {
        let (card, busy, refresh) = (card.clone(), busy.clone(), refresh.clone());
        move || {
            if !busy.get() {
                card.set(which);
                refresh();
            }
        }
    };
    for e in add_xyz.iter().chain(std::iter::once(&entry_el)) {
        let t = touched(Card::Add);
        e.connect_changed(move |_| t());
    }
    for e in &move_xyz {
        let t = touched(Card::Move);
        e.connect_changed(move |_| t());
    }
    {
        let t = touched(Card::Replace);
        entry_rep.connect_changed(move |_| t());
    }
    {
        let (t, prefill) = (touched(Card::Move), prefill_move.clone());
        dd_mode.connect_selected_notify(move |_| {
            prefill();
            t();
        });
    }
    {
        // Switching the coordinate system converts what is typed, so the
        // fields keep describing the same point (or the same shift: the
        // conversion is linear).
        let state = state.clone();
        let (add_xyz, move_xyz, busy, refresh) =
            (add_xyz.clone(), move_xyz.clone(), busy.clone(), refresh.clone());
        dd_coords.connect_selected_notify(move |dd| {
            let lattice = match state.borrow().active_tab().structure.as_ref() {
                Some(s) => s.lattice,
                None => return,
            };
            busy.set(true);
            for xyz in [&add_xyz, &move_xyz] {
                if let Some(v) = read(xyz) {
                    let w = if dd.selected() == 0 {
                        cart_to_frac(v, lattice).unwrap_or(v)
                    } else {
                        frac_to_cart(v, lattice)
                    };
                    write(xyz, w);
                }
            }
            busy.set(false);
            refresh();
        });
    }
    {
        let (sites, add_xyz, entry_el, busy) =
            (sites.clone(), add_xyz.clone(), entry_el.clone(), busy.clone());
        let state = state.clone();
        let (t, is_frac) = (touched(Card::Add), is_frac.clone());
        dd_site.connect_selected_notify(move |dd| {
            let i = dd.selected() as usize;
            let Some((frac, cart)) = i.checked_sub(1).and_then(|i| sites.borrow().get(i).copied()) else {
                return;
            };
            busy.set(true);
            write(&add_xyz, if is_frac() { frac } else { cart });
            // Offer the screened ion as the element when none is typed.
            if entry_el.text().trim().is_empty() {
                if let Some(ion) = state.borrow().active_tab().interstitial.as_ref().map(|o| o.ion.clone()) {
                    let sym: String = ion.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                    if let Some(el) = normalize_element(&sym) {
                        entry_el.set_text(&el);
                    }
                }
            }
            busy.set(false);
            t();
        });
    }

    // --- Actions ---
    let nb = main_notebook.downgrade();
    {
        let state = state.clone();
        let (entry_el, add_xyz, refresh, nb) = (entry_el.clone(), add_xyz.clone(), refresh.clone(), nb.clone());
        let is_frac = is_frac.clone();
        btn_add.connect_clicked(move |_| {
            {
                let mut st = state.borrow_mut();
                let tab = st.active_tab_mut();
                let Some(s) = &tab.structure else { return };
                let (Some(el), Some(v)) = (element_of(&entry_el.text(), s), read(&add_xyz)) else {
                    return;
                };
                let cart = if is_frac() { frac_to_cart(v, s.lattice) } else { v };
                let new = atom_edit::add_atom(s, &el, cart);
                let idx = s.atoms.len();
                let at = add_xyz.iter().map(|e| e.text().trim().to_string()).collect::<Vec<_>>().join(", ");
                let unit = if is_frac() { "" } else { " Å" };
                tab.commit(&format!("Added {el} at ({at}){unit}"), new, Edit::Geometry);
                // Keep the new atom picked, ready to be moved.
                st.select_atoms(&[idx]);
            }
            redraw_main(&nb);
            refresh();
        });
    }
    {
        let state = state.clone();
        let (move_xyz, dd_mode, refresh, nb) = (move_xyz.clone(), dd_mode.clone(), refresh.clone(), nb.clone());
        let is_frac = is_frac.clone();
        btn_move.connect_clicked(move |_| {
            {
                let mut st = state.borrow_mut();
                let sel = selected_indices(&st);
                let tab = st.active_tab_mut();
                let (Some(s), Some(v)) = (&tab.structure, read(&move_xyz)) else { return };
                let cart = if is_frac() { frac_to_cart(v, s.lattice) } else { v };
                let typed = move_xyz.iter().map(|e| e.text().trim().to_string()).collect::<Vec<_>>().join(", ");
                let unit = if is_frac() { "" } else { " Å" };
                let (new, op) = match (sel.as_slice(), dd_mode.selected()) {
                    ([], _) => return,
                    ([i], MOVE_TO) => (
                        atom_edit::place_atom(s, *i, cart),
                        format!("Moved {} to ({typed}){unit}", s.atoms[*i].element),
                    ),
                    (_, MOVE_TO) => return,
                    _ => (
                        atom_edit::translate_atoms(s, &sel, cart),
                        format!("Shifted {} atom(s) by ({typed}){unit}", sel.len()),
                    ),
                };
                tab.commit(&op, new, Edit::Geometry);
                // Moving changes the rendered instances; pick the same atoms
                // again so a shift can be repeated.
                st.select_atoms(&sel);
            }
            redraw_main(&nb);
            refresh();
        });
    }

    {
        let state = state.clone();
        let (entry_rep, refresh, nb) = (entry_rep.clone(), refresh.clone(), nb.clone());
        btn_rep.connect_clicked(move |_| {
            {
                let mut st = state.borrow_mut();
                let sel = selected_indices(&st);
                let tab = st.active_tab_mut();
                let Some(s) = &tab.structure else { return };
                let Some(el) = element_of(&entry_rep.text(), s) else { return };
                let changing: Vec<usize> =
                    sel.iter().copied().filter(|&i| s.atoms[i].element != el).collect();
                if changing.is_empty() {
                    return;
                }
                let op = format!("Replaced {} → {el}", species_counts(s, &changing));
                let new = crate::physics::operations::basis::modify_selection(s, &changing, &el);
                // Same atoms at the same places: the selection is kept.
                tab.commit(&op, new, Edit::Species);
            }
            redraw_main(&nb);
            refresh();
        });
    }

    // The selection and the structure change in the main window, which this
    // window cannot hear; poll while the tab is on screen.
    {
        let weak = lbl_sel.downgrade();
        let state = state.clone();
        let (refresh, prefill) = (refresh.clone(), prefill_move.clone());
        let mut last = 0u64;
        glib::timeout_add_local(Duration::from_millis(400), move || {
            let Some(lbl) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if lbl.is_mapped() {
                let fp = {
                    let st = state.borrow();
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    st.active_tab_index.hash(&mut h);
                    let sel = selected_indices(&st);
                    sel.hash(&mut h);
                    if let Some(s) = &st.active_tab().structure {
                        s.atoms.len().hash(&mut h);
                        for &i in &sel {
                            s.atoms[i].position.map(f64::to_bits).hash(&mut h);
                        }
                    }
                    h.finish()
                };
                if fp != last {
                    last = fp;
                    prefill();
                    refresh();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // On entering: offer the tab's interstitial sites, force Cartesian for a
    // structure with no cell, and refresh.
    let on_enter: Box<dyn Fn()> = {
        let state = state.clone();
        let (dd_site, site_box, dd_coords) = (dd_site.clone(), site_box.clone(), dd_coords.clone());
        let (sites, busy, refresh) = (sites.clone(), busy.clone(), refresh.clone());
        Box::new(move || {
            {
                let st = state.borrow();
                let tab = st.active_tab();
                let periodic = tab.structure.as_ref().map_or(true, |s| s.is_periodic);
                busy.set(true);
                if !periodic {
                    dd_coords.set_selected(1);
                }
                dd_coords.set_sensitive(periodic);

                let mut list = sites.borrow_mut();
                list.clear();
                let mut names = vec!["—".to_string()];
                if let Some(o) = &tab.interstitial {
                    for (k, site) in o.sites.iter().enumerate() {
                        list.push((site.frac, site.cart));
                        let f = site.frac.map(|x| format!("{:.3}", x)).join(" ");
                        names.push(format!("{} site {}: {} (r {:.2} Å)", o.ion, k + 1, f, site.radius));
                    }
                }
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                dd_site.set_model(Some(&gtk4::StringList::new(&names)));
                dd_site.set_selected(0);
                site_box.set_visible(!list.is_empty());
                busy.set(false);
            }
            refresh();
        })
    };

    TabParts { controls, on_enter }
}
