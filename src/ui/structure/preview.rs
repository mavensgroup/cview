// src/ui/structure/preview.rs
//
// The shared preview pane of the Structure window: a ball-and-stick view of
// the active tab's structure, drawn with the same scene/painter code as the
// main view. It can show a *candidate* structure (what a supercell would
// produce) and highlight atoms (the selection, a find-element, list rows).
//
// It reads the live state at draw time and polls a cheap fingerprint, so it
// follows edits made elsewhere (main view, other tabs) instead of showing the
// structure it was opened with. Orientation and zoom mirror the main view:
// rotate there and the preview follows.

use crate::config::ColorMode;
use crate::rendering;
use crate::state::{AppState, SelectedAtom, TabState};
use crate::model::structure::Structure;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{DrawingArea, Frame};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
struct PreviewData {
    /// Drawn instead of the tab's structure when set (supercell result).
    candidate: Option<Structure>,
    /// Atom indices (into `structure.atoms`) to highlight.
    indices: HashSet<usize>,
    /// Also highlight whatever is selected in the main view.
    main_selection: bool,
    /// Also highlight every atom of this element.
    element: Option<String>,
}

#[derive(Clone)]
pub struct Preview {
    frame: Frame,
    area: DrawingArea,
    data: Rc<RefCell<PreviewData>>,
}

impl Preview {
    pub fn new(state: Rc<RefCell<AppState>>) -> Self {
        let area = DrawingArea::new();
        area.set_content_width(400);
        area.set_content_height(400);
        area.set_hexpand(true);
        area.set_vexpand(true);

        let frame = Frame::new(Some("Preview"));
        frame.set_child(Some(&area));
        frame.set_hexpand(true);

        let data: Rc<RefCell<PreviewData>> = Rc::default();

        let s = state.clone();
        let d = data.clone();
        area.set_draw_func(move |_, cr, w, h| draw(&s.borrow(), &d.borrow(), cr, w, h));

        // Follow edits made elsewhere: redraw when the fingerprint changes.
        let weak_area = area.downgrade();
        let s = Rc::downgrade(&state);
        let mut last = 0u64;
        glib::timeout_add_local(Duration::from_millis(400), move || {
            let (Some(area), Some(st)) = (weak_area.upgrade(), s.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            let fp = fingerprint(&st.borrow());
            if fp != last {
                last = fp;
                area.queue_draw();
            }
            glib::ControlFlow::Continue
        });

        Self { frame, area, data }
    }

    pub fn widget(&self) -> &Frame {
        &self.frame
    }

    /// Show `candidate` instead of the active structure (None = the active one).
    pub fn set_candidate(&self, candidate: Option<Structure>) {
        self.data.borrow_mut().candidate = candidate;
        self.area.queue_draw();
    }

    pub fn set_highlight(
        &self,
        indices: HashSet<usize>,
        main_selection: bool,
        element: Option<String>,
    ) {
        {
            let mut d = self.data.borrow_mut();
            d.indices = indices;
            d.main_selection = main_selection;
            d.element = element;
        }
        self.area.queue_draw();
    }

    pub fn clear_highlight(&self) {
        self.set_highlight(HashSet::new(), false, None);
    }

    pub fn queue_draw(&self) {
        self.area.queue_draw();
    }
}

/// Cheap change detector for the active tab: structure, selection, overrides
/// and the view. Not cryptographic; a collision only delays a redraw.
fn fingerprint(st: &AppState) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    st.active_tab_index.hash(&mut h);
    st.tabs.len().hash(&mut h);
    if let Some(tab) = st.tabs.get(st.active_tab_index) {
        if let Some(s) = &tab.structure {
            s.atoms.len().hash(&mut h);
            for row in s.lattice {
                for v in row {
                    v.to_bits().hash(&mut h);
                }
            }
            for a in &s.atoms {
                a.element.hash(&mut h);
                for v in a.position {
                    v.to_bits().hash(&mut h);
                }
            }
        }
        tab.interaction.selected.len().hash(&mut h);
        tab.interaction
            .selected
            .keys()
            .fold(0usize, |acc, k| acc ^ k.wrapping_mul(0x9E37_79B9))
            .hash(&mut h);
        tab.overrides.len().hash(&mut h);
        format!("{:?}", tab.view).hash(&mut h);
    }
    h.finish()
}

fn draw(st: &AppState, d: &PreviewData, cr: &gtk4::cairo::Context, w: i32, h: i32) {
    let Some(active) = st.tabs.get(st.active_tab_index) else {
        return;
    };

    let (bg_r, bg_g, bg_b) = active.style.background_color;
    cr.set_source_rgb(bg_r, bg_g, bg_b);
    let _ = cr.paint();

    let Some(structure) = d.candidate.clone().or_else(|| active.structure.clone()) else {
        return;
    };

    // A scratch tab: same painter, own copy of the data. Orientation, zoom and
    // style come from the main view; the candidate has no per-atom overrides
    // (its indices no longer match).
    let mut scratch = TabState::new(&st.config);
    scratch.structure = Some(structure);
    scratch.view = active.view.clone();
    // A little smaller than the main view so atoms at the cell edge are not
    // clipped in this smaller pane.
    scratch.view.zoom *= 0.88;
    scratch.style = active.style.clone();
    if d.candidate.is_none() {
        scratch.overrides = active.overrides.clone();
    }
    if matches!(scratch.style.color_mode, ColorMode::BondValence) {
        let _ = scratch.get_bvs_values();
    }

    let (atoms, corners, bounds) = rendering::scene::calculate_scene(
        &scratch,
        &st.config,
        w as f64,
        h as f64,
        false,
        None,
        None,
    );

    // Highlight set, as indices into the structure being drawn.
    let mut hl: HashSet<usize> = d.indices.clone();
    if d.main_selection && d.candidate.is_none() {
        hl.extend(active.interaction.selected.values().map(|s| s.original_index));
    }
    if let (Some(el), Some(s)) = (&d.element, &scratch.structure) {
        hl.extend(
            s.atoms
                .iter()
                .enumerate()
                .filter(|(_, a)| &a.element == el)
                .map(|(i, _)| i),
        );
    }
    let mut selected = HashMap::new();
    for a in atoms.iter().filter(|a| !a.is_coord_only) {
        if hl.contains(&a.original_index) {
            selected.insert(
                a.unique_id,
                SelectedAtom {
                    unique_id: a.unique_id,
                    original_index: a.original_index,
                    cart_pos: a.cart_pos,
                    element: a.element.clone(),
                    seq: 0,
                },
            );
        }
    }
    scratch.interaction.selected = selected;

    rendering::painter::draw_unit_cell(cr, &corners, false);
    rendering::painter::draw_structure(
        cr,
        &atoms,
        &scratch,
        bounds.scale,
        false,
        st.config.color_scheme,
    );
    rendering::painter::draw_axes(cr, &scratch, w as f64, h as f64);
}
