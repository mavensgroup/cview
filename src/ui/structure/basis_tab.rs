// src/ui/structure/basis_tab.rs
//
// Chemistry edits: change the selected atoms, replace an element (all of it,
// or only some of it, for partial substitution and disordered models),
// standardize positions. The preview highlights what an action would touch.

use super::preview::Preview;
use super::TabParts;
use crate::physics::operations::basis;
use crate::state::AppState;
use crate::ui::style::{captioned, card_body, group};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, Entry, Label, Notebook, Orientation, SpinButton,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

/// Unique base-atom indices of the current selection. Selection is keyed by
/// per-instance `unique_id`, so several ghost copies can map to the same
/// `original_index`; dedupe before passing to basis ops.
fn selected_original_indices(state: &AppState) -> Vec<usize> {
    let mut result: Vec<usize> = state
        .active_tab()
        .interaction
        .selected
        .values()
        .map(|s| s.original_index)
        .collect();
    result.sort_unstable();
    result.dedup();
    result
}

fn redraw_main(nb: &gtk4::glib::WeakRef<Notebook>) {
    if let Some(nb) = nb.upgrade() {
        if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
            da.queue_draw();
        }
    }
}

/// Number of atoms of `element` in the active structure.
fn count_element(state: &AppState, element: &str) -> usize {
    state
        .active_tab()
        .structure
        .as_ref()
        .map(|s| s.atoms.iter().filter(|a| a.element == element).count())
        .unwrap_or(0)
}

pub fn build(
    state: Rc<RefCell<AppState>>,
    main_notebook: &Notebook,
    preview: Preview,
) -> TabParts {
    let controls = GtkBox::new(Orientation::Vertical, 10);

    // --- Selected atoms ---
    let lbl_count = Label::new(None);
    lbl_count.set_halign(Align::Start);
    let entry_el = Entry::new();
    entry_el.set_placeholder_text(Some("e.g. Au"));
    entry_el.set_hexpand(true);
    let btn_change = Button::with_label("Change element");

    let row_change = GtkBox::new(Orientation::Horizontal, 8);
    row_change.append(&captioned("New element", &entry_el));
    btn_change.set_valign(Align::End);
    row_change.append(&btn_change);

    let sel_body = card_body(8);
    sel_body.append(&lbl_count);
    sel_body.append(&row_change);
    controls.append(&group("Selected atoms", &sel_body));

    // --- Replace an element: all of it, or some of it ---
    let entry_find = Entry::new();
    entry_find.set_placeholder_text(Some("e.g. O"));
    entry_find.set_hexpand(true);
    let entry_replace = Entry::new();
    entry_replace.set_placeholder_text(Some("e.g. N"));
    entry_replace.set_hexpand(true);

    let row_sub = GtkBox::new(Orientation::Horizontal, 8);
    row_sub.set_homogeneous(true);
    row_sub.append(&captioned("Find", &entry_find));
    row_sub.append(&captioned("Replace with", &entry_replace));

    // How many of the matching atoms to replace; all by default.
    let spin_count = SpinButton::with_range(1.0, 1.0, 1.0);
    spin_count.set_sensitive(false);
    let lbl_of = Label::new(Some("of 0"));
    lbl_of.set_halign(Align::Start);
    lbl_of.add_css_class("cview-caption");
    let row_count = GtkBox::new(Orientation::Horizontal, 8);
    let count_box = GtkBox::new(Orientation::Vertical, 4);
    count_box.append(&spin_count);
    row_count.append(&captioned("Atoms to replace", &count_box));
    lbl_of.set_valign(Align::End);
    lbl_of.set_margin_bottom(8);
    row_count.append(&lbl_of);

    let check_random = CheckButton::with_label("Choose at random (disordered model)");
    check_random.set_sensitive(false);
    let btn_repick = Button::with_label("Pick again");
    btn_repick.set_sensitive(false);
    let btn_sub = Button::with_label("Replace");
    btn_sub.set_sensitive(false);

    let row_rand = GtkBox::new(Orientation::Horizontal, 8);
    row_rand.append(&check_random);
    btn_repick.set_halign(Align::End);
    btn_repick.set_hexpand(true);
    row_rand.append(&btn_repick);

    let sub_note = Label::new(Some(
        "Replacing fewer than all gives a partial substitution, e.g. one O of \
         three. The atoms it will change are ringed in the preview.",
    ));
    sub_note.set_wrap(true);
    sub_note.set_xalign(0.0);
    sub_note.add_css_class("cview-caption");

    let sub_body = card_body(8);
    sub_body.append(&row_sub);
    sub_body.append(&row_count);
    sub_body.append(&row_rand);
    sub_body.append(&sub_note);
    sub_body.append(&btn_sub);
    controls.append(&group("Replace an element", &sub_body));

    // --- Positions ---
    let btn_wrap = Button::with_label("Standardize positions [0, 1)");
    let wrap_note = Label::new(Some(
        "Wraps fractional coordinates into the unit cell. This is not IUCr \
         standardization; use Toggle Primitive/Conventional for that.",
    ));
    wrap_note.set_wrap(true);
    wrap_note.set_xalign(0.0);
    wrap_note.add_css_class("cview-caption");
    let pos_body = card_body(8);
    pos_body.append(&wrap_note);
    pos_body.append(&btn_wrap);
    controls.append(&group("Positions", &pos_body));

    // --- The partial-replacement picks, shared by the preview and the action ---
    // Atom indices the Replace button will change, and the seed behind a random
    // choice (changed by "Pick again").
    let picks: Rc<RefCell<Vec<usize>>> = Rc::default();
    let seed: Rc<Cell<u64>> = Rc::new(Cell::new(0x5EED_0001));
    // Re-entrancy guard: setting the spin's range/value fires its handler.
    let busy: Rc<Cell<bool>> = Rc::default();

    // Recompute everything derived from the Find text, the count and the
    // random choice, then update the controls and the preview.
    let refresh_replace: Rc<dyn Fn()> = {
        let state = state.clone();
        let (find, repl) = (entry_find.clone(), entry_replace.clone());
        let (spin, lbl_of) = (spin_count.clone(), lbl_of.clone());
        let (rand, repick, sub) = (check_random.clone(), btn_repick.clone(), btn_sub.clone());
        let (picks, seed, busy) = (picks.clone(), seed.clone(), busy.clone());
        let preview = preview.clone();
        Rc::new(move || {
            if busy.get() {
                return;
            }
            busy.set(true);
            let st = state.borrow();
            let from = find.text().to_string();
            let m = if from.is_empty() { 0 } else { count_element(&st, &from) };

            // The count spin runs 1..=m and rests at m ("all").
            spin.set_range(1.0, m.max(1) as f64);
            let want = (spin.value().round() as usize).clamp(1, m.max(1));
            lbl_of.set_text(&format!("of {m}"));
            spin.set_sensitive(m > 1);
            rand.set_sensitive(m > 1);

            let n = if m == 0 { 0 } else { want };
            let partial = n > 0 && n < m;
            repick.set_sensitive(partial && rand.is_active());
            sub.set_sensitive(m > 0 && !repl.text().is_empty());

            let new_picks = match (&st.active_tab().structure, m) {
                (Some(s), m) if m > 0 => basis::pick_atoms(
                    s,
                    &from,
                    n,
                    rand.is_active().then(|| seed.get()),
                ),
                _ => Vec::new(),
            };
            let hl: HashSet<usize> = new_picks.iter().copied().collect();
            *picks.borrow_mut() = new_picks;
            // Ring the atoms that will change (all of the element when
            // replacing everything), plus anything selected in the main view.
            preview.set_highlight(hl, true, None);
            busy.set(false);
        })
    };
    {
        let (r, spin) = (refresh_replace.clone(), spin_count.clone());
        let (find, busy) = (entry_find.clone(), busy.clone());
        // A new Find element starts at "all of them".
        let state = state.clone();
        entry_find.connect_changed(move |_| {
            let m = count_element(&state.borrow(), &find.text());
            busy.set(true);
            spin.set_range(1.0, m.max(1) as f64);
            spin.set_value(m.max(1) as f64);
            busy.set(false);
            r();
        });
    }
    {
        let r = refresh_replace.clone();
        entry_replace.connect_changed(move |_| r());
    }
    {
        let r = refresh_replace.clone();
        spin_count.connect_value_changed(move |_| r());
    }
    {
        let r = refresh_replace.clone();
        check_random.connect_toggled(move |_| r());
    }
    {
        let (r, seed) = (refresh_replace.clone(), seed.clone());
        btn_repick.connect_clicked(move |_| {
            seed.set(seed.get().wrapping_add(1));
            r();
        });
    }

    // --- Refresh: selection count + selection-change highlight ---
    let refresh: Rc<dyn Fn()> = {
        let state = state.clone();
        let lbl = lbl_count.clone();
        let btn = btn_change.clone();
        let rr = refresh_replace.clone();
        Rc::new(move || {
            let n = selected_original_indices(&state.borrow()).len();
            lbl.set_text(&format!("{} atom(s) selected in the main view", n));
            btn.set_sensitive(n > 0);
            // The preview highlight depends on both the selection and the
            // replace picks; rebuild it from the latter.
            rr();
        })
    };
    // The selection changes in the main window, which this window cannot hear;
    // poll like the preview does. Stops when the tab is destroyed.
    {
        let r = refresh.clone();
        let weak = lbl_count.downgrade();
        let last = Rc::new(RefCell::new(usize::MAX));
        let state = state.clone();
        glib::timeout_add_local(Duration::from_millis(400), move || {
            let Some(lbl) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            // Only while this tab is on screen.
            if lbl.is_mapped() {
                let n = selected_original_indices(&state.borrow()).len();
                if n != *last.borrow() {
                    *last.borrow_mut() = n;
                    r();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // --- Actions ---
    {
        let state = state.clone();
        let nb = main_notebook.downgrade();
        let entry = entry_el.clone();
        let refresh = refresh.clone();
        btn_change.connect_clicked(move |_| {
            let new_el = entry.text().to_string();
            if new_el.is_empty() {
                return;
            }
            let indices = selected_original_indices(&state.borrow());
            if indices.is_empty() {
                return;
            }
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                if let Some(current) = &tab.structure {
                    let new_s = basis::modify_selection(current, &indices, &new_el);
                    tab.structure = Some(new_s);
                    tab.invalidate_derived();
                    crate::utils::console::log_info(&format!(
                        "Changed {} atom(s) to {}",
                        indices.len(),
                        new_el
                    ));
                }
            }
            redraw_main(&nb);
            refresh();
        });
    }
    {
        let state = state.clone();
        let nb = main_notebook.downgrade();
        let (find, repl) = (entry_find.clone(), entry_replace.clone());
        let (picks, seed) = (picks.clone(), seed.clone());
        let rr = refresh_replace.clone();
        btn_sub.connect_clicked(move |_| {
            let (from, to) = (find.text().to_string(), repl.text().to_string());
            let chosen = picks.borrow().clone();
            if from.is_empty() || to.is_empty() || chosen.is_empty() {
                return;
            }
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                if let Some(current) = &tab.structure {
                    let total = current.atoms.iter().filter(|a| a.element == from).count();
                    let new_s = basis::modify_selection(current, &chosen, &to);
                    tab.structure = Some(new_s);
                    tab.invalidate_derived();
                    let what = if chosen.len() == total {
                        format!("Element substitution {} → {}", from, to)
                    } else {
                        format!(
                            "Partial substitution: {} of {} {} → {}",
                            chosen.len(),
                            total,
                            from,
                            to
                        )
                    };
                    if let Some(s) = &tab.structure {
                        crate::utils::console::structure_changed(&what, s);
                    }
                }
            }
            // The next pick is a fresh draw.
            seed.set(seed.get().wrapping_add(1));
            redraw_main(&nb);
            rr();
        });
    }
    {
        let state = state.clone();
        let nb = main_notebook.downgrade();
        btn_wrap.connect_clicked(move |_| {
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                if let Some(current) = &tab.structure {
                    let new_s = basis::standardize_positions(current);
                    tab.structure = Some(new_s);
                    tab.invalidate_derived();
                    if let Some(s) = &tab.structure {
                        crate::utils::console::structure_changed(
                            "Standardize positions [0, 1)",
                            s,
                        );
                    }
                }
            }
            redraw_main(&nb);
        });
    }

    refresh();
    TabParts {
        controls,
        on_enter: Box::new(move || {
            preview.set_candidate(None);
            refresh();
        }),
    }
}
