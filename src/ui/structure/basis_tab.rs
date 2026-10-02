// src/ui/structure/basis_tab.rs
//
// Chemistry edits: change the selected atoms, replace an element everywhere,
// standardize positions. The preview highlights what an action would touch.

use super::preview::Preview;
use super::TabParts;
use crate::physics::operations::basis;
use crate::state::AppState;
use crate::ui::style::{captioned, card_body, group};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Entry, Label, Notebook, Orientation};
use std::cell::RefCell;
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

    // --- Replace everywhere ---
    let entry_find = Entry::new();
    entry_find.set_placeholder_text(Some("e.g. Si"));
    entry_find.set_hexpand(true);
    let entry_replace = Entry::new();
    entry_replace.set_placeholder_text(Some("e.g. Ge"));
    entry_replace.set_hexpand(true);
    let btn_sub = Button::with_label("Replace all");

    let row_sub = GtkBox::new(Orientation::Horizontal, 8);
    row_sub.set_homogeneous(true);
    row_sub.append(&captioned("Find", &entry_find));
    row_sub.append(&captioned("Replace with", &entry_replace));
    let sub_body = card_body(8);
    sub_body.append(&row_sub);
    sub_body.append(&btn_sub);
    controls.append(&group("Replace element everywhere", &sub_body));

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

    // --- Refresh: selection count + preview highlight ---
    let refresh: Rc<dyn Fn()> = {
        let state = state.clone();
        let lbl = lbl_count.clone();
        let btn = btn_change.clone();
        let find = entry_find.clone();
        let preview = preview.clone();
        Rc::new(move || {
            let st = state.borrow();
            let n = selected_original_indices(&st).len();
            lbl.set_text(&format!("{} atom(s) selected in the main view", n));
            btn.set_sensitive(n > 0);

            // Highlight the selection, and the element named in "Find" if the
            // structure has it.
            let find_el = find.text().to_string();
            let element = st
                .active_tab()
                .structure
                .as_ref()
                .filter(|s| !find_el.is_empty() && s.atoms.iter().any(|a| a.element == find_el))
                .map(|_| find_el);
            preview.set_highlight(HashSet::new(), true, element);
        })
    };
    {
        let r = refresh.clone();
        entry_find.connect_changed(move |_| r());
    }
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
        let refresh = refresh.clone();
        btn_sub.connect_clicked(move |_| {
            let (from, to) = (find.text().to_string(), repl.text().to_string());
            if from.is_empty() || to.is_empty() {
                return;
            }
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                if let Some(current) = &tab.structure {
                    let new_s = basis::substitute_element(current, &from, &to);
                    tab.structure = Some(new_s);
                    tab.invalidate_derived();
                    if let Some(s) = &tab.structure {
                        crate::utils::console::structure_changed(
                            &format!("Element substitution {} → {}", from, to),
                            s,
                        );
                    }
                }
            }
            find.set_text("");
            redraw_main(&nb);
            refresh();
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
