// src/ui/dialogs/atom_instances_dlg.rs
//
// "Atom Instances" — per-atom render overrides for distinguishing
// inequivalent sites (e.g. Fe1 vs Fe2 in Fe3O4) without touching the
// underlying structure or any IO format. Overrides are session-only.

use crate::state::AppState;
use crate::utils::linalg::cart_to_frac;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, ColorButton, Dialog, DropDown, Entry, Label, ListBox,
    ListBoxRow, Notebook, Orientation, PolicyType, ResponseType, ScrolledWindow, SelectionMode,
    Separator, Window,
};
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
    unsafe { row.set_data::<usize>("atom_idx", atom_idx) };
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

pub fn show(parent: &impl IsA<Window>, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    let dialog = Dialog::builder()
        .title("Atom Instances")
        .transient_for(parent)
        .modal(false)
        .default_width(560)
        .default_height(520)
        .build();

    let content = dialog.content_area();
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    // ---------- Toolbar: filter + label/color editor + apply/reset ----------
    let toolbar = GtkBox::new(Orientation::Horizontal, 8);

    // Element filter
    let mut filter_strs: Vec<String> = vec!["All".to_string()];
    if let Some(s) = &state.borrow().active_tab().structure {
        let mut elems: Vec<String> = s.atoms.iter().map(|a| a.element.clone()).collect();
        elems.sort();
        elems.dedup();
        filter_strs.extend(elems);
    }
    let filter_refs: Vec<&str> = filter_strs.iter().map(|s| s.as_str()).collect();
    let filter_dd = DropDown::from_strings(&filter_refs);
    filter_dd.set_selected(0);

    toolbar.append(&Label::new(Some("Filter:")));
    toolbar.append(&filter_dd);
    toolbar.append(&Separator::new(Orientation::Vertical));

    // Label entry + Color picker + Apply / Reset
    let entry_label = Entry::new();
    entry_label.set_placeholder_text(Some("Label (e.g. Fe1)"));
    entry_label.set_width_chars(10);

    let btn_color = ColorButton::new();
    btn_color.set_rgba(&gdk::RGBA::new(0.85, 0.55, 0.20, 1.0));

    let btn_apply = Button::with_label("Apply to Selected");
    let btn_reset = Button::with_label("Reset Selected");

    toolbar.append(&Label::new(Some("Label:")));
    toolbar.append(&entry_label);
    toolbar.append(&Label::new(Some("Color:")));
    toolbar.append(&btn_color);
    toolbar.append(&btn_apply);
    toolbar.append(&btn_reset);

    content.append(&toolbar);
    content.append(&Separator::new(Orientation::Horizontal));

    // ---------- List of atoms ----------
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .build();

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Multiple);
    populate_list(&list, &state.borrow(), None);
    scroll.set_child(Some(&list));
    content.append(&scroll);

    // ---------- Status bar ----------
    let status = Label::new(Some(""));
    status.set_xalign(0.0);
    status.set_opacity(0.7);
    status.set_margin_top(6);
    content.append(&status);

    // ---------- Help text ----------
    let help = Label::new(Some(
        "Tip: Ctrl/Shift-click to multi-select. Use \"Apply to Selected\" to mark \
         inequivalent sites (e.g. Fe1, Fe2) — the change affects display only and \
         is not written to CIF/POSCAR.",
    ));
    help.set_wrap(true);
    help.set_xalign(0.0);
    help.set_opacity(0.6);
    help.set_margin_top(4);
    content.append(&help);

    // ---------- Filter dropdown wiring ----------
    {
        let state_w = Rc::downgrade(&state);
        let list_w = list.downgrade();
        let strs = filter_strs.clone();
        filter_dd.connect_selected_notify(move |dd| {
            if let (Some(st), Some(ls)) = (state_w.upgrade(), list_w.upgrade()) {
                let idx = dd.selected() as usize;
                let f = strs.get(idx).map(|s| s.as_str());
                populate_list(&ls, &st.borrow(), f);
            }
        });
    }

    // ---------- Apply button wiring ----------
    {
        let state_w = Rc::downgrade(&state);
        let list_w = list.downgrade();
        let nb_w = notebook.downgrade();
        let entry_w = entry_label.clone();
        let btn_color_w = btn_color.clone();
        let filter_dd_w = filter_dd.clone();
        let strs = filter_strs.clone();
        let status_w = status.clone();

        btn_apply.connect_clicked(move |_| {
            let Some(st) = state_w.upgrade() else {
                return;
            };
            let Some(ls) = list_w.upgrade() else {
                return;
            };

            let selected_rows = ls.selected_rows();
            if selected_rows.is_empty() {
                status_w.set_text("Select one or more atoms first.");
                return;
            }

            let label_text = entry_w.text().to_string();
            let label_opt = if label_text.is_empty() {
                None
            } else {
                Some(label_text)
            };
            let c = btn_color_w.rgba();
            let color_tuple = (c.red() as f64, c.green() as f64, c.blue() as f64);

            let mut count = 0usize;
            {
                let mut s = st.borrow_mut();
                let tab = s.active_tab_mut();
                for row in &selected_rows {
                    let idx = unsafe { row.data::<usize>("atom_idx") };
                    let Some(idx) = idx else {
                        continue;
                    };
                    let idx = unsafe { *idx.as_ref() };
                    let entry = tab.overrides.entry(idx).or_default();
                    if let Some(ref l) = label_opt {
                        entry.display_label = Some(l.clone());
                    }
                    entry.color = Some(color_tuple);
                    count += 1;
                }
                // Vector path is used for overridden atoms, so sprite cache
                // doesn't need invalidation. But element-keyed sprites for
                // non-overridden atoms are unaffected — leave them.
            }

            // Refresh list with the same filter.
            let f_idx = filter_dd_w.selected() as usize;
            let f = strs.get(f_idx).map(|s| s.as_str());
            populate_list(&ls, &st.borrow(), f);

            // Redraw main view.
            if let Some(nb) = nb_w.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }

            status_w.set_text(&format!("Applied to {} atom(s).", count));
        });
    }

    // ---------- Reset button wiring ----------
    {
        let state_w = Rc::downgrade(&state);
        let list_w = list.downgrade();
        let nb_w = notebook.downgrade();
        let filter_dd_w = filter_dd.clone();
        let strs = filter_strs.clone();
        let status_w = status.clone();

        btn_reset.connect_clicked(move |_| {
            let Some(st) = state_w.upgrade() else {
                return;
            };
            let Some(ls) = list_w.upgrade() else {
                return;
            };

            let selected_rows = ls.selected_rows();
            if selected_rows.is_empty() {
                status_w.set_text("Select one or more atoms first.");
                return;
            }

            let mut count = 0usize;
            {
                let mut s = st.borrow_mut();
                let tab = s.active_tab_mut();
                for row in &selected_rows {
                    let idx = unsafe { row.data::<usize>("atom_idx") };
                    let Some(idx) = idx else {
                        continue;
                    };
                    let idx = unsafe { *idx.as_ref() };
                    if tab.overrides.remove(&idx).is_some() {
                        count += 1;
                    }
                }
            }

            let f_idx = filter_dd_w.selected() as usize;
            let f = strs.get(f_idx).map(|s| s.as_str());
            populate_list(&ls, &st.borrow(), f);

            if let Some(nb) = nb_w.upgrade() {
                if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                    da.queue_draw();
                }
            }

            status_w.set_text(&format!("Cleared override on {} atom(s).", count));
        });
    }

    dialog.add_button("Close", ResponseType::Close);
    dialog.connect_response(|dlg, _| dlg.close());
    dialog.show();
}
