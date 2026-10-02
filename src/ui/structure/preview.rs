// src/ui/structure/preview.rs
//
// The shared preview pane of the Structure window: a flat, schematic view of
// the active tab's structure in the same style as the Slab cutting-plane
// canvas (white background, flat dots, wireframe cell, the same isometric
// projection), so the two look alike when you switch tabs. It is a preview,
// not the 3D viewport: no shading, no bonds.
//
// It can show a *candidate* structure (what a supercell would produce) and
// highlight atoms (the selection, a find-element, list rows). It reads the
// live state at draw time and polls a cheap fingerprint, so it follows edits
// made elsewhere instead of showing the structure it was opened with.

use crate::model::elements::get_element_color;
use crate::model::structure::Structure;
use crate::state::AppState;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{DrawingArea, Frame};

/// Isometric view shared with the Slab canvas, so both previews orient the
/// cell the same way.
pub const ISO_YAW: f64 = PI / 4.0 + 0.5;
pub const ISO_PITCH: f64 = PI / 6.0;
use std::cell::RefCell;
use std::collections::HashSet;
use std::f64::consts::PI;
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
        tab.style.element_colors.len().hash(&mut h);
    }
    h.finish()
}

/// Rotate a Cartesian point into the shared isometric view: (screen x, screen
/// y, depth).
fn iso(p: [f64; 3]) -> (f64, f64, f64) {
    let (x, y, z) = (p[0], p[1], p[2]);
    let x1 = x * ISO_YAW.cos() - z * ISO_YAW.sin();
    let z1 = x * ISO_YAW.sin() + z * ISO_YAW.cos();
    let y2 = y * ISO_PITCH.cos() - z1 * ISO_PITCH.sin();
    let depth = y * ISO_PITCH.sin() + z1 * ISO_PITCH.cos();
    (x1, y2, depth)
}

fn draw(st: &AppState, d: &PreviewData, cr: &gtk4::cairo::Context, w: i32, h: i32) {
    // Same white canvas as the Slab tab.
    cr.set_source_rgb(1.0, 1.0, 1.0);
    let _ = cr.paint();

    let Some(active) = st.tabs.get(st.active_tab_index) else {
        return;
    };
    let Some(structure) = d.candidate.as_ref().or(active.structure.as_ref()) else {
        return;
    };
    let lat = structure.lattice;
    let cart = |f: [f64; 3]| -> [f64; 3] {
        [
            f[0] * lat[0][0] + f[1] * lat[1][0] + f[2] * lat[2][0],
            f[0] * lat[0][1] + f[1] * lat[1][1] + f[2] * lat[2][1],
            f[0] * lat[0][2] + f[1] * lat[1][2] + f[2] * lat[2][2],
        ]
    };
    let to_frac = |p: [f64; 3]| -> Option<[f64; 3]> {
        crate::utils::linalg::cart_to_frac(p, lat)
    };
    let periodic = structure.is_periodic;

    // Highlight set, as indices into the structure being drawn.
    let mut hl: HashSet<usize> = d.indices.clone();
    if d.main_selection && d.candidate.is_none() {
        hl.extend(active.interaction.selected.values().map(|s| s.original_index));
    }
    if let Some(el) = &d.element {
        hl.extend(
            structure
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, a)| &a.element == el)
                .map(|(i, _)| i),
        );
    }

    // Points to draw: (cartesian, atom index). A periodic cell also draws the
    // images that sit on its faces and edges, as the Slab canvas does.
    let mut pts: Vec<([f64; 3], usize)> = Vec::new();
    for (i, a) in structure.atoms.iter().enumerate() {
        if periodic {
            if let Some(f) = to_frac(a.position) {
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            let g = [f[0] + dx as f64, f[1] + dy as f64, f[2] + dz as f64];
                            if g.iter().all(|v| (-0.05..=1.05).contains(v)) {
                                pts.push((cart(g), i));
                            }
                        }
                    }
                }
            }
        } else {
            pts.push((a.position, i));
        }
    }

    // Cell corners (periodic only) and the centre everything is drawn around.
    let corners: Vec<[f64; 3]> = if periodic {
        (0..8)
            .map(|k| {
                cart([
                    (k & 1) as f64,
                    ((k >> 1) & 1) as f64,
                    ((k >> 2) & 1) as f64,
                ])
            })
            .collect()
    } else {
        Vec::new()
    };
    let basis: Vec<[f64; 3]> = if periodic {
        corners.clone()
    } else {
        pts.iter().map(|(p, _)| *p).collect()
    };
    if basis.is_empty() {
        return;
    }
    let n = basis.len() as f64;
    let centre = [
        basis.iter().map(|p| p[0]).sum::<f64>() / n,
        basis.iter().map(|p| p[1]).sum::<f64>() / n,
        basis.iter().map(|p| p[2]).sum::<f64>() / n,
    ];
    let rel = |p: [f64; 3]| [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];

    // Fit the whole cell (or molecule) into the canvas.
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &basis {
        let (x, y, _) = iso(rel(*p));
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let margin = 28.0;
    let span_x = (max_x - min_x).max(1e-6);
    let span_y = (max_y - min_y).max(1e-6);
    let scale = ((w as f64 - 2.0 * margin) / span_x).min((h as f64 - 2.0 * margin) / span_y);
    let (cx, cy) = (
        w as f64 / 2.0 - (min_x + max_x) / 2.0 * scale,
        h as f64 / 2.0 - (min_y + max_y) / 2.0 * scale,
    );
    let screen = |p: [f64; 3]| -> (f64, f64, f64) {
        let (x, y, z) = iso(rel(p));
        (cx + x * scale, cy + y * scale, z)
    };

    // Atoms first, far to near, so nearer ones overlap farther ones.
    let mut drawn: Vec<(f64, f64, f64, usize)> = pts
        .iter()
        .map(|(p, i)| {
            let (x, y, z) = screen(*p);
            (x, y, z, *i)
        })
        .collect();
    drawn.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Cell wireframe, same weight and colour as the Slab canvas.
    if periodic {
        cr.set_line_width(1.5);
        cr.set_source_rgb(0.2, 0.2, 0.2);
        for k in 0..8usize {
            for bit in 0..3 {
                let m = 1 << bit;
                if k & m == 0 {
                    let (x0, y0, _) = screen(corners[k]);
                    let (x1, y1, _) = screen(corners[k | m]);
                    cr.move_to(x0, y0);
                    cr.line_to(x1, y1);
                }
            }
        }
        let _ = cr.stroke();
    }

    let radius = if drawn.len() > 600 { 3.5 } else { 6.0 };
    for (x, y, _, i) in drawn {
        let el = &structure.atoms[i].element;
        let (r, g, b) = active
            .style
            .element_colors
            .get(el)
            .copied()
            .unwrap_or_else(|| get_element_color(el, st.config.color_scheme));
        cr.new_path();
        cr.arc(x, y, radius, 0.0, 2.0 * PI);
        cr.set_source_rgba(r, g, b, 0.85);
        let _ = cr.fill();

        if hl.contains(&i) {
            // Dark ring (as the Slab canvas marks a selected atom): the atoms
            // an action would touch. Dark so it reads on any element colour.
            cr.new_path();
            cr.arc(x, y, radius + 2.5, 0.0, 2.0 * PI);
            cr.set_source_rgb(0.1, 0.1, 0.1);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
    }
}
