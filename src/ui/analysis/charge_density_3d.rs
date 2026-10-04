// src/ui/analysis/charge_density_3d.rs
//
// 3D page of the Charge Density window: isosurfaces of the loaded CHGCAR
// (or ρ_A − ρ_B, or the magnetisation) with atoms, cell and an optional
// density-coloured section plane, on the GPU.
//
// The page shares `ChargeDensityState` with the 2D slice page: files,
// difference mode and channel are chosen on the right-hand pane (they apply
// immediately); this page follows by polling a fingerprint. Its own
// controls (surface, section, display, report/export) go into the right
// pane in place of the slice controls while the 3D page is shown.
//
// Surfaces, their ambient occlusion and the enclosed charge are computed on
// a worker; every control change replaces the job in flight, so the slider
// stays live while the previous surface stays on screen.

mod figure_dialog;
mod scene_file;

use super::charge_density_tab::ChargeDensityState;
use crate::model::elements::{get_atom_cov, get_element_color, ColorScheme};
use crate::physics::analysis::charge_density::get_channel_data;
use crate::physics::analysis::isosurface::{isosurface, Grid, Inside, Mesh, Region};
use crate::physics::analysis::volumetric::{
    ambient_occlusion, enclosed, iso_for_charge_fraction, log_histogram, Enclosed, Occluders, Sign,
};
use crate::rendering::gl::{loader, AtomInstance, Camera, GlBackend, GpuBackend, Material, MeshData, Section, Volume};
use crate::utils::{console, task};
use gtk4::prelude::*;
use gtk4::{glib, Align, Orientation};
use nalgebra::{Matrix3, Rotation3, UnitQuaternion, Vector3};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const DRAG_DEG_PER_PX: f64 = 0.4;
/// Atoms are drawn small, as in VESTA's charge-density views, so they sit
/// inside the lobes instead of hiding them.
const ATOM_SCALE: f64 = 0.32;
const IMAGE_TOL: f64 = 0.02;
const BACKGROUND: [f32; 3] = [1.0, 1.0, 1.0];
const HIST_BINS: usize = 160;
/// How far (Å) neighbouring lobes and atoms still darken a surface.
const AO_REACH: f64 = 2.5;
const AO_RAYS: usize = 24;

/// Which lobes to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum Lobes {
    Positive,
    Negative,
    Both,
}

/// How the isovalue is set.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum IsoMode {
    /// An absolute value in e/Å³.
    Value,
    /// The value enclosing this fraction (0–1) of the charge.
    Fraction(f64),
}

/// The scalar field on screen, in e/Å³.
pub(super) struct Field {
    pub(super) data: Arc<Vec<f32>>,
    pub(super) dims: [usize; 3],
    pub(super) lattice: [[f64; 3]; 3],
    pub(super) label: String,
    pub(super) min: f32,
    pub(super) max: f32,
    default_iso: f64,
    pub(super) atoms: Vec<(String, [f64; 3])>,
    histogram: Vec<u32>,
    /// FNV-1a of the values: identifies the data a saved scene belongs to.
    pub(super) hash: u64,
}

impl Field {
    pub(super) fn abs_max(&self) -> f64 {
        (self.min.abs().max(self.max.abs()) as f64).max(1e-12)
    }
    fn has_negative(&self) -> bool {
        (self.min as f64) < -0.02 * self.abs_max()
    }
    fn has_positive(&self) -> bool {
        (self.max as f64) > 0.02 * self.abs_max()
    }
    /// A difference or spin density: both signs matter.
    pub(super) fn signed(&self) -> bool {
        self.has_negative() && self.has_positive()
    }
    pub(super) fn lattice_matrix(&self) -> Matrix3<f64> {
        lattice_matrix(&self.lattice)
    }
}

/// What the page needs to know to notice that the user changed the data.
#[derive(Clone, Copy, PartialEq)]
struct Fingerprint {
    a: usize,
    b: usize,
    diff: bool,
    channel: u8,
}

fn fingerprint(st: &ChargeDensityState) -> Option<Fingerprint> {
    let a = st.chgcar_a.as_ref()?;
    Some(Fingerprint {
        a: a.charge_total.as_ptr() as usize,
        b: st.chgcar_b.as_ref().map_or(0, |b| b.charge_total.as_ptr() as usize),
        diff: st.difference_mode,
        channel: st.channel as u8,
    })
}

/// |ρ| below which `frac` of the grid lies, from a bounded sample.
fn abs_percentile(data: &[f32], frac: f64) -> f64 {
    let stride = (data.len() / 500_000).max(1);
    let mut v: Vec<f32> = data.iter().step_by(stride).map(|x| x.abs()).filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return 0.0;
    }
    let k = ((v.len() - 1) as f64 * frac).round() as usize;
    v.select_nth_unstable_by(k, f32::total_cmp);
    v[k] as f64
}

fn fnv1a(data: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in data {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

fn build_field(st: &ChargeDensityState) -> Option<Field> {
    let c = st.active_chgcar()?;
    let values = get_channel_data(&c, st.channel, true)?;
    let data: Vec<f32> = values.iter().map(|&v| v as f32).collect();
    let (min, max) = data.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    // Surfaces around the densest ~5% of the cell: a readable first view for
    // total, difference and spin densities alike.
    let default_iso = abs_percentile(&data, 0.95).max(1e-6);
    let mut label = format!("{} density", st.channel.label());
    if st.difference_mode {
        label = format!("Δρ = ρ(A) − ρ(B), {}", st.channel.label().to_lowercase());
    }
    let mut f = Field {
        dims: c.grid,
        lattice: c.lattice,
        label,
        min,
        max,
        default_iso,
        atoms: c.atoms.iter().map(|a| (a.element.clone(), a.frac_coords)).collect(),
        histogram: Vec::new(),
        hash: fnv1a(&data),
        data: Arc::new(data),
    };
    let (lo, hi) = iso_range(&f);
    f.histogram = log_histogram(&f.data, lo, hi, HIST_BINS);
    Some(f)
}

fn lattice_matrix(l: &[[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::new(l[0][0], l[1][0], l[2][0], l[0][1], l[1][1], l[2][1], l[0][2], l[1][2], l[2][2])
}

/// Rotation (main-view convention: screen y down, +z into the screen) that
/// looks along `dir` with `up` pointing up on screen.
fn look_along(dir: Vector3<f64>, up: Vector3<f64>) -> UnitQuaternion<f64> {
    let z = dir.normalize();
    let mut u = up - z * up.dot(&z);
    if u.norm() < 1e-6 {
        u = if z.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
        u -= z * u.dot(&z);
    }
    let y = -u.normalize(); // screen y points down
    let x = y.cross(&z);
    let r = Matrix3::from_rows(&[x.transpose(), y.transpose(), z.transpose()]);
    UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(r))
}

/// A density section through the cell, by Miller indices.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct SectionState {
    pub(super) show: bool,
    /// Remove everything on the far side of the plane.
    pub(super) cut: bool,
    pub(super) flip: bool,
    pub(super) hkl: [i32; 3],
    /// Position across the cell along the plane normal, 0–1.
    pub(super) pos: f64,
}

impl Default for SectionState {
    fn default() -> Self {
        Self { show: false, cut: false, flip: false, hkl: [0, 0, 1], pos: 0.5 }
    }
}

/// The section plane in world space: unit normal n and offset d (n·x = d),
/// or `None` for hkl = 000.
fn section_plane(f: &Field, s: &SectionState) -> Option<(Vector3<f64>, f64)> {
    let h = Vector3::new(s.hkl[0] as f64, s.hkl[1] as f64, s.hkl[2] as f64);
    if h.norm() < 1e-9 {
        return None;
    }
    // h·frac = c, frac = M⁻¹x  ⇒  (M⁻ᵀh)·x = c.
    let g = f.lattice_matrix().try_inverse()?.transpose() * h;
    let (cmin, cmax) = (h.iter().map(|v| v.min(0.0)).sum::<f64>(), h.iter().map(|v| v.max(0.0)).sum::<f64>());
    let c = cmin + s.pos.clamp(0.0, 1.0) * (cmax - cmin);
    let len = g.norm();
    Some((g / len, c / len))
}

/// One drawn isosurface with what was computed for it.
pub(super) struct Lobe {
    pub(super) mesh: Mesh,
    pub(super) ao: Option<Vec<f32>>,
    pub(super) enclosed: Enclosed,
}

pub(super) struct View {
    pub(super) field: Option<Field>,
    fingerprint: Option<Fingerprint>,
    pub(super) iso: f64,
    pub(super) iso_mode: IsoMode,
    pub(super) lobes: Lobes,
    pub(super) opacity: f32,
    pub(super) color_pos: [f32; 3],
    pub(super) color_neg: [f32; 3],
    pub(super) ao: bool,
    pub(super) ao_strength: f32,
    pub(super) show_atoms: bool,
    pub(super) show_cell: bool,
    pub(super) section: SectionState,
    pub(super) scheme: ColorScheme,
    pub(super) camera: Camera,
    needs_fit: bool,
    pub(super) backend: Option<GlBackend>,
    /// Current surfaces (positive, negative); kept on the CPU for reports
    /// and mesh export.
    pub(super) lobes_now: [Option<Arc<Lobe>>; 2],
    meshes_dirty: bool,
    scene_dirty: bool,
    volume_dirty: bool,
    job: Option<task::JobHandle>,
    stats: String,
    busy: bool,
    /// Aspect ratio of the figure being set up, drawn as a frame.
    pub(super) export_frame: Option<f64>,
}

#[derive(Clone)]
pub(super) struct Ui {
    pub(super) area: glib::WeakRef<gtk4::GLArea>,
    hud: glib::WeakRef<gtk4::DrawingArea>,
    hist: glib::WeakRef<gtk4::DrawingArea>,
    iso_scale: glib::WeakRef<gtk4::Scale>,
    iso_spin: glib::WeakRef<gtk4::SpinButton>,
    lobes: glib::WeakRef<gtk4::DropDown>,
    placeholder: glib::WeakRef<gtk4::Label>,
}

/// Slider position (0..1, logarithmic) ↔ isovalue.
fn iso_range(f: &Field) -> (f64, f64) {
    let hi = f.abs_max() * 0.999;
    (hi * 1e-4, hi)
}
fn iso_to_slider(f: &Field, iso: f64) -> f64 {
    let (lo, hi) = iso_range(f);
    ((iso.clamp(lo, hi) / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0)
}
fn slider_to_iso(f: &Field, s: f64) -> f64 {
    let (lo, hi) = iso_range(f);
    lo * (hi / lo).powf(s)
}

pub(super) fn redraw(ui: &Ui) {
    if let Some(a) = ui.area.upgrade() {
        a.queue_render();
    }
    for d in [&ui.hud, &ui.hist] {
        if let Some(d) = d.upgrade() {
            d.queue_draw();
        }
    }
}

/// Atom spheres including images on the cell faces: (element, centre, radius).
pub(super) fn atom_spheres(f: &Field) -> Vec<(String, Vector3<f64>, f64)> {
    let m = f.lattice_matrix();
    let mut out = Vec::new();
    for (el, fr) in &f.atoms {
        let base = Vector3::new(fr[0].rem_euclid(1.0), fr[1].rem_euclid(1.0), fr[2].rem_euclid(1.0));
        let shifts = |x: f64| -> Vec<f64> {
            let mut s = vec![0.0];
            if x < IMAGE_TOL {
                s.push(1.0);
            }
            if x > 1.0 - IMAGE_TOL {
                s.push(-1.0);
            }
            s
        };
        let radius = get_atom_cov(el) * ATOM_SCALE;
        for sx in shifts(base.x) {
            for sy in shifts(base.y) {
                for sz in shifts(base.z) {
                    out.push((el.clone(), m * (base + Vector3::new(sx, sy, sz)), radius));
                }
            }
        }
    }
    out
}

/// If the isovalue follows an enclosed-charge fraction, recompute it.
pub(super) fn apply_iso_mode(v: &mut View) {
    if let (IsoMode::Fraction(frac), Some(f)) = (v.iso_mode, v.field.as_ref()) {
        let sign = if v.lobes == Lobes::Negative { Sign::Negative } else { Sign::Positive };
        if let Some(iso) = iso_for_charge_fraction(&f.data, frac, sign) {
            v.iso = iso;
        }
    }
}

/// Push the view's isovalue into the slider and spin box.
pub(super) fn sync_iso_controls(view: &Rc<RefCell<View>>, ui: &Ui, syncing: &Cell<bool>) {
    syncing.set(true);
    let v = view.borrow();
    if let Some(f) = &v.field {
        if let Some(s) = ui.iso_spin.upgrade() {
            s.set_value(v.iso);
        }
        if let Some(s) = ui.iso_scale.upgrade() {
            s.set_value(iso_to_slider(f, v.iso));
        }
    }
    drop(v);
    syncing.set(false);
}

/// Start meshing the current field (surfaces, occlusion, enclosed charge),
/// replacing any job in flight.
pub(super) fn remesh(view: &Rc<RefCell<View>>, ui: &Ui) {
    let mut v = view.borrow_mut();
    let Some(f) = v.field.as_ref() else { return };
    let (data, dims, lattice) = (f.data.clone(), f.dims, f.lattice);
    let (iso, lobes, ao_on) = (v.iso as f32, v.lobes, v.ao);
    let spheres: Vec<([f64; 3], f64)> = if v.show_atoms {
        atom_spheres(f).into_iter().map(|(_, c, r)| ([c.x, c.y, c.z], r)).collect()
    } else {
        Vec::new()
    };
    v.busy = true;
    let weak = Rc::downgrade(view);
    let ui2 = ui.clone();
    v.job = Some(task::spawn(
        move |cancel| {
            let t = std::time::Instant::now();
            let grid = Grid { data: &data, dims, lattice };
            let region = Region::cell(dims);
            let want_pos = lobes != Lobes::Negative;
            let want_neg = lobes != Lobes::Positive;
            let occ = Occluders {
                iso_pos: want_pos.then_some(iso as f64),
                iso_neg: want_neg.then_some(iso as f64),
                spheres: &spheres,
            };
            let lobe = |inside: Inside| -> Option<Arc<Lobe>> {
                let (level, sign) = match inside {
                    Inside::Above => (iso, Sign::Positive),
                    Inside::Below => (-iso, Sign::Negative),
                };
                let mesh = isosurface(&grid, level, inside, region, &cancel)?;
                let ao = if ao_on {
                    Some(ambient_occlusion(&grid, &mesh.positions, &mesh.normals, &occ, AO_REACH, AO_RAYS, &cancel)?)
                } else {
                    None
                };
                Some(Arc::new(Lobe { mesh, ao, enclosed: enclosed(&data, &lattice, iso as f64, sign) }))
            };
            let pos = if want_pos { Some(lobe(Inside::Above)?) } else { None };
            let neg = if want_neg { Some(lobe(Inside::Below)?) } else { None };
            Some((pos, neg, t.elapsed()))
        },
        move |(pos, neg, took)| {
            let Some(view) = weak.upgrade() else { return };
            {
                let mut v = view.borrow_mut();
                let tris: usize = [&pos, &neg].iter().filter_map(|l| l.as_ref()).map(|l| l.mesh.triangle_count()).sum();
                v.stats = format!("{} triangles · {:.0} ms", tris, took.as_secs_f64() * 1e3);
                v.lobes_now = [pos, neg];
                v.meshes_dirty = true;
                v.busy = false;
            }
            redraw(&ui2);
        },
    ));
    drop(v);
    redraw(ui);
}

/// Atoms and cell edges.
fn upload_scene(backend: &mut GlBackend, f: &Field, show_atoms: bool, show_cell: bool, scheme: ColorScheme) {
    let mut atoms = Vec::new();
    if show_atoms {
        for (el, c, r) in atom_spheres(f) {
            let col = get_element_color(&el, scheme);
            atoms.push(AtomInstance {
                pos: [c.x as f32, c.y as f32, c.z as f32],
                radius: r as f32,
                color: [col.0 as f32, col.1 as f32, col.2 as f32],
            });
        }
    }
    backend.set_atoms(&atoms);
    let m = f.lattice_matrix();
    let mut lines = Vec::new();
    if show_cell {
        let corner = |k: u32| {
            let v = m * Vector3::new((k & 1) as f64, ((k >> 1) & 1) as f64, ((k >> 2) & 1) as f64);
            [v.x as f32, v.y as f32, v.z as f32]
        };
        for i in 0..8u32 {
            for axis in 0..3 {
                if i & (1 << axis) == 0 {
                    lines.push(corner(i));
                    lines.push(corner(i | (1 << axis)));
                }
            }
        }
    }
    backend.set_lines(&lines, [0.25, 0.25, 0.28]);
}

/// Push section and clip settings to the backend.
fn apply_section(backend: &mut GlBackend, v: &View) {
    let Some(f) = v.field.as_ref() else { return };
    let plane = if v.section.show || v.section.cut { section_plane(f, &v.section) } else { None };
    match plane {
        Some((n, d)) => {
            let m = f.lattice_matrix();
            let center = m * Vector3::new(0.5, 0.5, 0.5);
            let reach = (0..8)
                .map(|k| (m * Vector3::new((k & 1) as f64, ((k >> 1) & 1) as f64, ((k >> 2) & 1) as f64) - center).norm())
                .fold(0.0, f64::max)
                * 1.05;
            let origin = center - n * (n.dot(&center) - d);
            let any = if n.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
            let e1 = n.cross(&any).normalize() * reach;
            let e2 = n.cross(&e1).normalize() * reach;
            let to = |x: Vector3<f64>| [x.x as f32, x.y as f32, x.z as f32];
            let signed = f.signed();
            backend.set_section(v.section.show.then_some(Section {
                origin: to(origin),
                e1: to(e1),
                e2: to(e2),
                signed,
                lo: if signed { 0.0 } else { (f.abs_max() * 1e-4) as f32 },
                // Signed maps saturate at the isovalue, matching the surfaces.
                hi: if signed { v.iso as f32 } else { f.abs_max() as f32 },
                iso: v.iso as f32,
            }));
            // Keep the side the normal points away from (or the other one),
            // a hair beyond the plane so the section itself is not clipped.
            let s = if v.section.flip { -1.0 } else { 1.0 };
            backend.set_clip(v.section.cut.then_some((to(n * s), (d * s + 1e-3) as f32)));
        }
        None => {
            backend.set_section(None);
            backend.set_clip(None);
        }
    }
}

fn draw_hud(cr: &gtk4::cairo::Context, w: f64, h: f64, v: &View) {
    let Some(f) = v.field.as_ref() else { return };
    cr.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_source_rgba(0.08, 0.08, 0.1, 0.9);
    cr.set_font_size(15.0);
    cr.move_to(14.0, 24.0);
    let _ = cr.show_text(&f.label);
    cr.set_font_size(12.0);
    cr.set_source_rgba(0.08, 0.08, 0.1, 0.7);
    let mode = match v.iso_mode {
        IsoMode::Value => String::new(),
        IsoMode::Fraction(fr) => format!("  (encloses {:.0}% of the charge)", fr * 100.0),
    };
    let sign = match v.lobes {
        Lobes::Both => "±",
        Lobes::Positive => "+",
        Lobes::Negative => "−",
    };
    cr.move_to(14.0, 42.0);
    let _ = cr.show_text(&format!("isosurface {sign}{:.5} e/Å³{mode}", v.iso));
    cr.move_to(14.0, 58.0);
    let status = if v.busy { "computing…".to_string() } else { v.stats.clone() };
    let _ = cr.show_text(&format!("range {:.3} … {:.3} e/Å³   {}", f.min, f.max, status));

    let mut y = 76.0;
    for (slot, color, name) in [(0usize, v.color_pos, "ρ > +iso"), (1, v.color_neg, "ρ < −iso")] {
        let Some(l) = &v.lobes_now[slot] else { continue };
        cr.set_source_rgb(color[0] as f64, color[1] as f64, color[2] as f64);
        cr.rectangle(14.0, y - 9.0, 12.0, 10.0);
        let _ = cr.fill();
        cr.set_source_rgba(0.08, 0.08, 0.1, 0.7);
        cr.move_to(32.0, y);
        let e = l.enclosed;
        let _ = cr.show_text(&format!(
            "{name}: {:.3} e in {:.2} Å³ ({:.1}% of the charge)",
            e.electrons,
            e.volume,
            e.charge_fraction * 100.0
        ));
        y += 16.0;
    }

    // Figure frame: what the export will contain.
    if let Some(aspect) = v.export_frame {
        let (fw, fh) = frame_rect(w, h, aspect);
        let (x0, y0) = ((w - fw) / 2.0, (h - fh) / 2.0);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.18);
        cr.rectangle(0.0, 0.0, w, h);
        cr.rectangle(x0, y0, fw, fh);
        cr.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
        let _ = cr.fill();
        cr.set_source_rgba(0.85, 0.2, 0.15, 0.9);
        cr.set_line_width(1.5);
        cr.set_dash(&[6.0, 4.0], 0.0);
        cr.rectangle(x0, y0, fw, fh);
        let _ = cr.stroke();
    }
}

/// Zoom so the cell and atoms fill the figure frame (aspect = width/height)
/// on a `cw`×`ch` canvas, keeping the orientation.
pub(super) fn fit_into_frame(v: &mut View, cw: f64, ch: f64, aspect: f64) {
    let Some(f) = v.field.as_ref() else { return };
    let m = f.lattice_matrix();
    let mut pts: Vec<(Vector3<f64>, f64)> = (0..8)
        .map(|c| (m * Vector3::new((c & 1) as f64, ((c >> 1) & 1) as f64, ((c >> 2) & 1) as f64), 0.0))
        .collect();
    if v.show_atoms {
        pts.extend(atom_spheres(f).into_iter().map(|(_, c, r)| (c, r)));
    }
    let rot = v.camera.rotation.to_rotation_matrix();
    let (mut hx, mut hy) = (1e-3_f64, 1e-3_f64);
    for (p, r) in pts {
        let q = rot * (p - v.camera.center);
        hx = hx.max(q.x.abs() + r);
        hy = hy.max(q.y.abs() + r);
    }
    let (fw, fh) = frame_rect(cw, ch, aspect);
    v.camera.zoom = 1.0;
    let (_, hh0) = v.camera.half_extents(cw, ch);
    // At zoom z the frame spans fw·2·hh0/(z·ch) Å across.
    let z = (0.95 * fw * hh0 / (ch * hx)).min(0.95 * fh * hh0 / (ch * hy));
    v.camera.zoom = z.clamp(0.05, 50.0);
    v.needs_fit = false;
}

/// The figure frame inside a `w`×`h` canvas for `aspect` = width/height.
pub(super) fn frame_rect(w: f64, h: f64, aspect: f64) -> (f64, f64) {
    let (mw, mh) = (w * 0.94, h * 0.94);
    if mw / mh > aspect {
        (mh * aspect, mh)
    } else {
        (mw, mw / aspect)
    }
}

fn draw_histogram(cr: &gtk4::cairo::Context, w: f64, h: f64, v: &View) {
    let Some(f) = v.field.as_ref() else { return };
    let hist = &f.histogram;
    let max = hist.iter().copied().max().unwrap_or(0).max(1) as f64;
    let n = hist.len().max(1) as f64;
    cr.set_source_rgb(0.97, 0.97, 0.98);
    let _ = cr.paint();
    cr.set_source_rgba(0.35, 0.45, 0.65, 0.55);
    for (i, &c) in hist.iter().enumerate() {
        if c == 0 {
            continue;
        }
        // Log counts: tails stay visible next to the bulk.
        let bh = (h - 4.0) * ((c as f64).ln_1p() / max.ln_1p());
        cr.rectangle(w * i as f64 / n, h - bh, w / n + 0.3, bh);
    }
    let _ = cr.fill();
    let x = w * iso_to_slider(f, v.iso);
    cr.set_source_rgb(0.85, 0.2, 0.15);
    cr.set_line_width(1.5);
    cr.move_to(x, 0.0);
    cr.line_to(x, h);
    let _ = cr.stroke();
}

pub use figure_dialog::FigureSettings;

/// Widgets the 3D page contributes to the window.
pub struct Parts {
    /// The 3D canvas.
    pub page: gtk4::Box,
    /// Histogram, isovalue and opacity: the window's bottom bar in 3D mode.
    pub bottom: gtk4::Box,
    /// Surface, section, display and export controls (the right pane).
    pub controls: gtk4::Box,
    /// Programmatic access (scripting, tests).
    pub handle: Handle,
}

/// Drive the 3D page without its controls (the controls are not updated to
/// match; this is for scripting and tests).
#[derive(Clone)]
pub struct Handle {
    view: Rc<RefCell<View>>,
    ui: Ui,
}

impl Handle {
    /// Render and write a figure (format from the extension).
    pub fn export_figure(&self, path: &std::path::Path, settings: &FigureSettings) -> Result<String, String> {
        figure_dialog::export_with(path, &self.view, &self.ui, settings)
    }

    /// Show a section plane (Miller indices, position 0–1), optionally
    /// cutting away the far side.
    pub fn set_section(&self, hkl: [i32; 3], pos: f64, cut: bool) {
        self.view.borrow_mut().section = SectionState { show: true, cut, flip: false, hkl, pos };
        redraw(&self.ui);
    }

    /// Set the isovalue so the surface encloses `fraction` of the charge.
    pub fn enclose(&self, fraction: f64) {
        {
            let mut v = self.view.borrow_mut();
            v.iso_mode = IsoMode::Fraction(fraction);
            apply_iso_mode(&mut v);
        }
        remesh(&self.view, &self.ui);
    }

    /// Current surface report (as written to Structure Info).
    pub fn report(&self) -> Option<String> {
        let v = self.view.borrow();
        v.field.as_ref().map(|f| enclosed_report(f, &v))
    }

    /// True while surfaces are being computed.
    pub fn busy(&self) -> bool {
        self.view.borrow().busy
    }
}

fn section(title: &str) -> (gtk4::Box, gtk4::Box) {
    let body = crate::ui::style::card_body(6);
    (crate::ui::style::group(title, &body), body)
}

fn row(label: &str, w: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let b = gtk4::Box::new(Orientation::Horizontal, 6);
    let l = gtk4::Label::new(Some(label));
    l.set_halign(Align::Start);
    l.set_hexpand(true);
    b.append(&l);
    b.append(w);
    b
}

/// Presets: lobes, colours and how the isovalue is chosen.
const PRESETS: [&str; 4] = ["Custom", "Total density", "Bonding / difference (±)", "Spin density (±)"];

/// Build the 3D page for `state` (shared with the 2D page).
pub fn build(state: Rc<RefCell<ChargeDensityState>>, scheme: ColorScheme) -> Parts {
    let view = Rc::new(RefCell::new(View {
        field: None,
        fingerprint: None,
        iso: 0.0,
        iso_mode: IsoMode::Value,
        lobes: Lobes::Positive,
        opacity: 0.75,
        color_pos: [0.97, 0.80, 0.18], // VESTA-like yellow
        color_neg: [0.20, 0.72, 0.90], // cyan
        ao: true,
        ao_strength: 0.7,
        show_atoms: true,
        show_cell: true,
        section: SectionState::default(),
        scheme,
        camera: Camera::new(Vector3::zeros(), 1.0, UnitQuaternion::identity()),
        needs_fit: true,
        backend: None,
        lobes_now: [None, None],
        meshes_dirty: false,
        scene_dirty: true,
        volume_dirty: true,
        job: None,
        stats: String::new(),
        busy: false,
        export_frame: None,
    }));

    // ================= left: canvas, histogram, isovalue =================
    let area = gtk4::GLArea::new();
    area.set_required_version(3, 3);
    area.set_has_depth_buffer(true);
    area.set_hexpand(true);
    area.set_vexpand(true);
    let hud = gtk4::DrawingArea::new();
    hud.set_can_target(false);
    let placeholder = gtk4::Label::new(Some("Load a CHGCAR on the right to see its isosurfaces."));
    placeholder.add_css_class("dim-label");
    placeholder.set_wrap(true);
    placeholder.set_halign(Align::Center);
    placeholder.set_valign(Align::Center);
    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&hud);
    overlay.add_overlay(&placeholder);
    overlay.set_vexpand(true);

    let hist = gtk4::DrawingArea::new();
    hist.set_content_height(34);
    hist.set_tooltip_text(Some("Distribution of |ρ| (log scale); click to set the isovalue"));
    let iso_scale = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.001);
    iso_scale.set_draw_value(false);
    iso_scale.set_hexpand(true);
    iso_scale.set_tooltip_text(Some("Isovalue (logarithmic)"));
    let iso_spin = gtk4::SpinButton::with_range(1e-6, 1e4, 1e-4);
    iso_spin.set_digits(5);
    iso_spin.set_width_chars(9);
    let iso_row = gtk4::Box::new(Orientation::Horizontal, 8);
    iso_row.append(&gtk4::Label::new(Some("Isovalue")));
    iso_row.append(&iso_scale);
    iso_row.append(&iso_spin);
    iso_row.append(&gtk4::Label::new(Some("e/Å³")));
    for w in [hist.upcast_ref::<gtk4::Widget>(), iso_row.upcast_ref()] {
        w.set_margin_start(6);
        w.set_margin_end(6);
    }
    iso_row.set_margin_bottom(4);

    let page = gtk4::Box::new(Orientation::Vertical, 0);
    page.append(&overlay);
    // The sliders shaping the picture live in the bottom bar.
    let bottom = gtk4::Box::new(Orientation::Vertical, 4);
    bottom.append(&hist);
    bottom.append(&iso_row);

    // ================= right: controls =================
    let controls = gtk4::Box::new(Orientation::Vertical, 8);

    let preset = gtk4::DropDown::from_strings(&PRESETS);
    let (g_preset, b_preset) = section("Preset");
    b_preset.append(&preset);
    controls.append(&g_preset);

    let (g_surf, b_surf) = section("Surface");
    let lobes_dd = gtk4::DropDown::from_strings(&["+ lobes", "− lobes", "± lobes"]);
    b_surf.append(&row("Lobes", &lobes_dd));
    let mode_dd = gtk4::DropDown::from_strings(&["Isovalue", "Enclosed charge"]);
    mode_dd.set_tooltip_text(Some("Set the surface by an isovalue, or by the fraction of the charge it encloses"));
    b_surf.append(&row("Set by", &mode_dd));
    let percent = gtk4::SpinButton::with_range(1.0, 99.9, 1.0);
    percent.set_digits(1);
    percent.set_value(50.0);
    percent.set_sensitive(false);
    b_surf.append(&row("Enclosed charge (%)", &percent));
    let opacity = gtk4::Scale::with_range(Orientation::Horizontal, 0.15, 1.0, 0.05);
    opacity.set_value(view.borrow().opacity as f64);
    opacity.set_draw_value(false);
    opacity.set_size_request(120, -1);
    opacity.set_tooltip_text(Some("Surface opacity"));
    iso_row.append(&gtk4::Separator::new(Orientation::Vertical));
    iso_row.append(&gtk4::Label::new(Some("Opacity")));
    iso_row.append(&opacity);
    let to_rgba = |c: [f32; 3]| gtk4::gdk::RGBA::new(c[0], c[1], c[2], 1.0);
    let col_pos = gtk4::ColorButton::with_rgba(&to_rgba(view.borrow().color_pos));
    let col_neg = gtk4::ColorButton::with_rgba(&to_rgba(view.borrow().color_neg));
    let colors = gtk4::Box::new(Orientation::Horizontal, 6);
    colors.append(&col_pos);
    colors.append(&col_neg);
    b_surf.append(&row("Colours (+ / −)", &colors));
    let ao = gtk4::CheckButton::with_label("Ambient occlusion");
    ao.set_active(true);
    ao.set_tooltip_text(Some("Darken crevices and contacts between lobes and atoms (depth cue)"));
    let ao_strength = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    ao_strength.set_value(view.borrow().ao_strength as f64);
    ao_strength.set_draw_value(false);
    ao_strength.set_size_request(120, -1);
    let ao_row = gtk4::Box::new(Orientation::Horizontal, 6);
    ao.set_hexpand(true);
    ao_row.append(&ao);
    ao_row.append(&ao_strength);
    b_surf.append(&ao_row);
    controls.append(&g_surf);

    let (g_sec, b_sec) = section("Section plane");
    let sec_show = gtk4::CheckButton::with_label("Show density section");
    b_sec.append(&sec_show);
    let hkl_box = gtk4::Box::new(Orientation::Horizontal, 4);
    let spin = |v: f64| {
        let s = gtk4::SpinButton::with_range(-9.0, 9.0, 1.0);
        s.set_value(v);
        s.set_width_chars(2);
        s
    };
    let (sh, sk, sl) = (spin(0.0), spin(0.0), spin(1.0));
    for (name, s) in [("h", &sh), ("k", &sk), ("l", &sl)] {
        hkl_box.append(&gtk4::Label::new(Some(name)));
        hkl_box.append(s);
    }
    b_sec.append(&hkl_box);
    let sec_pos = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.005);
    sec_pos.set_value(0.5);
    sec_pos.set_draw_value(false);
    sec_pos.set_size_request(120, -1);
    b_sec.append(&row("Position", &sec_pos));
    let sec_cut = gtk4::CheckButton::with_label("Cut away beyond the plane");
    let sec_flip = gtk4::CheckButton::with_label("Flip side");
    b_sec.append(&sec_cut);
    b_sec.append(&sec_flip);
    controls.append(&g_sec);

    let (g_disp, b_disp) = section("Display");
    let atoms = gtk4::CheckButton::with_label("Atoms");
    atoms.set_active(true);
    let cell = gtk4::CheckButton::with_label("Cell");
    cell.set_active(true);
    let toggles = gtk4::Box::new(Orientation::Horizontal, 12);
    toggles.append(&atoms);
    toggles.append(&cell);
    b_disp.append(&toggles);
    let views = gtk4::Box::new(Orientation::Horizontal, 4);
    views.append(&gtk4::Label::new(Some("View along")));
    let view_btn = |label: &str, tip: &str| {
        let b = gtk4::Button::with_label(label);
        b.set_tooltip_text(Some(tip));
        b
    };
    let along_a = view_btn("a", "Look along a");
    let along_b = view_btn("b", "Look along b");
    let along_c = view_btn("c", "Look along c");
    let fit = view_btn("Fit", "Fit the cell to the view");
    for b in [&along_a, &along_b, &along_c, &fit] {
        views.append(b);
    }
    b_disp.append(&views);
    controls.append(&g_disp);

    let (g_out, b_out) = section("Report and export");
    let report = gtk4::Button::with_label("Report Enclosed Charge");
    let figure = gtk4::Button::with_label("Export Figure…");
    figure.add_css_class("suggested-action");
    figure.set_tooltip_text(Some("Journal-ready PDF/SVG/PNG/TIFF at a physical size and dpi"));
    let mesh_btn = gtk4::Button::with_label("Export Mesh (OBJ/PLY)…");
    let save_scene = gtk4::Button::with_label("Save Scene…");
    let load_scene = gtk4::Button::with_label("Load Scene…");
    for b in [&figure, &report, &mesh_btn, &save_scene, &load_scene] {
        b_out.append(b);
    }
    controls.append(&g_out);

    let ui = Ui {
        area: area.downgrade(),
        hud: hud.downgrade(),
        hist: hist.downgrade(),
        iso_scale: iso_scale.downgrade(),
        iso_spin: iso_spin.downgrade(),
        lobes: lobes_dd.downgrade(),
        placeholder: placeholder.downgrade(),
    };
    // Set while code moves controls itself, so handlers ignore it.
    let syncing = Rc::new(Cell::new(false));

    // ---- GL lifecycle ----
    {
        let view = view.clone();
        let ph = placeholder.downgrade();
        area.connect_realize(move |a| {
            a.make_current();
            let result = match a.error() {
                Some(e) => Err(e.to_string()),
                None => loader::create_context().and_then(GlBackend::new),
            };
            match result {
                Ok(b) => {
                    let mut v = view.borrow_mut();
                    v.backend = Some(b);
                    v.scene_dirty = true;
                    v.meshes_dirty = true;
                    v.volume_dirty = true;
                }
                Err(e) => {
                    console::log_error(&format!("Charge density 3D: OpenGL 3.3 is not available ({e})"));
                    if let Some(l) = ph.upgrade() {
                        l.set_text(&format!("OpenGL 3.3 is not available, so the 3D view cannot be shown.\n\n{e}"));
                        l.set_visible(true);
                    }
                }
            }
        });
    }
    {
        let view = view.clone();
        area.connect_unrealize(move |a| {
            a.make_current();
            if let Some(mut b) = view.borrow_mut().backend.take() {
                b.destroy();
            }
        });
    }
    {
        let view = view.clone();
        area.connect_render(move |a, _| {
            let mut guard = view.borrow_mut();
            let v = &mut *guard;
            let sf = a.scale_factor();
            let (w, h) = (a.width() * sf, a.height() * sf);
            prepare_backend(v);
            let (Some(backend), Some(f)) = (v.backend.as_mut(), v.field.as_ref()) else {
                return glib::Propagation::Proceed;
            };
            if v.needs_fit && w > 0 && h > 0 {
                let m = f.lattice_matrix();
                let pts: Vec<Vector3<f64>> = (0..8)
                    .map(|c| m * Vector3::new((c & 1) as f64, ((c >> 1) & 1) as f64, ((c >> 2) & 1) as f64))
                    .collect();
                v.camera.fit(&pts, 0.5, w as f64 / h as f64);
                v.needs_fit = false;
            }
            backend.set_line_width(1.5 * sf as f32);
            backend.draw(&v.camera, w, h, BACKGROUND);
            glib::Propagation::Stop
        });
    }
    {
        let view = view.clone();
        hud.set_draw_func(move |_, cr, w, h| draw_hud(cr, w as f64, h as f64, &view.borrow()));
    }
    {
        let view = view.clone();
        hist.set_draw_func(move |_, cr, w, h| draw_histogram(cr, w as f64, h as f64, &view.borrow()));
    }

    // ---- follow the data chosen on the right-hand pane ----
    {
        let (view, state, ui, syncing) = (view.clone(), state.clone(), ui.clone(), syncing.clone());
        let poll = move || {
            let fp = fingerprint(&state.borrow());
            if fp == view.borrow().fingerprint {
                return;
            }
            let field = fp.and_then(|_| build_field(&state.borrow()));
            {
                let mut v = view.borrow_mut();
                v.fingerprint = fp;
                v.lobes_now = [None, None];
                v.meshes_dirty = true;
                v.scene_dirty = true;
                v.volume_dirty = true;
                if let Some(f) = &field {
                    let first = v.field.is_none();
                    v.iso = f.default_iso;
                    v.lobes = match (f.has_positive(), f.has_negative()) {
                        (true, true) => Lobes::Both,
                        (false, true) => Lobes::Negative,
                        _ => Lobes::Positive,
                    };
                    let m = f.lattice_matrix();
                    let center = m * Vector3::new(0.5, 0.5, 0.5);
                    let radius = (0..8)
                        .map(|c| (m * Vector3::new((c & 1) as f64, ((c >> 1) & 1) as f64, ((c >> 2) & 1) as f64) - center).norm())
                        .fold(1.0, f64::max);
                    // A tilted three-quarter view shows depth on first open.
                    let rotation = if first { UnitQuaternion::from_euler_angles(-0.35, 0.55, 0.0) } else { v.camera.rotation };
                    v.camera = Camera::new(center, radius, rotation);
                    v.needs_fit = true;
                }
                v.field = field;
                apply_iso_mode(&mut v);
            }
            if let Some(p) = ui.placeholder.upgrade() {
                p.set_visible(view.borrow().field.is_none());
            }
            {
                let v = view.borrow();
                if let Some(f) = &v.field {
                    let (lo, hi) = iso_range(f);
                    syncing.set(true);
                    if let Some(s) = ui.iso_spin.upgrade() {
                        s.set_range(lo, hi);
                        s.set_increments(lo, lo * 100.0);
                    }
                    if let Some(d) = ui.lobes.upgrade() {
                        d.set_selected(match v.lobes {
                            Lobes::Positive => 0,
                            Lobes::Negative => 1,
                            Lobes::Both => 2,
                        });
                    }
                    syncing.set(false);
                }
            }
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        };
        poll();
        let area_w = area.downgrade();
        glib::timeout_add_local(Duration::from_millis(300), move || {
            if area_w.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            poll();
            glib::ControlFlow::Continue
        });
    }

    // ---- isovalue: slider, spin box, histogram click ----
    let set_iso = {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        Rc::new(move |iso: f64| {
            {
                let mut v = view.borrow_mut();
                v.iso = iso;
                v.iso_mode = IsoMode::Value; // a hand-picked value ends "enclosed %" mode
            }
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        })
    };
    // Picking a value by hand switches "Set by" back to Isovalue.
    let to_value_mode = {
        let (mode, pct, syncing) = (mode_dd.downgrade(), percent.downgrade(), syncing.clone());
        Rc::new(move || {
            syncing.set(true);
            if let Some(m) = mode.upgrade() {
                m.set_selected(0);
            }
            if let Some(p) = pct.upgrade() {
                p.set_sensitive(false);
            }
            syncing.set(false);
        })
    };
    {
        let (view, syncing, set_iso, tvm) = (view.clone(), syncing.clone(), set_iso.clone(), to_value_mode.clone());
        iso_scale.connect_value_changed(move |s| {
            if syncing.get() {
                return;
            }
            let iso = match view.borrow().field.as_ref() {
                Some(f) => slider_to_iso(f, s.value()),
                None => return,
            };
            tvm();
            set_iso(iso);
        });
    }
    {
        let (syncing, set_iso, tvm) = (syncing.clone(), set_iso.clone(), to_value_mode.clone());
        iso_spin.connect_value_changed(move |s| {
            if syncing.get() {
                return;
            }
            tvm();
            set_iso(s.value());
        });
    }
    {
        let click = gtk4::GestureClick::new();
        let (view, set_iso, tvm) = (view.clone(), set_iso.clone(), to_value_mode.clone());
        let hist_w = hist.downgrade();
        click.connect_pressed(move |_, _, x, _| {
            let Some(hw) = hist_w.upgrade() else { return };
            let iso = match view.borrow().field.as_ref() {
                Some(f) => slider_to_iso(f, (x / hw.width().max(1) as f64).clamp(0.0, 1.0)),
                None => return,
            };
            tvm();
            set_iso(iso);
        });
        hist.add_controller(click);
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        let pct = percent.downgrade();
        mode_dd.connect_selected_notify(move |d| {
            if syncing.get() {
                return;
            }
            let by_charge = d.selected() == 1;
            if let Some(p) = pct.upgrade() {
                p.set_sensitive(by_charge);
            }
            {
                let mut v = view.borrow_mut();
                v.iso_mode = if by_charge {
                    IsoMode::Fraction(pct.upgrade().map_or(0.5, |p| p.value() / 100.0))
                } else {
                    IsoMode::Value
                };
                apply_iso_mode(&mut v);
            }
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        });
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        percent.connect_value_changed(move |p| {
            if syncing.get() {
                return;
            }
            {
                let mut v = view.borrow_mut();
                if !matches!(v.iso_mode, IsoMode::Fraction(_)) {
                    return;
                }
                v.iso_mode = IsoMode::Fraction(p.value() / 100.0);
                apply_iso_mode(&mut v);
            }
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        });
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        lobes_dd.connect_selected_notify(move |d| {
            if syncing.get() {
                return;
            }
            {
                let mut v = view.borrow_mut();
                v.lobes = match d.selected() {
                    1 => Lobes::Negative,
                    2 => Lobes::Both,
                    _ => Lobes::Positive,
                };
                apply_iso_mode(&mut v);
            }
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        });
    }

    // ---- appearance ----
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        opacity.connect_value_changed(move |s| {
            if syncing.get() {
                return;
            }
            view.borrow_mut().opacity = s.value() as f32;
            redraw(&ui);
        });
    }
    for (btn, positive) in [(&col_pos, true), (&col_neg, false)] {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        btn.connect_rgba_notify(move |b| {
            if syncing.get() {
                return;
            }
            let c = b.rgba();
            let c = [c.red(), c.green(), c.blue()];
            let mut v = view.borrow_mut();
            if positive {
                v.color_pos = c;
            } else {
                v.color_neg = c;
            }
            drop(v);
            redraw(&ui);
        });
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        ao.connect_toggled(move |c| {
            if syncing.get() {
                return;
            }
            view.borrow_mut().ao = c.is_active();
            remesh(&view, &ui);
        });
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        ao_strength.connect_value_changed(move |s| {
            if syncing.get() {
                return;
            }
            view.borrow_mut().ao_strength = s.value() as f32;
            redraw(&ui);
        });
    }
    for (chk, is_atoms) in [(&atoms, true), (&cell, false)] {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        chk.connect_toggled(move |c| {
            if syncing.get() {
                return;
            }
            let ao_on = {
                let mut v = view.borrow_mut();
                if is_atoms {
                    v.show_atoms = c.is_active();
                } else {
                    v.show_cell = c.is_active();
                }
                v.scene_dirty = true;
                v.ao
            };
            // Atoms occlude the surfaces: their AO changes with them.
            if is_atoms && ao_on {
                remesh(&view, &ui);
            } else {
                redraw(&ui);
            }
        });
    }

    // ---- section plane ----
    {
        let update = {
            let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
            let (show, cut, flip, h, k, l, pos) = (
                sec_show.downgrade(),
                sec_cut.downgrade(),
                sec_flip.downgrade(),
                sh.downgrade(),
                sk.downgrade(),
                sl.downgrade(),
                sec_pos.downgrade(),
            );
            Rc::new(move || {
                if syncing.get() {
                    return;
                }
                let (Some(show), Some(cut), Some(flip), Some(h), Some(k), Some(l), Some(pos)) = (
                    show.upgrade(),
                    cut.upgrade(),
                    flip.upgrade(),
                    h.upgrade(),
                    k.upgrade(),
                    l.upgrade(),
                    pos.upgrade(),
                ) else {
                    return;
                };
                view.borrow_mut().section = SectionState {
                    show: show.is_active(),
                    cut: cut.is_active(),
                    flip: flip.is_active(),
                    hkl: [h.value_as_int(), k.value_as_int(), l.value_as_int()],
                    pos: pos.value(),
                };
                redraw(&ui);
            })
        };
        for c in [&sec_show, &sec_cut, &sec_flip] {
            let u = update.clone();
            c.connect_toggled(move |_| u());
        }
        for s in [&sh, &sk, &sl] {
            let u = update.clone();
            s.connect_value_changed(move |_| u());
        }
        let u = update.clone();
        sec_pos.connect_value_changed(move |_| u());
    }

    // ---- presets ----
    {
        let (view, ui, syncing, state) = (view.clone(), ui.clone(), syncing.clone(), state.clone());
        let (lobes_w, mode_w, pct_w, op_w, cp_w, cn_w) = (
            lobes_dd.downgrade(),
            mode_dd.downgrade(),
            percent.downgrade(),
            opacity.downgrade(),
            col_pos.downgrade(),
            col_neg.downgrade(),
        );
        preset.connect_selected_notify(move |d| {
            // (lobes index, enclosed %, opacity, + colour, − colour)
            let p = match d.selected() {
                1 => (0u32, 90.0, 0.55f32, [0.97f32, 0.80, 0.18], [0.20f32, 0.72, 0.90]),
                2 => (2, 50.0, 0.8, [0.97, 0.80, 0.18], [0.20, 0.72, 0.90]),
                3 => {
                    // Spin density needs the magnetisation channel.
                    let mut st = state.borrow_mut();
                    if st.chgcar_a.as_ref().is_some_and(|c| c.spin_polarized) {
                        st.channel = crate::physics::analysis::charge_density::DensityChannel::Magnetization;
                        st.recompute();
                    } else {
                        console::log_warn("Spin density preset: this CHGCAR is not spin-polarised");
                    }
                    (2, 50.0, 0.8, [0.86, 0.22, 0.20], [0.20, 0.40, 0.86])
                }
                _ => return,
            };
            syncing.set(true);
            if let Some(w) = lobes_w.upgrade() {
                w.set_selected(p.0);
            }
            if let Some(w) = pct_w.upgrade() {
                w.set_value(p.1);
                w.set_sensitive(true);
            }
            if let Some(w) = mode_w.upgrade() {
                w.set_selected(1);
            }
            if let Some(w) = op_w.upgrade() {
                w.set_value(p.2 as f64);
            }
            let rgba = |c: [f32; 3]| gtk4::gdk::RGBA::new(c[0], c[1], c[2], 1.0);
            if let Some(w) = cp_w.upgrade() {
                w.set_rgba(&rgba(p.3));
            }
            if let Some(w) = cn_w.upgrade() {
                w.set_rgba(&rgba(p.4));
            }
            syncing.set(false);
            {
                let mut v = view.borrow_mut();
                v.lobes = [Lobes::Positive, Lobes::Negative, Lobes::Both][p.0 as usize];
                v.iso_mode = IsoMode::Fraction(p.1 / 100.0);
                v.opacity = p.2;
                v.color_pos = p.3;
                v.color_neg = p.4;
                apply_iso_mode(&mut v);
            }
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        });
    }

    // ---- views ----
    for (btn, axis) in [(&along_a, 0usize), (&along_b, 1), (&along_c, 2)] {
        let (view, ui) = (view.clone(), ui.clone());
        btn.connect_clicked(move |_| {
            let mut v = view.borrow_mut();
            let Some(f) = v.field.as_ref() else { return };
            let l = f.lattice;
            let vec = |i: usize| Vector3::new(l[i][0], l[i][1], l[i][2]);
            let up = if axis == 2 { vec(1) } else { vec(2) };
            v.camera.rotation = look_along(vec(axis), up);
            v.needs_fit = true;
            drop(v);
            redraw(&ui);
        });
    }
    {
        let (view, ui) = (view.clone(), ui.clone());
        fit.connect_clicked(move |_| {
            view.borrow_mut().needs_fit = true;
            redraw(&ui);
        });
    }

    // ---- report and export ----
    {
        let view = view.clone();
        report.connect_clicked(move |_| {
            let v = view.borrow();
            let Some(f) = v.field.as_ref() else { return };
            console::info_report(&enclosed_report(f, &v));
            console::log_info("Charge density: enclosed charge written to Structure Info");
        });
    }
    {
        let (view, ui) = (view.clone(), ui.clone());
        figure.connect_clicked(move |b| {
            if let Some(win) = b.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                figure_dialog::show(&win, view.clone(), ui.clone());
            }
        });
    }
    {
        let view = view.clone();
        mesh_btn.connect_clicked(move |b| scene_file::export_mesh_dialog(b, view.clone()));
    }
    {
        let view = view.clone();
        save_scene.connect_clicked(move |b| scene_file::save_dialog(b, view.clone()));
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        let widgets = scene_file::SceneWidgets {
            lobes: lobes_dd.downgrade(),
            mode: mode_dd.downgrade(),
            percent: percent.downgrade(),
            opacity: opacity.downgrade(),
            col_pos: col_pos.downgrade(),
            col_neg: col_neg.downgrade(),
            ao: ao.downgrade(),
            ao_strength: ao_strength.downgrade(),
            atoms: atoms.downgrade(),
            cell: cell.downgrade(),
            sec_show: sec_show.downgrade(),
            sec_cut: sec_cut.downgrade(),
            sec_flip: sec_flip.downgrade(),
            h: sh.downgrade(),
            k: sk.downgrade(),
            l: sl.downgrade(),
            sec_pos: sec_pos.downgrade(),
        };
        load_scene.connect_clicked(move |b| {
            let (view, ui, syncing, widgets) = (view.clone(), ui.clone(), syncing.clone(), widgets.clone());
            scene_file::load_dialog(b, move |scene| {
                scene_file::apply(&scene, &view, &widgets, &syncing);
                sync_iso_controls(&view, &ui, &syncing);
                remesh(&view, &ui);
            });
        });
    }

    // ---- mouse ----
    {
        let drag = gtk4::GestureDrag::new();
        let prev = Rc::new(Cell::new((0.0, 0.0)));
        let p0 = prev.clone();
        drag.connect_drag_begin(move |_, _, _| p0.set((0.0, 0.0)));
        let (v2, ui2) = (view.clone(), ui.clone());
        drag.connect_drag_update(move |_, x, y| {
            let (px, py) = prev.get();
            prev.set((x, y));
            v2.borrow_mut().camera.rotate_screen_deg((x - px) * DRAG_DEG_PER_PX, (y - py) * DRAG_DEG_PER_PX);
            redraw(&ui2);
        });
        area.add_controller(drag);

        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let (v3, ui3) = (view.clone(), ui.clone());
        scroll.connect_scroll(move |_, _, dy| {
            v3.borrow_mut().camera.zoom_by(1.1f64.powf(-dy));
            redraw(&ui3);
            glib::Propagation::Stop
        });
        area.add_controller(scroll);
    }

    let handle = Handle { view: view.clone(), ui: ui.clone() };
    Parts { page, bottom, controls, handle }
}

impl View {
    /// Atoms or cell changed: re-upload them before the next draw.
    pub(super) fn mark_scene_dirty(&mut self) {
        self.scene_dirty = true;
    }
}

/// Bring the GPU up to date with the view (scene, volume, meshes, materials,
/// section). Called before every draw, interactive or exported.
pub(super) fn prepare_backend(v: &mut View) {
    let Some(mut backend) = v.backend.take() else { return };
    prepare_with(&mut backend, v);
    v.backend = Some(backend);
}

fn prepare_with(backend: &mut GlBackend, v: &mut View) {
    let Some(f) = v.field.as_ref() else { return };
    if v.scene_dirty {
        upload_scene(backend, f, v.show_atoms, v.show_cell, v.scheme);
        v.scene_dirty = false;
    }
    if v.volume_dirty {
        backend.set_volume(Some(Volume { data: &f.data, dims: f.dims, lattice: f.lattice }));
        v.volume_dirty = false;
    }
    if v.meshes_dirty {
        for (slot, lobe) in v.lobes_now.iter().enumerate() {
            backend.set_mesh(
                slot,
                lobe.as_ref().map(|l| MeshData {
                    positions: &l.mesh.positions,
                    normals: &l.mesh.normals,
                    indices: &l.mesh.indices,
                    ao: l.ao.as_deref(),
                }),
            );
        }
        v.meshes_dirty = false;
    }
    backend.set_mesh_material(0, Material { color: v.color_pos, opacity: v.opacity });
    backend.set_mesh_material(1, Material { color: v.color_neg, opacity: v.opacity });
    backend.set_ao_strength(if v.ao { v.ao_strength } else { 0.0 });
    apply_section(backend, v);
}

/// Structure Info text for the current surfaces.
fn enclosed_report(f: &Field, v: &View) -> String {
    let vol = crate::physics::analysis::volumetric::cell_volume(&f.lattice);
    let total: f64 = f.data.iter().map(|&x| x as f64).sum::<f64>() * vol / f.data.len() as f64;
    let mut out = format!("Isosurface report: {}\n", f.label);
    out += &format!("  Grid          : {} × {} × {}\n", f.dims[0], f.dims[1], f.dims[2]);
    out += &format!("  Cell volume   : {vol:.3} Å³\n");
    out += &format!("  Net charge    : {total:.4} e  (∫ρ dV over the cell)\n");
    out += &format!("  Isovalue      : {:.6} e/Å³\n", v.iso);
    for (slot, name) in [(0usize, "ρ > +iso"), (1, "ρ < −iso")] {
        if let Some(l) = &v.lobes_now[slot] {
            let e = l.enclosed;
            out += &format!(
                "  {name:<13} : {:.4} e in {:.3} Å³  ({:.2}% of the cell, {:.2}% of that sign's charge), {} triangles\n",
                e.electrons,
                e.volume,
                e.volume_fraction * 100.0,
                e.charge_fraction * 100.0,
                l.mesh.triangle_count()
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(min: f32, max: f32) -> Field {
        Field {
            data: Arc::new(vec![0.0]),
            dims: [1, 1, 1],
            lattice: [[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]],
            label: String::new(),
            min,
            max,
            default_iso: 0.1,
            atoms: vec![],
            histogram: vec![],
            hash: 0,
        }
    }

    #[test]
    fn look_along_maps_the_direction_into_the_screen() {
        let lat = [[4.0, 0.0, 0.0], [1.0, 4.0, 0.0], [0.5, 0.5, 6.0]];
        for (axis, up) in [(0usize, 2usize), (1, 2), (2, 1)] {
            let d = Vector3::new(lat[axis][0], lat[axis][1], lat[axis][2]);
            let u = Vector3::new(lat[up][0], lat[up][1], lat[up][2]);
            let q = look_along(d, u);
            let z = q * d.normalize();
            assert!((z - Vector3::z()).norm() < 1e-9, "axis {axis}: {z:?}");
            assert!((q * u).y < 0.0);
        }
    }

    #[test]
    fn slider_is_logarithmic_and_round_trips() {
        let f = field(-0.5, 2.0);
        for iso in [0.001, 0.01, 0.1, 1.0] {
            assert!((slider_to_iso(&f, iso_to_slider(&f, iso)) - iso).abs() / iso < 1e-9);
        }
        assert!(f.signed());
        assert!(!field(-0.001, 2.0).signed());
    }

    #[test]
    fn section_plane_by_miller_indices() {
        let f = field(0.0, 1.0);
        // (001) at the middle of a 4 Å cube: z = 2.
        let (n, d) = section_plane(&f, &SectionState { hkl: [0, 0, 1], pos: 0.5, ..Default::default() }).unwrap();
        assert!((n - Vector3::z()).norm() < 1e-12 && (d - 2.0).abs() < 1e-12);
        // (110) through the middle passes through the cell centre.
        let (n, d) = section_plane(&f, &SectionState { hkl: [1, 1, 0], pos: 0.5, ..Default::default() }).unwrap();
        assert!((n.dot(&Vector3::new(2.0, 2.0, 2.0)) - d).abs() < 1e-12);
        assert!(section_plane(&f, &SectionState { hkl: [0, 0, 0], ..Default::default() }).is_none());
    }

    #[test]
    fn figure_frame_fits_inside_the_canvas() {
        let (w, h) = frame_rect(1000.0, 500.0, 1.0);
        assert!((w - h).abs() < 1e-9 && h <= 500.0);
        let (w, h) = frame_rect(400.0, 800.0, 2.0);
        assert!((w / h - 2.0).abs() < 1e-9 && w <= 400.0);
    }
}
