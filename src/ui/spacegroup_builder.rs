// src/ui/spacegroup_builder.rs
//
// Space Group Builder: make a crystal from a space group, cell parameters
// and the asymmetric unit (File → New from Space Group), or edit the active
// structure the same way (Structure → Space Group & Sites). The window
// rebuilds on every change and shows the result in the flat schematic
// preview; the main view is only touched on Create / Apply.
//
// Sites are entered as International Tables do it (and xband): pick the
// Wyckoff position, then give only its free parameters. Every position of
// the setting is listed with multiplicity, site symmetry and coordinates;
// clicking one adds a site there.

use crate::menu::actions_file::{place_structure, OpenContext};
use crate::physics::operations::spacegroup_build::{self as sgb, Param};
use crate::ui::structure::preview::Preview;
use crate::ui::style::{card_body, group, prefixed_field};
use crate::utils::console;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, DropDown, Entry, Expander, Grid, Label,
    ListBox, ListBoxRow, ListItem, Orientation, PolicyType, ScrolledWindow, SelectionMode,
    SignalListItemFactory, SpinButton, StringList, StringObject, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Build a new structure into a new tab.
    New,
    /// Rewrite the active tab's structure (undoable).
    Edit,
}

const PARAM_NAMES: [&str; 6] = ["a", "b", "c", "α", "β", "γ"];

/// One row of the sites table.
struct Row {
    /// Identifies the row; the remove button.
    remove: Button,
    label: Entry,
    element: Entry,
    /// Wyckoff position, an index into `Builder::positions`.
    wyckoff: DropDown,
    /// The free parameters x, y, z of the position. A coordinate the
    /// position fixes is shown greyed with its value.
    coords: [Entry; 3],
    occ: Entry,
    mult: Label,
    /// Oxidation state carried over from the structure being edited; used
    /// only while the row keeps that element.
    carried: Option<(String, i32)>,
    /// The label was generated, not typed: it follows the element (Na1, Na2).
    auto_label: Rc<Cell<bool>>,
}

/// The tab an Edit window writes back to, as it was when the window opened.
struct Target {
    index: usize,
    file_name: String,
}

struct Builder {
    ctx: OpenContext,
    mode: Mode,
    target: Option<Target>,
    window: glib::WeakRef<Window>,
    group_dd: DropDown,
    setting_dd: DropDown,
    setting_model: StringList,
    settings: RefCell<Vec<sgb::SettingChoice>>,
    info: Label,
    /// Wyckoff positions and operations of the current setting. Positions
    /// run from 'a' (most special) to the general position, as xband lists
    /// them: the special positions are the ones people look for.
    positions: RefCell<Vec<sgb::Wyckoff>>,
    ops: RefCell<Vec<sgb::SymOp>>,
    /// Labels of `positions`, shared by every row's Wyckoff picker.
    wyckoff_model: StringList,
    /// The clickable list of positions under the space group.
    wyckoff_list: ListBox,
    wyckoff_expander: Expander,
    cell: [SpinButton; 6],
    volume: Label,
    /// The sites table: header in row 0, site i in row i + 1.
    table: Grid,
    rows: RefCell<Vec<Row>>,
    status: Label,
    action: Button,
    preview: Preview,
    built: RefCell<Option<sgb::Built>>,
    /// Set while the window itself changes widgets, so their signals do not
    /// re-enter.
    updating: Cell<bool>,
}

/// Open the builder. In Edit mode it starts from the active structure
/// (falls back to New when the active tab is empty).
pub fn show(parent: &ApplicationWindow, ctx: OpenContext, mode: Mode) {
    let Some(state) = ctx.state.upgrade() else { return };

    let (mode, start, target) = {
        let st = state.borrow();
        let tab = st.tabs.get(st.active_tab_index);
        match (mode, tab.and_then(|t| t.structure.as_ref().map(|s| (t, s)))) {
            (Mode::Edit, Some((tab, s))) => (
                Mode::Edit,
                Some(sgb::describe(s)),
                Some(Target { index: st.active_tab_index, file_name: tab.file_name.clone() }),
            ),
            (Mode::Edit, None) => {
                console::log_warn("Space Group & Sites: no structure open; starting a new one");
                (Mode::New, None, None)
            }
            (Mode::New, _) => (Mode::New, None, None),
        }
    };

    let window = Window::builder()
        .title(match mode {
            Mode::New => "New Structure from Space Group",
            Mode::Edit => "Space Group & Sites",
        })
        .transient_for(parent)
        .default_width(1080)
        .default_height(700)
        .modal(false)
        .build();

    // ---- Space group card ----
    let groups: Vec<String> = (1..=230)
        .map(|n| {
            let s = sgb::SettingChoice::from_hall(sgb::standard_hall_number(n).unwrap_or(1));
            format!("{n:>3}   {}", s.map(|s| s.short_symbol()).unwrap_or_default())
        })
        .collect();
    let group_refs: Vec<&str> = groups.iter().map(String::as_str).collect();
    let group_dd = DropDown::from_strings(&group_refs);
    group_dd.set_enable_search(true);
    group_dd.set_expression(Some(gtk4::PropertyExpression::new(
        gtk4::StringObject::static_type(),
        None::<&gtk4::Expression>,
        "string",
    )));
    group_dd.set_hexpand(true);
    group_dd.set_tooltip_text(Some("Space group (type to search by number)"));

    let find = Entry::builder()
        .placeholder_text("225, Fm-3m…")
        .width_chars(11)
        .tooltip_text("Type a space-group number or symbol (225, Fm-3m, P21/c)")
        .primary_icon_name("system-search-symbolic")
        .build();

    // The button names the setting briefly ("origin choice 2"); the list
    // gives the full symbol with it.
    let setting_model = StringList::new(&[]);
    let setting_dd = DropDown::new(Some(setting_model.clone()), None::<gtk4::Expression>);
    set_compact_button(&setting_dd, 16, |s| match (s.find('('), s.rfind(')')) {
        (Some(a), Some(b)) if b > a => s[a + 1..b].to_string(),
        _ => s.trim().to_string(),
    });
    setting_dd.set_hexpand(true);
    setting_dd.set_tooltip_text(Some("Setting: axes, cell or origin choice"));

    let info = Label::builder().halign(Align::Start).wrap(true).xalign(0.0).build();
    info.add_css_class("dim-label");

    let wyckoff_list = ListBox::new();
    wyckoff_list.set_selection_mode(SelectionMode::None);
    wyckoff_list.set_activate_on_single_click(true);
    wyckoff_list.add_css_class("boxed-list");
    let wyckoff_scroll = ScrolledWindow::builder()
        .child(&wyckoff_list)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .min_content_height(170)
        .max_content_height(260)
        .propagate_natural_height(true)
        .build();
    let wyckoff_expander = Expander::new(Some("Wyckoff positions"));
    wyckoff_expander.set_expanded(true);
    wyckoff_expander.set_child(Some(&wyckoff_scroll));

    let sg_line = GtkBox::new(Orientation::Horizontal, 6);
    sg_line.append(&find);
    sg_line.append(&group_dd);
    sg_line.append(&setting_dd);
    let sg_body = card_body(8);
    sg_body.append(&sg_line);
    sg_body.append(&info);
    sg_body.append(&wyckoff_expander);

    // ---- Cell card ----
    let cell: [SpinButton; 6] = std::array::from_fn(|i| {
        let spin = if i < 3 {
            SpinButton::with_range(0.1, 1000.0, 0.01)
        } else {
            SpinButton::with_range(1.0, 179.0, 0.1)
        };
        spin.set_digits(if i < 3 { 4 } else { 3 });
        spin.set_value(if i < 3 { 5.0 } else { 90.0 });
        spin
    });
    let grid = Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);
    grid.set_column_homogeneous(true);
    for (i, spin) in cell.iter().enumerate() {
        let field = prefixed_field(PARAM_NAMES[i], spin);
        grid.attach(&field, (i % 3) as i32, (i / 3) as i32, 1, 1);
    }
    let volume = Label::builder().halign(Align::Start).build();
    volume.add_css_class("dim-label");
    let cell_body = card_body(8);
    cell_body.append(&grid);
    cell_body.append(&volume);

    // ---- Sites card ----
    let table = Grid::new();
    table.set_row_spacing(4);
    table.set_column_spacing(4);
    for (col, text) in ["Label", "El.", "Wyck.", "x", "y", "z", "Occ.", "Site"].iter().enumerate() {
        let l = Label::new(Some(text));
        l.set_xalign(0.0);
        l.add_css_class("dim-label");
        table.attach(&l, col as i32, 0, 1, 1);
    }
    let add_btn = Button::with_label("Add Site");
    add_btn.set_halign(Align::Start);
    let hint = Label::builder()
        .label("Click a Wyckoff position above to add a site, then type its free \
                parameters (1/3 works). Greyed coordinates are fixed by the position.")
        .wrap(true)
        .xalign(0.0)
        .build();
    hint.add_css_class("dim-label");
    let sites_body = card_body(6);
    sites_body.append(&table);
    sites_body.append(&add_btn);
    sites_body.append(&hint);

    // ---- Status and buttons ----
    let status = Label::builder().halign(Align::Start).wrap(true).xalign(0.0).selectable(true).build();
    let cancel = Button::with_label("Cancel");
    let action = Button::with_label(match mode {
        Mode::New => "Create",
        Mode::Edit => "Apply",
    });
    action.add_css_class("suggested-action");
    let buttons = GtkBox::new(Orientation::Horizontal, 8);
    buttons.set_halign(Align::End);
    buttons.append(&cancel);
    buttons.append(&action);

    let controls = GtkBox::new(Orientation::Vertical, 12);
    controls.append(&group("Space Group", &sg_body));
    controls.append(&group("Unit Cell", &cell_body));
    controls.append(&group("Sites (asymmetric unit)", &sites_body));
    controls.append(&status);

    let scroll = ScrolledWindow::builder()
        .child(&controls)
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .build();
    scroll.set_width_request(500);

    let right = GtkBox::new(Orientation::Vertical, 10);
    right.append(&scroll);
    right.append(&buttons);

    let preview = Preview::new(state.clone());
    let page = GtkBox::new(Orientation::Horizontal, 15);
    page.set_margin_top(15);
    page.set_margin_bottom(15);
    page.set_margin_start(15);
    page.set_margin_end(15);
    page.append(preview.widget());
    page.append(&right);
    window.set_child(Some(&page));

    let b = Rc::new(Builder {
        ctx,
        mode,
        target,
        window: window.downgrade(),
        group_dd: group_dd.clone(),
        setting_dd: setting_dd.clone(),
        setting_model,
        settings: RefCell::new(Vec::new()),
        info,
        positions: RefCell::new(Vec::new()),
        ops: RefCell::new(Vec::new()),
        wyckoff_model: StringList::new(&[]),
        wyckoff_list: wyckoff_list.clone(),
        wyckoff_expander,
        cell: cell.clone(),
        volume,
        table,
        rows: RefCell::new(Vec::new()),
        status,
        action: action.clone(),
        preview,
        built: RefCell::new(None),
        updating: Cell::new(false),
    });

    // ---- Initial content ----
    let (hall, params, sites) = match &start {
        Some(d) => (d.hall_number, d.params, d.sites.clone()),
        None => (1, [5.0, 5.0, 5.0, 90.0, 90.0, 90.0], Vec::new()),
    };
    b.select_hall(hall);
    b.set_params(params);
    // A new structure starts with no sites: choose the group, then click
    // the Wyckoff positions to fill.
    for s in &sites {
        b.add_row(Some(s), None);
    }
    if let Some(note) = start.as_ref().and_then(|d| d.note.clone()) {
        console::log_info(&format!("Space Group & Sites: {note}"));
    }

    // ---- Signals (all hold the builder weakly; the window's close
    // handler holds the one strong reference) ----
    let weak = Rc::downgrade(&b);
    group_dd.connect_selected_notify(with(&weak, |b| {
        if !b.updating.get() {
            b.group_changed(None);
        }
    }));
    setting_dd.connect_selected_notify(with(&weak, |b| {
        if !b.updating.get() {
            b.setting_changed();
        }
    }));
    for spin in &cell {
        spin.connect_value_changed(with(&weak, |b| {
            if !b.updating.get() {
                b.cell_changed();
            }
        }));
    }
    {
        let weak = weak.clone();
        find.connect_changed(move |e| {
            if let (Some(b), Some(hall)) = (weak.upgrade(), lookup(&e.text())) {
                b.select_hall(hall);
            }
        });
    }
    add_btn.connect_clicked(with(&weak, |b| {
        b.add_row(None, None);
        b.rebuild();
    }));
    {
        let weak = weak.clone();
        wyckoff_list.connect_row_activated(move |_, row| {
            if let Some(b) = weak.upgrade() {
                b.add_row(None, Some(row.index() as usize));
                b.rebuild();
            }
        });
    }
    {
        let w = window.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.close();
            }
        });
    }
    action.connect_clicked(with(&weak, |b| b.commit()));
    {
        let keep = RefCell::new(Some(b.clone()));
        window.connect_close_request(move |_| {
            keep.borrow_mut().take();
            glib::Propagation::Proceed
        });
    }

    b.rebuild();
    window.present();
}

/// A signal handler that runs `f` on the builder while it is alive.
fn with<W>(weak: &Weak<Builder>, f: impl Fn(&Rc<Builder>) + 'static) -> impl Fn(&W) + 'static {
    let weak = weak.clone();
    move |_| {
        if let Some(b) = weak.upgrade() {
            f(&b);
        }
    }
}

/// Give a string drop-down a short button: `short` turns the selected
/// item into what the button shows (ellipsized at `chars`), while the popup
/// list keeps the full text.
fn set_compact_button(dd: &DropDown, chars: i32, short: impl Fn(&str) -> String + 'static) {
    let label_factory = |f: Box<dyn Fn(&str) -> String>, chars: Option<i32>| {
        let factory = SignalListItemFactory::new();
        factory.connect_setup(move |_, obj| {
            let Some(item) = obj.downcast_ref::<ListItem>() else { return };
            let l = Label::new(None);
            l.set_xalign(0.0);
            if let Some(n) = chars {
                l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                l.set_max_width_chars(n);
            }
            item.set_child(Some(&l));
        });
        factory.connect_bind(move |_, obj| {
            let Some(item) = obj.downcast_ref::<ListItem>() else { return };
            let text = item
                .item()
                .and_downcast::<StringObject>()
                .map(|s| s.string().to_string())
                .unwrap_or_default();
            if let Some(l) = item.child().and_downcast::<Label>() {
                l.set_text(&f(&text));
                l.set_tooltip_text(Some(&text));
            }
        });
        factory
    };
    dd.set_factory(Some(&label_factory(Box::new(short), Some(chars))));
    dd.set_list_factory(Some(&label_factory(Box::new(|s: &str| s.to_string()), None)));
}

/// Symbols compared without spaces, underscores or case: "P 2_1/c" = "p21/c".
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// The setting a typed number or symbol names: a number gives the standard
/// setting; a short symbol its standard setting; a full symbol that setting.
fn lookup(text: &str) -> Option<i32> {
    let t = text.trim();
    if let Ok(n) = t.parse::<i32>() {
        return sgb::standard_hall_number(n);
    }
    let q = squash(t);
    if q.len() < 2 {
        return None;
    }
    let all: Vec<sgb::SettingChoice> = (1..=530).filter_map(sgb::SettingChoice::from_hall).collect();
    if let Some(s) = all.iter().find(|s| squash(s.hm_short) == q) {
        return sgb::standard_hall_number(s.number);
    }
    all.iter().find(|s| squash(s.hm_full) == q).map(|s| s.hall_number)
}

fn entry(text: &str, chars: i32, expand: bool) -> Entry {
    let e = Entry::builder().text(text).width_chars(chars).max_width_chars(chars).build();
    e.set_hexpand(expand);
    e
}

impl Builder {
    fn hall_number(&self) -> Option<i32> {
        self.settings
            .borrow()
            .get(self.setting_dd.selected() as usize)
            .map(|s| s.hall_number)
    }

    /// Show `hall` in the group and setting pickers.
    fn select_hall(self: &Rc<Self>, hall: i32) {
        let Some(choice) = sgb::SettingChoice::from_hall(hall) else { return };
        self.updating.set(true);
        self.group_dd.set_selected((choice.number - 1) as u32);
        self.updating.set(false);
        self.group_changed(Some(hall));
    }

    /// The group changed: list its settings and pick `hall` (or the standard one).
    fn group_changed(self: &Rc<Self>, hall: Option<i32>) {
        let number = self.group_dd.selected() as i32 + 1;
        let settings = sgb::settings_for(number);
        let want = hall.or_else(|| sgb::standard_hall_number(number));
        let idx = settings.iter().position(|s| Some(s.hall_number) == want).unwrap_or(0);
        let labels: Vec<String> = settings.iter().map(|s| s.label()).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();

        self.updating.set(true);
        *self.settings.borrow_mut() = settings;
        self.setting_model.splice(0, self.setting_model.n_items(), &refs);
        self.setting_dd.set_selected(idx as u32);
        self.setting_dd.set_sensitive(refs.len() > 1);
        self.updating.set(false);
        self.setting_changed();
    }

    fn setting_changed(self: &Rc<Self>) {
        let Some(hall) = self.hall_number() else { return };
        self.load_positions(hall);
        let rules = sgb::param_rules(hall);
        for (spin, rule) in self.cell.iter().zip(rules) {
            spin.set_sensitive(matches!(rule, Param::Free));
        }
        let choice = sgb::SettingChoice::from_hall(hall);
        let ops = sgb::operations(hall).map(|o| o.len()).unwrap_or(0);
        if let Some(c) = choice {
            let tied: Vec<String> = rules
                .iter()
                .enumerate()
                .filter_map(|(i, r)| match r {
                    Param::SameAs(j) => Some(format!("{} = {}", PARAM_NAMES[i], PARAM_NAMES[*j])),
                    _ => None,
                })
                .collect();
            let mut text = format!(
                "No. {} {} · {} · {} operation{} per cell",
                c.number,
                c.short_symbol(),
                sgb::crystal_system(c.number).name(),
                ops,
                if ops == 1 { "" } else { "s" }
            );
            if !tied.is_empty() {
                text.push_str(&format!(" · {}", tied.join(", ")));
            }
            self.info.set_text(&text);
        }
        self.cell_changed();
    }

    fn params(&self) -> sgb::CellParams {
        std::array::from_fn(|i| self.cell[i].value())
    }

    fn set_params(self: &Rc<Self>, p: sgb::CellParams) {
        self.updating.set(true);
        for (spin, v) in self.cell.iter().zip(p) {
            spin.set_value(v);
        }
        self.updating.set(false);
        self.cell_changed();
    }

    /// Apply the setting's constraints to the cell fields, then rebuild.
    fn cell_changed(self: &Rc<Self>) {
        if let Some(hall) = self.hall_number() {
            let p = sgb::constrain(hall, self.params());
            self.updating.set(true);
            for (spin, v) in self.cell.iter().zip(p) {
                if (spin.value() - v).abs() > 1e-12 {
                    spin.set_value(v);
                }
            }
            self.updating.set(false);
        }
        self.rebuild();
    }

    /// Switch the Wyckoff positions to those of `hall`: refill the shared
    /// picker model and the list, and put every existing site on the
    /// position its coordinates lie on in the new setting.
    fn load_positions(self: &Rc<Self>, hall: i32) {
        // Where the sites are now, in the old setting.
        let fracs: Vec<Option<[f64; 3]>> = {
            let rows = self.rows.borrow();
            rows.iter().map(|r| self.row_frac(r).ok()).collect()
        };

        let mut positions = sgb::wyckoff_positions(hall);
        positions.reverse();
        let ops = sgb::operations(hall).unwrap_or_default();
        let labels: Vec<String> = positions.iter().map(|w| w.label()).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();

        self.updating.set(true);
        *self.positions.borrow_mut() = positions;
        *self.ops.borrow_mut() = ops;
        self.wyckoff_model.splice(0, self.wyckoff_model.n_items(), &refs);
        self.fill_wyckoff_list();
        let rows = self.rows.borrow();
        let general = self.positions.borrow().len().saturating_sub(1);
        for (r, frac) in rows.iter().zip(fracs) {
            let (idx, params) = frac
                .and_then(|f| self.place(f))
                .unwrap_or((general, sgb::GENERIC_PARAMS));
            r.wyckoff.set_selected(idx as u32);
            self.show_params(r, Some(params));
        }
        drop(rows);
        self.updating.set(false);
    }

    /// Rebuild the clickable list of Wyckoff positions.
    fn fill_wyckoff_list(&self) {
        while let Some(child) = self.wyckoff_list.first_child() {
            self.wyckoff_list.remove(&child);
        }
        let positions = self.positions.borrow();
        for w in positions.iter() {
            let line = GtkBox::new(Orientation::Horizontal, 12);
            line.set_margin_top(3);
            line.set_margin_bottom(3);
            line.set_margin_start(8);
            line.set_margin_end(8);
            for (text, chars, dim) in [
                (w.name(), 5, false),
                (w.site_symmetry.to_string(), 8, true),
                (format!("({})", w.coordinates.replace(',', ", ")), 0, false),
            ] {
                let l = Label::new(Some(&text));
                l.set_xalign(0.0);
                if chars > 0 {
                    l.set_width_chars(chars);
                }
                if dim {
                    l.add_css_class("dim-label");
                }
                line.append(&l);
            }
            let row = ListBoxRow::new();
            row.set_child(Some(&line));
            row.set_tooltip_text(Some("Add a site on this position"));
            self.wyckoff_list.append(&row);
        }
        self.wyckoff_expander.set_label(Some(&format!(
            "Wyckoff positions ({})",
            positions.len()
        )));
    }

    /// The position (index) and parameters that describe `frac`: the most
    /// special Wyckoff position it lies on.
    fn place(&self, frac: [f64; 3]) -> Option<(usize, [f64; 3])> {
        let positions = self.positions.borrow();
        let ops = self.ops.borrow();
        let (w, p) = sgb::identify_wyckoff(&positions, &ops, frac, None)?;
        let idx = positions.iter().position(|x| std::ptr::eq(x, w))?;
        Some((idx, p))
    }

    /// Set a row's parameter fields for its current Wyckoff position: free
    /// ones editable (filled from `params` when given, else kept if they
    /// hold a number, else a generic value), fixed ones greyed.
    fn show_params(&self, r: &Row, params: Option<[f64; 3]>) {
        let positions = self.positions.borrow();
        let Some(w) = positions.get(r.wyckoff.selected() as usize) else { return };
        let free = w.free();
        let was = self.updating.replace(true);
        let mut p = [0.0; 3];
        for (j, e) in r.coords.iter().enumerate() {
            p[j] = match params {
                Some(v) => v[j],
                None if free[j] && e.is_sensitive() => {
                    sgb::parse_coord(&e.text()).unwrap_or(sgb::GENERIC_PARAMS[j])
                }
                None => sgb::GENERIC_PARAMS[j],
            };
        }
        let coords = w.position(p);
        for (j, e) in r.coords.iter().enumerate() {
            // A free parameter that was already editable keeps what was typed
            // (rewriting it would fight the typing).
            let keep = free[j] && params.is_none() && e.is_sensitive();
            e.set_sensitive(free[j]);
            let text = if free[j] { sgb::format_coord(p[j]) } else { sgb::format_coord(coords[j]) };
            if !keep && e.text() != text {
                e.set_text(&text);
            }
            e.set_tooltip_text(Some(&if free[j] {
                format!("Free parameter {} of {}", ["x", "y", "z"][j], w.name())
            } else {
                format!("Fixed by {} ({})", w.name(), w.coordinates.replace(',', ", "))
            }));
            if !keep {
                mark(e, true);
            }
        }
        self.updating.set(was);
    }

    /// A row's coordinates from its Wyckoff position and parameters.
    fn row_frac(&self, r: &Row) -> Result<[f64; 3], String> {
        let positions = self.positions.borrow();
        let w = positions
            .get(r.wyckoff.selected() as usize)
            .ok_or_else(|| "choose a Wyckoff position.".to_string())?;
        let free = w.free();
        let mut p = [0.0; 3];
        let mut ok = true;
        for (j, e) in r.coords.iter().enumerate() {
            if free[j] {
                match sgb::parse_coord(&e.text()) {
                    Some(v) => p[j] = v,
                    None => {
                        mark(e, false);
                        ok = false;
                        continue;
                    }
                }
            }
            mark(e, true);
        }
        if !ok {
            return Err("parameters must be numbers or fractions like 1/3.".into());
        }
        Ok(w.position(p))
    }

    /// Add a site row. `site` fills it (editing); otherwise it goes on
    /// position `wyckoff`, or on the most special position no row uses yet.
    fn add_row(self: &Rc<Self>, site: Option<&sgb::Site>, wyckoff: Option<usize>) {
        let n = self.rows.borrow().len() + 1;
        let (label, element, occ) = match site {
            Some(s) => (s.label.clone(), s.element.clone(), s.occupancy),
            None => (format!("X{n}"), String::new(), 1.0),
        };
        let (idx, params) = match site {
            Some(s) => {
                let positions = self.positions.borrow();
                let by_letter = s.wyckoff.and_then(|l| {
                    let i = positions.iter().position(|w| w.letter == l)?;
                    Some((i, positions[i].params_for(s.frac)?))
                });
                drop(positions);
                let general = self.positions.borrow().len().saturating_sub(1);
                by_letter.or_else(|| self.place(s.frac)).unwrap_or((general, s.frac))
            }
            None => {
                let used: Vec<u32> = self.rows.borrow().iter().map(|r| r.wyckoff.selected()).collect();
                let count = self.positions.borrow().len();
                let idx = wyckoff.unwrap_or_else(|| {
                    (0..count)
                        .find(|i| !used.contains(&(*i as u32)))
                        .unwrap_or(count.saturating_sub(1))
                });
                (idx, sgb::GENERIC_PARAMS)
            }
        };

        let label_e = entry(&label, 5, false);
        let element_e = entry(&element, 3, false);
        element_e.set_placeholder_text(Some("El"));
        // The button shows just "4a"; the list shows symmetry and coordinates.
        let wyckoff_dd = DropDown::new(Some(self.wyckoff_model.clone()), None::<gtk4::Expression>);
        set_compact_button(&wyckoff_dd, 5, |s| s.split_whitespace().next().unwrap_or("").to_string());
        wyckoff_dd.set_selected(idx as u32);
        let coords: [Entry; 3] = std::array::from_fn(|_| entry("", 5, true));
        let occ_s = entry(&sgb::format_coord(occ), 4, false);
        occ_s.set_tooltip_text(Some("Occupancy, 0 to 1"));
        let mult = Label::new(Some("–"));
        mult.set_width_chars(4);
        let remove = Button::from_icon_name("list-remove-symbolic");
        remove.set_tooltip_text(Some("Remove this site"));
        remove.add_css_class("flat");

        let r = n as i32;
        self.table.attach(&label_e, 0, r, 1, 1);
        self.table.attach(&element_e, 1, r, 1, 1);
        self.table.attach(&wyckoff_dd, 2, r, 1, 1);
        for (k, c) in coords.iter().enumerate() {
            self.table.attach(c, 3 + k as i32, r, 1, 1);
        }
        self.table.attach(&occ_s, 6, r, 1, 1);
        self.table.attach(&mult, 7, r, 1, 1);
        self.table.attach(&remove, 8, r, 1, 1);

        let weak = Rc::downgrade(self);
        let auto_label = Rc::new(Cell::new(site.is_none()));
        {
            // Typing a label of one's own stops it following the element.
            let auto = auto_label.clone();
            let weak = weak.clone();
            label_e.connect_changed(move |_| {
                let Some(b) = weak.upgrade() else { return };
                if !b.updating.get() {
                    auto.set(false);
                }
                b.rebuild();
            });
        }
        for e in [&element_e, &coords[0], &coords[1], &coords[2]] {
            e.connect_changed(with(&weak, |b| b.rebuild()));
        }
        element_e.connect_changed(with(&weak, |b| b.relabel()));
        occ_s.connect_changed(with(&weak, |b| b.rebuild()));
        {
            let weak = weak.clone();
            wyckoff_dd.connect_selected_notify(move |dd| {
                let Some(b) = weak.upgrade() else { return };
                if b.updating.get() {
                    return;
                }
                if let Some(r) = b.rows.borrow().iter().find(|r| &r.wyckoff == dd) {
                    b.show_params(r, None);
                }
                b.rebuild();
            });
        }
        {
            let weak = weak.clone();
            remove.connect_clicked(move |btn| {
                let Some(b) = weak.upgrade() else { return };
                let idx = b.rows.borrow().iter().position(|r| &r.remove == btn);
                if let Some(idx) = idx {
                    b.rows.borrow_mut().remove(idx);
                    b.table.remove_row(idx as i32 + 1);
                }
                b.rebuild();
            });
        }

        let carried = site.and_then(|s| s.oxidation.map(|o| (s.element.clone(), o)));
        let row = Row {
            remove,
            label: label_e,
            element: element_e,
            wyckoff: wyckoff_dd,
            coords,
            occ: occ_s,
            mult,
            carried,
            auto_label,
        };
        self.show_params(&row, Some(params));
        self.rows.borrow_mut().push(row);
    }

    /// Give generated labels the element and a running number per element.
    fn relabel(self: &Rc<Self>) {
        let rows = self.rows.borrow();
        let mut seen: std::collections::HashMap<String, usize> = Default::default();
        self.updating.set(true);
        for r in rows.iter() {
            let el = sgb::normalize_element(&r.element.text()).unwrap_or_else(|| "X".into());
            let n = seen.entry(el.clone()).or_insert(0);
            *n += 1;
            if r.auto_label.get() {
                let want = format!("{el}{n}");
                if r.label.text() != want {
                    r.label.set_text(&want);
                }
            }
        }
        self.updating.set(false);
        drop(rows);
        self.rebuild();
    }

    /// Read the sites table. Marks bad fields and returns the first problem.
    fn read_sites(&self) -> Result<Vec<sgb::Site>, String> {
        let rows = self.rows.borrow();
        let mut sites = Vec::with_capacity(rows.len());
        let mut first_err: Option<String> = None;
        for (i, r) in rows.iter().enumerate() {
            let label = {
                let t = r.label.text().trim().to_string();
                if t.is_empty() {
                    format!("site {}", i + 1)
                } else {
                    t
                }
            };
            let element = sgb::normalize_element(&r.element.text());
            mark(&r.element, element.is_some() || r.element.text().trim().is_empty());
            let frac = self.row_frac(r);
            let occ = sgb::parse_coord(&r.occ.text()).filter(|o| (0.0..=1.0).contains(o));
            mark(&r.occ, occ.is_some());
            let err = match (&element, &frac) {
                (None, _) if r.element.text().trim().is_empty() => Some(format!("{label}: enter an element.")),
                (None, _) => Some(format!("{label}: \u{201c}{}\u{201d} is not an element.", r.element.text().trim())),
                (_, Err(e)) => Some(format!("{label}: {e}")),
                _ if occ.is_none() => Some(format!("{label}: occupancy must be between 0 and 1.")),
                _ => None,
            };
            if let Some(e) = err {
                first_err.get_or_insert(e);
                continue;
            }
            let (Some(element), Ok(frac)) = (element, frac) else { continue };
            let wyckoff = self.positions.borrow().get(r.wyckoff.selected() as usize).map(|w| w.letter);
            let oxidation = r.carried.as_ref().filter(|(el, _)| *el == element).map(|(_, o)| *o);
            sites.push(sgb::Site {
                label,
                element,
                frac,
                occupancy: occ.unwrap_or(1.0),
                oxidation,
                wyckoff,
            });
        }
        match first_err {
            Some(e) => Err(e),
            None if sites.is_empty() => {
                Err("Add a site: click a Wyckoff position above, or Add Site.".into())
            }
            None => Ok(sites),
        }
    }

    /// Rebuild the crystal from the current fields and show it.
    fn rebuild(self: &Rc<Self>) {
        if self.updating.get() {
            return;
        }
        let params = self.params();
        match sgb::lattice_from_params(params) {
            Ok(lat) => {
                let v = crate::utils::linalg::mat3_det(lat).abs();
                self.volume.set_text(&format!("Lengths in Å, angles in degrees · volume {v:.3} Å³"));
            }
            Err(e) => self.volume.set_text(&e),
        }

        let result = self
            .hall_number()
            .ok_or_else(|| "Choose a space group.".to_string())
            .and_then(|hall| {
                let sites = self.read_sites()?;
                sgb::build(hall, params, &sites)
            });

        let rows = self.rows.borrow();
        self.table.set_visible(!rows.is_empty());
        match &result {
            Ok(built) => {
                // Built sites are the valid rows, which is every row here.
                let positions = self.positions.borrow();
                for (r, s) in rows.iter().zip(&built.sites) {
                    // Greyed coordinates follow the free parameters (y = x …).
                    self.show_params(r, None);
                    let chosen = positions.get(r.wyckoff.selected() as usize).map(|w| w.name());
                    let actual = s.wyckoff.clone().unwrap_or_else(|| s.multiplicity.to_string());
                    r.mult.set_text(&actual);
                    let mut tip = format!(
                        "{} atom{} in the conventional cell at ({}, {}, {})",
                        s.multiplicity,
                        if s.multiplicity == 1 { "" } else { "s" },
                        sgb::format_coord(s.frac[0]),
                        sgb::format_coord(s.frac[1]),
                        sgb::format_coord(s.frac[2])
                    );
                    if chosen.as_deref() != s.wyckoff.as_deref() {
                        tip.push_str(&format!(
                            "\nThese parameters put the site on {actual}, not {}.",
                            chosen.unwrap_or_default()
                        ));
                        r.mult.add_css_class("warning");
                    } else {
                        r.mult.remove_css_class("warning");
                    }
                    if s.moved > 1e-6 {
                        tip.push_str(&format!("\nMoved {:.3} Å onto the special position.", s.moved));
                    }
                    r.mult.set_tooltip_text(Some(&tip));
                }
                drop(positions);
                let st = &built.structure;
                let mut text = format!("{} atoms in the cell · {}", st.atoms.len(), st.formula);
                for w in &built.warnings {
                    text.push_str("\n⚠ ");
                    text.push_str(w);
                }
                self.status.set_text(&text);
                self.status.remove_css_class("error");
                if built.warnings.is_empty() {
                    self.status.remove_css_class("warning");
                } else {
                    self.status.add_css_class("warning");
                }
                self.preview.set_candidate(Some(st.clone()));
            }
            Err(e) => {
                for r in rows.iter() {
                    r.mult.set_text("–");
                }
                self.status.set_text(e);
                self.status.remove_css_class("warning");
                self.status.add_css_class("error");
            }
        }
        drop(rows);
        let ok = result.as_ref().is_ok_and(|b| !b.structure.atoms.is_empty());
        self.action.set_sensitive(ok);
        *self.built.borrow_mut() = result.ok();
    }

    /// Create / Apply.
    fn commit(self: &Rc<Self>) {
        let Some(built) = self.built.borrow().clone() else { return };
        let Some(hall) = self.hall_number() else { return };
        let symbol = sgb::SettingChoice::from_hall(hall)
            .map(|c| c.short_symbol())
            .unwrap_or_default();
        let structure = built.structure;
        let op = format!("Built from space group {symbol}");

        match self.mode {
            Mode::New => {
                let name = format!("{} ({symbol})", structure.formula);
                if place_structure(&self.ctx, &name, structure.clone(), None, 0).is_none() {
                    return;
                }
                console::structure_changed(&op, &structure);
            }
            Mode::Edit => {
                let (Some(state), Some(t)) = (self.ctx.state.upgrade(), self.target.as_ref()) else {
                    return;
                };
                {
                    let mut st = state.borrow_mut();
                    let Some(tab) = st.tabs.get_mut(t.index).filter(|tab| tab.file_name == t.file_name)
                    else {
                        drop(st);
                        console::log_error(
                            "Space Group & Sites: the tab being edited was closed or moved; \
                             reopen the editor on it.",
                        );
                        return;
                    };
                    if let Some(old) = tab.structure.take() {
                        tab.interaction.undo_stack.push(old);
                    }
                    tab.structure = Some(structure.clone());
                    tab.interaction.selected.clear();
                    tab.overrides.clear();
                    tab.invalidate_derived();
                    tab.style_generic_species();
                }
                console::structure_changed("Edited space group & sites", &structure);
                if let Some(nb) = self.ctx.notebook.upgrade() {
                    if let Some(da) = crate::ui::get_active_drawing_area(&nb) {
                        da.queue_draw();
                    }
                    if let Some(ab) = self.ctx.atom_box.upgrade() {
                        crate::panels::sidebar::refresh_atom_list(&ab, state.clone(), &nb);
                    }
                }
            }
        }
        if let Some(w) = self.window.upgrade() {
            w.close();
        }
    }
}

/// Red outline on a field that cannot be read.
fn mark(e: &Entry, ok: bool) {
    if ok {
        e.remove_css_class("error");
    } else {
        e.add_css_class("error");
    }
}
