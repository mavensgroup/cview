// src/ui/dialogs/atom_instances_dlg.rs
//
// "Atom Instances" — per-atom render overrides for distinguishing
// inequivalent sites (e.g. Fe1 vs Fe2 in Fe3O4) without touching the
// underlying structure or any IO format. Overrides are session-only.

use super::preview::Preview;
use super::TabParts;
use crate::state::AppState;
use crate::ui::style::{captioned, card_body, group, surface};
use crate::utils::linalg::cart_to_frac;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, ColorButton, DropDown, Entry, Label, ListBox, ListBoxRow,
    Notebook, Orientation, PolicyType, ScrolledWindow, SelectionMode, StringList,
};
use std::collections::HashSet;
use std::cell::RefCell;
use std::rc::Rc;

/// Build one row showing index, element, fractional position, current label
/// (if any), and a colored swatch reflecting the current effective color.
fn build_row(
    atom_idx: usize,
    element: &str,
    frac: [f64; 3],
    label: Option<&str>,
    color: (f64, f64, f64),
    has_override: bool,
) -> ListBoxRow {
    let row = ListBoxRow::new();

    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_start(6);
    hbox.set_margin_end(6);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);

    // Index column
    let lbl_idx = Label::new(Some(&format!("#{}", atom_idx)));
    lbl_idx.set_width_chars(5);
    lbl_idx.set_xalign(0.0);
    lbl_idx.set_opacity(0.65);
    hbox.append(&lbl_idx);

    // Element column
    let lbl_el = Label::new(Some(element));
    lbl_el.set_width_chars(4);
    lbl_el.set_xalign(0.0);
    hbox.append(&lbl_el);

    // Fractional position
    let lbl_pos = Label::new(Some(&format!(
        "({:6.3}, {:6.3}, {:6.3})",
        frac[0], frac[1], frac[2]
    )));
    lbl_pos.set_xalign(0.0);
    lbl_pos.set_opacity(0.75);
    hbox.append(&lbl_pos);

    // Label (display text). Bold + colored when overridden, dimmed when not.
    let label_text = label.unwrap_or("—");
    let lbl_label = Label::new(Some(label_text));
    lbl_label.set_width_chars(8);
    lbl_label.set_xalign(0.0);
    if has_override && label.is_some() {
        lbl_label.add_css_class("heading");
    } else {
        lbl_label.set_opacity(0.4);
    }
    hbox.append(&lbl_label);

    // Color swatch — small read-only color indicator
    let swatch = gtk4::DrawingArea::new();
    swatch.set_size_request(28, 18);
    swatch.set_valign(Align::Center);
    let r = color.0;
    let g = color.1;
    let b = color.2;
    swatch.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(r, g, b);
        cr.rectangle(0.0, 0.0, w as f64, h as f64);
        let _ = cr.fill_preserve();
        cr.set_source_rgb(0.2, 0.2, 0.2);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    });
    hbox.append(&swatch);

    row.set_child(Some(&hbox));
    // Stash the atom index on the row for retrieval during multi-select apply.
    // The widget name is unused for CSS here, so it's a safe place to carry the
    // index (row position can't be used — a filter shows only a subset).
    row.set_widget_name(&atom_idx.to_string());
    row
}

/// Returns (atoms-snapshot, lattice). Empty list if no structure is loaded.
fn snapshot_atoms(
    state: &AppState,
) -> (
    Vec<(usize, String, [f64; 3], Option<String>, (f64, f64, f64), bool)>,
    [[f64; 3]; 3],
) {
    let tab = state.active_tab();
    let lattice = tab
        .structure
        .as_ref()
        .map(|s| s.lattice)
        .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    let atoms = match &tab.structure {
        Some(s) => s
            .atoms
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let frac = cart_to_frac(a.position, lattice).unwrap_or(a.position);
                let ovr = tab.overrides.get(&i);
                let label = ovr.and_then(|o| o.display_label.clone());
                let has = ovr.map(|o| !o.is_empty()).unwrap_or(false);
                let default_rgb =
                    crate::model::elements::get_element_color(&a.element, state.config.color_scheme);
                let color = ovr
                    .and_then(|o| o.color)
                    .or_else(|| tab.style.element_colors.get(&a.element).copied())
                    .unwrap_or(default_rgb);
                (i, a.element.clone(), frac, label, color, has)
            })
            .collect(),
        None => vec![],
    };
    (atoms, lattice)
}

/// (re)populate the ListBox with current atom state. Optional `filter` keeps
/// only atoms whose element matches.
fn populate_list(list: &ListBox, state: &AppState, filter: Option<&str>) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let (atoms, _lat) = snapshot_atoms(state);
    for (idx, el, frac, label, color, has) in atoms {
        if let Some(f) = filter {
            if f != "All" && el != f {
                continue;
            }
        }
        let row = build_row(idx, &el, frac, label.as_deref(), color, has);
        list.append(&row);
    }
}

/// Atom indices of the rows currently selected in the list.
fn selected_indices(list: &ListBox) -> HashSet<usize> {
    list.selected_rows()
        .iter()
        .filter_map(|r| r.widget_name().parse::<usize>().ok())
        .collect()
}

/// Element names for the filter dropdown: "All" plus the structure's elements.
fn filter_options(state: &AppState) -> Vec<String> {
    let mut out = vec!["All".to_string()];
    if let Some(s) = &state.active_tab().structure {
        let mut elems: Vec<String> = s.atoms.iter().map(|a| a.element.clone()).collect();
        elems.sort();
        elems.dedup();
        out.extend(elems);
    }
    out
}

pub fn build(
    state: Rc<RefCell<AppState>>,
    main_notebook: &Notebook,
    preview: Preview,
) -> TabParts {
    let controls = GtkBox::new(Orientation::Vertical, 10);
    let notebook = main_notebook;

    // ---------- Filter ----------
    let filter_strs: Rc<RefCell<Vec<String>>> =
        Rc::new(RefCell::new(filter_options(&state.borrow())));
    let filter_dd = DropDown::new(
        Some(StringList::new(
            &filter_strs.borrow().iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        )),
        None::<&gtk4::Expression>,
    );
    filter_dd.set_selected(0);
    filter_dd.set_hexpand(true);

    // ---------- Label + color editor ----------
    let entry_label = Entry::new();
    entry_label.set_placeholder_text(Some("e.g. Fe1"));
    entry_label.set_hexpand(true);
    let btn_color = ColorButton::new();
    btn_color.set_rgba(&gdk::RGBA::new(0.85, 0.55, 0.20, 1.0));

    let row_edit = GtkBox::new(Orientation::Horizontal, 8);
    row_edit.append(&captioned("Label", &entry_label));
    row_edit.append(&captioned("Color", &btn_color));

    let btn_apply = Button::with_label("Apply to selected");
    btn_apply.add_css_class("suggested-action");
    let btn_reset = Button::with_label("Reset selected");
    let row_btns = GtkBox::new(Orientation::Horizontal, 8);
    row_btns.set_homogeneous(true);
    row_btns.append(&btn_reset);
    row_btns.append(&btn_apply);

    let edit_body = card_body(8);
    edit_body.append(&captioned("Show elements", &filter_dd));
    edit_body.append(&row_edit);
    edit_body.append(&row_btns);
    controls.append(&group("Mark inequivalent sites", &edit_body));

    // ---------- List of atoms ----------
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .min_content_height(160)
        .build();
    scroll.add_css_class("cview-text");
    scroll.set_overflow(gtk4::Overflow::Hidden);

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Multiple);
    populate_list(&list, &state.borrow(), None);
    scroll.set_child(Some(&list));
    let list_body = card_body(8);
    list_body.set_vexpand(true);
    list_body.append(&scroll);
    let list_card = surface(&list_body);
    list_card.set_vexpand(true);
    controls.append(&list_card);

    // ---------- Status + help ----------
    let status = Label::new(Some(""));
    status.set_xalign(0.0);
    status.set_opacity(0.7);
    controls.append(&status);

    let help = Label::new(Some(
        "Ctrl/Shift-click to multi-select. Labels and colors mark inequivalent \
         sites (e.g. Fe1, Fe2) for display only; they are not written to CIF/POSCAR.",
    ));
    help.set_wrap(true);
    help.set_xalign(0.0);
    help.add_css_class("cview-caption");
    controls.append(&help);

    // Current filter element name ("All" = none).
    let current_filter = {
        let dd = filter_dd.clone();
        let strs = filter_strs.clone();
        move || -> Option<String> {
            strs.borrow().get(dd.selected() as usize).cloned()
        }
    };

    // ---------- Filter wiring ----------
    {
        let state = state.clone();
        let list = list.clone();
        let cf = current_filter.clone();
        filter_dd.connect_selected_notify(move |_| {
            populate_list(&list, &state.borrow(), cf().as_deref());
        });
    }

    // ---------- Highlight rows in the preview ----------
    {
        let preview = preview.clone();
        list.connect_selected_rows_changed(move |l| {
            preview.set_highlight(selected_indices(l), false, None);
        });
    }

    // ---------- Apply ----------
    {
        let state = state.clone();
        let list = list.clone();
        let nb = notebook.downgrade();
        let entry = entry_label.clone();
        let color = btn_color.clone();
        let status = status.clone();
        let cf = current_filter.clone();
        btn_apply.connect_clicked(move |_| {
            let indices = selected_indices(&list);
            if indices.is_empty() {
                status.set_text("Select one or more atoms first.");
                return;
            }
            let label = Some(entry.text().to_string()).filter(|l| !l.is_empty());
            let c = color.rgba();
            let rgb = (c.red() as f64, c.green() as f64, c.blue() as f64);
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                for idx in &indices {
                    let o = tab.overrides.entry(*idx).or_default();
                    if let Some(l) = &label {
                        o.display_label = Some(l.clone());
                    }
                    o.color = Some(rgb);
                }
                // The vector path draws overridden atoms, so the sprite cache
                // needs no invalidation.
            }
            populate_list(&list, &state.borrow(), cf().as_deref());
            if let Some(nb) = nb.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
            status.set_text(&format!("Applied to {} atom(s).", indices.len()));
        });
    }

    // ---------- Reset ----------
    {
        let state = state.clone();
        let list = list.clone();
        let nb = notebook.downgrade();
        let status = status.clone();
        let cf = current_filter.clone();
        btn_reset.connect_clicked(move |_| {
            let indices = selected_indices(&list);
            if indices.is_empty() {
                status.set_text("Select one or more atoms first.");
                return;
            }
            let mut count = 0usize;
            {
                let mut s = state.borrow_mut();
                let tab = s.active_tab_mut();
                for idx in &indices {
                    if tab.overrides.remove(idx).is_some() {
                        count += 1;
                    }
                }
            }
            populate_list(&list, &state.borrow(), cf().as_deref());
            if let Some(nb) = nb.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }
            status.set_text(&format!("Cleared override on {} atom(s).", count));
        });
    }

    // ---------- Refresh when the tab is shown ----------
    // The structure may have changed on another tab (supercell, substitution),
    // so rebuild the filter choices and the rows.
    let on_enter = {
        let state = state.clone();
        let list = list.clone();
        let dd = filter_dd.clone();
        let strs = filter_strs.clone();
        Box::new(move || {
            let opts = filter_options(&state.borrow());
            dd.set_model(Some(&StringList::new(
                &opts.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            )));
            *strs.borrow_mut() = opts;
            dd.set_selected(0);
            populate_list(&list, &state.borrow(), None);
            preview.set_candidate(None);
            preview.set_highlight(selected_indices(&list), false, None);
        }) as Box<dyn Fn()>
    };

    TabParts { controls, on_enter }
}
