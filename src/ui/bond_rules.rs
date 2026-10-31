// src/ui/bond_rules.rs
//
// Bond Rules window (View → Bond Rules, or the Bonds card in the sidebar):
// one row per element pair of the active structure, with an on/off switch,
// the minimum and maximum bond length, where the default came from, the
// shortest distance the pair actually has and how many bonds the rule draws
// per cell. Edits apply at once and are kept per tab; only the pairs the
// user touches are stored, the rest follow the defaults (VESTA's list, or
// covalent radii × tolerance — see `model::bonds`).
//
// The window follows the active tab, like the Structure window's preview.

use crate::model::bonds::{count_in_range, pair_distances, pair_key, BondRule, MIN_SEPARATION};
use crate::model::elements::get_electronegativity;
use crate::state::AppState;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, Grid, Label, Notebook,
    Orientation, PolicyType, ScrolledWindow, SpinButton, Window,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Distances are searched up to this (Å); the maximum length can't exceed it.
const SEARCH_CUTOFF: f64 = 6.0;

type Pair = (String, String);

struct RowWidgets {
    pair: Pair,
    on: CheckButton,
    min: SpinButton,
    max: SpinButton,
    source: Label,
    count: Label,
}

struct Editor {
    state: Weak<RefCell<AppState>>,
    notebook: glib::WeakRef<Notebook>,
    subtitle: Label,
    grid: Grid,
    tolerance: SpinButton,
    rows: RefCell<Vec<RowWidgets>>,
    distances: RefCell<Option<HashMap<Pair, Vec<f64>>>>,
    /// What the rows were built for (tab, elements, tolerance).
    key: Cell<u64>,
    updating: Cell<bool>,
}

thread_local! {
    static OPEN: RefCell<glib::WeakRef<Window>> = RefCell::new(glib::WeakRef::new());
}

pub fn show(parent: &ApplicationWindow, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    // One window: bring it forward if it is already open.
    if let Some(w) = OPEN.with(|o| o.borrow().upgrade()) {
        w.present();
        return;
    }

    let window = Window::builder()
        .title("Bond Rules")
        .transient_for(parent)
        .default_width(620)
        .default_height(540)
        .modal(false)
        .build();

    let subtitle = Label::builder().halign(Align::Start).xalign(0.0).build();
    subtitle.add_css_class("dim-label");

    let grid = Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(10);
    grid.set_margin_top(6);
    grid.set_margin_bottom(6);
    grid.set_margin_start(10);
    grid.set_margin_end(10);
    let scroll = ScrolledWindow::builder()
        .child(&grid)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .build();
    scroll.add_css_class("frame");

    let note = Label::builder()
        .label(
            "Defaults are VESTA's bond lengths. Pairs VESTA does not list stay unbonded, \
             except between elements with no listed partner here (metals, intermetallics), \
             which use covalent radii × tolerance. Atoms closer than 0.4 Å never bond.",
        )
        .wrap(true)
        .xalign(0.0)
        .build();
    note.add_css_class("dim-label");
    note.add_css_class("cview-caption");

    let tolerance = SpinButton::with_range(0.6, 1.6, 0.05);
    tolerance.set_digits(2);
    tolerance.set_tooltip_text(Some(
        "Scales the covalent radii for pairs that use them (not VESTA's lengths)",
    ));
    let tol_label = Label::new(Some("Covalent tolerance"));
    let reset = Button::with_label("Reset to Defaults");
    reset.set_tooltip_text(Some("Forget every change made to this structure's rules"));
    let close = Button::with_label("Close");
    let footer = GtkBox::new(Orientation::Horizontal, 8);
    footer.append(&tol_label);
    footer.append(&tolerance);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&spacer);
    footer.append(&reset);
    footer.append(&close);

    let page = GtkBox::new(Orientation::Vertical, 10);
    page.set_margin_top(15);
    page.set_margin_bottom(15);
    page.set_margin_start(15);
    page.set_margin_end(15);
    page.append(&subtitle);
    page.append(&scroll);
    page.append(&note);
    page.append(&footer);
    window.set_child(Some(&page));

    let ed = Rc::new(Editor {
        state: Rc::downgrade(&state),
        notebook: notebook.downgrade(),
        subtitle,
        grid,
        tolerance: tolerance.clone(),
        rows: RefCell::new(Vec::new()),
        distances: RefCell::new(None),
        key: Cell::new(0),
        updating: Cell::new(false),
    });
    ed.populate();

    // Follow the active tab: rebuild when it, its elements or the
    // tolerance change elsewhere.
    {
        let weak = Rc::downgrade(&ed);
        let win = window.downgrade();
        glib::timeout_add_local(Duration::from_millis(500), move || {
            let (Some(ed), Some(_)) = (weak.upgrade(), win.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if ed.current_key() != ed.key.get() {
                ed.populate();
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let weak = Rc::downgrade(&ed);
        tolerance.connect_value_changed(move |s| {
            let Some(ed) = weak.upgrade() else { return };
            if ed.updating.get() {
                return;
            }
            if let Some(st) = ed.state.upgrade() {
                st.borrow_mut().active_tab_mut().view.bond_cutoff = s.value();
            }
            ed.populate();
            ed.redraw();
        });
    }
    {
        let weak = Rc::downgrade(&ed);
        reset.connect_clicked(move |_| {
            let Some(ed) = weak.upgrade() else { return };
            if let Some(st) = ed.state.upgrade() {
                st.borrow_mut().active_tab_mut().bond_overrides.clear();
            }
            ed.populate();
            ed.redraw();
        });
    }
    {
        let w = window.downgrade();
        close.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.close();
            }
        });
    }
    {
        // The window's close handler holds the editor.
        let keep = RefCell::new(Some(ed));
        window.connect_close_request(move |_| {
            keep.borrow_mut().take();
            glib::Propagation::Proceed
        });
    }

    OPEN.with(|o| o.borrow().set(Some(&window)));
    window.present();
}

impl Editor {
    fn current_key(&self) -> u64 {
        let Some(st) = self.state.upgrade() else { return 0 };
        let st = st.borrow();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        st.active_tab_index.hash(&mut h);
        if let Some(tab) = st.tabs.get(st.active_tab_index) {
            tab.file_name.hash(&mut h);
            tab.bond_tolerance().to_bits().hash(&mut h);
            if let Some(s) = &tab.structure {
                s.atoms.len().hash(&mut h);
                for a in &s.atoms {
                    a.element.hash(&mut h);
                }
                for row in s.lattice {
                    for v in row {
                        v.to_bits().hash(&mut h);
                    }
                }
            }
        }
        h.finish()
    }

    fn redraw(&self) {
        if let Some(nb) = self.notebook.upgrade() {
            if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                da.queue_draw();
            }
        }
    }

    /// Rebuild every row from the active tab.
    fn populate(self: &Rc<Self>) {
        self.key.set(self.current_key());
        while let Some(c) = self.grid.first_child() {
            self.grid.remove(&c);
        }
        self.rows.borrow_mut().clear();

        let Some(st) = self.state.upgrade() else { return };
        let (rules, edited, tol, name, distances, periodic) = {
            let st = st.borrow();
            let Some(tab) = st.tabs.get(st.active_tab_index) else { return };
            let Some(s) = &tab.structure else {
                drop(st);
                self.subtitle.set_text("Open a structure to edit its bond rules.");
                return;
            };
            (
                tab.bond_rules(),
                tab.bond_overrides.keys().cloned().collect::<Vec<_>>(),
                tab.bond_tolerance(),
                tab.file_name.clone(),
                pair_distances(s, SEARCH_CUTOFF),
                s.is_periodic,
            )
        };
        self.subtitle.set_text(&format!(
            "{name} · {} element pairs · lengths in Å",
            rules.len()
        ));
        self.updating.set(true);
        self.tolerance.set_value(tol);
        self.updating.set(false);

        for (col, (text, tip)) in [
            ("", "Draw bonds for this pair"),
            ("Pair", ""),
            ("Min", "Shortest bond length"),
            ("Max", "Longest bond length"),
            ("Default", "Where the default comes from"),
            ("Shortest", "Shortest distance between these elements in the structure"),
            (
                if periodic { "Bonds/cell" } else { "Bonds" },
                "Bonds the rule draws (each counted once)",
            ),
        ]
        .iter()
        .enumerate()
        {
            let l = Label::new(Some(text));
            l.set_xalign(0.0);
            l.add_css_class("dim-label");
            if !tip.is_empty() {
                l.set_tooltip_text(Some(tip));
            }
            self.grid.attach(&l, col as i32, 0, 1, 1);
        }

        // Pairs that bond first, then the rest; alphabetical within each.
        let mut rules = rules;
        rules.sort_by(|x, y| y.enabled.cmp(&x.enabled).then((&x.a, &x.b).cmp(&(&y.a, &y.b))));

        let weak = Rc::downgrade(self);
        for (i, r) in rules.iter().enumerate() {
            let row = i as i32 + 1;
            let pair = pair_key(&r.a, &r.b);
            let dists = distances.as_ref().and_then(|d| d.get(&pair));

            let on = CheckButton::new();
            on.set_active(r.enabled);
            // Cation first, as chemists write it: "Ti – O", not "O – Ti".
            let (x, y) = if get_electronegativity(&r.a) <= get_electronegativity(&r.b) {
                (&r.a, &r.b)
            } else {
                (&r.b, &r.a)
            };
            let name = Label::new(Some(&format!("{x} – {y}")));
            name.set_xalign(0.0);
            let spin = |v: f64| {
                let s = SpinButton::with_range(0.0, SEARCH_CUTOFF, 0.05);
                s.set_digits(3);
                s.set_value(v.min(SEARCH_CUTOFF));
                s.set_width_chars(5);
                s.add_css_class("cview-compact");
                s.set_sensitive(r.enabled);
                s
            };
            let min = spin(r.min);
            let max = spin(r.max);
            let source = Label::new(Some(if edited.contains(&pair) { "edited" } else { r.source.name() }));
            source.set_xalign(0.0);
            source.add_css_class("dim-label");
            let shortest = Label::new(Some(&match (&distances, dists.and_then(|d| d.first())) {
                (None, _) => "–".to_string(),
                (Some(_), Some(d)) => format!("{d:.3}"),
                (Some(_), None) => format!("> {SEARCH_CUTOFF:.0}"),
            }));
            shortest.set_xalign(1.0);
            let count = Label::new(None);
            count.set_xalign(1.0);

            self.grid.attach(&on, 0, row, 1, 1);
            self.grid.attach(&name, 1, row, 1, 1);
            self.grid.attach(&min, 2, row, 1, 1);
            self.grid.attach(&max, 3, row, 1, 1);
            self.grid.attach(&source, 4, row, 1, 1);
            self.grid.attach(&shortest, 5, row, 1, 1);
            self.grid.attach(&count, 6, row, 1, 1);

            for s in [&min, &max] {
                let weak = weak.clone();
                let pair = pair.clone();
                s.connect_value_changed(move |_| {
                    if let Some(ed) = weak.upgrade() {
                        ed.changed(&pair);
                    }
                });
            }
            {
                let weak = weak.clone();
                let pair = pair.clone();
                on.connect_toggled(move |_| {
                    if let Some(ed) = weak.upgrade() {
                        ed.changed(&pair);
                    }
                });
            }

            let w = RowWidgets { pair, on, min, max, source, count };
            update_count(&w, distances.is_some(), dists);
            self.rows.borrow_mut().push(w);
        }
        *self.distances.borrow_mut() = distances;
    }

    /// A row was edited: store it as the tab's rule for the pair.
    fn changed(&self, pair: &Pair) {
        if self.updating.get() {
            return;
        }
        let Some(st) = self.state.upgrade() else { return };
        let rows = self.rows.borrow();
        let Some(w) = rows.iter().find(|w| &w.pair == pair) else { return };
        let enabled = w.on.is_active();
        w.min.set_sensitive(enabled);
        w.max.set_sensitive(enabled);
        {
            let mut st = st.borrow_mut();
            let tab = st.active_tab_mut();
            let source = tab
                .bond_rules()
                .into_iter()
                .find(|r| pair_key(&r.a, &r.b) == *pair)
                .map(|r| r.source)
                .unwrap_or(crate::model::bonds::RuleSource::None);
            tab.bond_overrides.insert(
                pair.clone(),
                BondRule {
                    a: pair.0.clone(),
                    b: pair.1.clone(),
                    min: w.min.value(),
                    max: w.max.value(),
                    enabled,
                    source,
                },
            );
        }
        w.source.set_text("edited");
        {
            let distances = self.distances.borrow();
            update_count(w, distances.is_some(), distances.as_ref().and_then(|d| d.get(pair)));
        }
        drop(rows);
        self.redraw();
    }
}

/// Show how many bonds a row's rule draws. `searched` is false when the
/// structure was too large to search, `dists` the pair's distances.
fn update_count(w: &RowWidgets, searched: bool, dists: Option<&Vec<f64>>) {
    let text = if !w.on.is_active() {
        String::new()
    } else if !searched {
        "–".to_string()
    } else {
        let d = dists.map(Vec::as_slice).unwrap_or(&[]);
        count_in_range(d, w.min.value().max(MIN_SEPARATION), w.max.value()).to_string()
    };
    w.count.set_text(&text);
}
