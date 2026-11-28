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
//
// Orbital glyphs: with an OUTCAR of a DFT+U run (LDAUPRINT >= 1, read from
// beside the CHGCAR or loaded by hand), each +U shell's occupied orbitals or
// spin density are drawn as small lobed surfaces on its atom (mesh slots 2
// and 3), from `physics::analysis::orbitals`. They are rebuilt on a worker
// only when what they depend on changes (`GlyphKey`).

mod figure_dialog;
mod scene_file;

use super::charge_density_tab::ChargeDensityState;
use crate::io::ldau::{self, OnsiteMatrix};
use crate::model::elements::{get_atom_cov, get_element_color, ColorScheme};
use crate::physics::analysis::charge_density::{get_channel_data, DensityChannel};
use crate::physics::analysis::isosurface::{isosurface, Grid, Inside, Mesh, Region};
use crate::physics::analysis::orbitals::{build_glyph_set, GlyphKind, GlyphParams};
use crate::rendering::figure::{Anchor, Mark, MarkerLabel};
use crate::physics::analysis::volumetric::{
    ambient_occlusion, enclosed, iso_for_charge_fraction, log_histogram, Enclosed, Occluders, Sign,
};
use crate::rendering::gl::{loader, AtomInstance, BondInstance, Camera, GlBackend, GpuBackend, Material, MeshData, Section, Volume};
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
/// First of the two mesh slots holding orbital glyphs (0 and 1 are the surfaces).
const GLYPH_SLOT: usize = 2;
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
    /// The channel of the 2D page's state this field was built from.
    pub(super) channel: DensityChannel,
    pub(super) min: f32,
    pub(super) max: f32,
    default_iso: f64,
    pub(super) atoms: Vec<(String, [f64; 3])>,
    histogram: Vec<u32>,
    /// FNV-1a of the values: identifies the data a saved scene belongs to.
    pub(super) hash: u64,
}

impl Field {
    /// Unit of the values: μB/Å³ for the magnetization, e/Å³ otherwise.
    pub(super) fn unit(&self) -> &'static str {
        if self.channel == DensityChannel::Magnetization {
            "μB/Å³"
        } else {
            "e/Å³"
        }
    }
    /// Unit of an integral of the values: μB or e.
    pub(super) fn integral_unit(&self) -> &'static str {
        if self.channel == DensityChannel::Magnetization {
            "μB"
        } else {
            "e"
        }
    }
    /// Symbol for the values in keys and labels.
    pub(super) fn symbol(&self) -> &'static str {
        if self.channel == DensityChannel::Magnetization {
            "m"
        } else {
            "ρ"
        }
    }
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
        channel: st.channel,
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

/// Show only a box around a point (a defect, say): the atoms inside it and
/// the surfaces meshed over it. Its edges follow the lattice vectors, and it
/// may reach past the cell (periodic images fill it).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct CropState {
    pub(super) on: bool,
    /// Centre, fractional coordinates.
    pub(super) center: [f64; 3],
    /// Edge length along each lattice vector, Å.
    pub(super) size: f64,
}

impl Default for CropState {
    fn default() -> Self {
        Self { on: false, center: [0.5; 3], size: 8.0 }
    }
}

/// A labelled point: a vacancy or interstitial site (a wire cube), or an
/// atom to point out in a figure (a ring, a dot, or the label alone).
/// Fields after `label` are optional in scene files; without them a marker
/// is the labelled wire cube of older scenes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Marker {
    /// Fractional coordinates.
    pub(super) frac: [f64; 3],
    pub(super) label: String,
    #[serde(default)]
    pub(super) style: MarkerStyle,
    /// Ring or dot colour; the annotation ink when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) color: Option<[f32; 3]>,
    #[serde(default)]
    pub(super) anchor: Anchor,
    /// Shift of the label after anchoring, in points (right, down).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) label_offset: Option<[f64; 2]>,
}

impl Marker {
    pub(super) fn new(frac: [f64; 3], label: impl Into<String>) -> Self {
        Self { frac, label: label.into(), style: MarkerStyle::Cube, color: None, anchor: Anchor::Above, label_offset: None }
    }
}

/// How a marker is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum MarkerStyle {
    /// A wire cube (in the 3D scene), for empty sites.
    #[default]
    Cube,
    /// The label alone.
    None,
    /// A thin circle around the point: 1.25 × the atom's radius when an atom
    /// sits there, else a small fixed radius.
    Ring,
    /// A small filled dot.
    Dot,
}

/// Recolour or resize one atom: by `index` (from 1, POSCAR order) or by the
/// atom nearest `frac` (within 0.3 Å).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Highlight {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) frac: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) color: Option<[f32; 3]>,
    #[serde(default = "unit_scale")]
    pub(super) scale: f64,
}

/// Highlight every site whose |moment| (from the OUTCAR) exceeds `threshold`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct HighlightMagnetic {
    #[serde(default = "half")]
    pub(super) threshold: f64,
    #[serde(default = "magnetic_color")]
    pub(super) color: [f32; 3],
    #[serde(default = "unit_scale")]
    pub(super) scale: f64,
}

impl Default for HighlightMagnetic {
    fn default() -> Self {
        Self { threshold: half(), color: magnetic_color(), scale: 1.0 }
    }
}

fn unit_scale() -> f64 {
    1.0
}
fn half() -> f64 {
    0.5
}
fn magnetic_color() -> [f32; 3] {
    [0.93, 0.68, 0.10]
}

/// Drawn colour and radius factor of one atom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AtomStyle {
    pub(super) color: Option<[f32; 3]>,
    pub(super) scale: f64,
}

impl Default for AtomStyle {
    fn default() -> Self {
        Self { color: None, scale: 1.0 }
    }
}

/// Distance within which a point counts as sitting on an atom, Å.
const ON_ATOM: f64 = 0.3;

/// The atom (index) within `ON_ATOM` of the fractional point, if any.
pub(super) fn atom_at(f: &Field, frac: [f64; 3]) -> Option<usize> {
    let m = f.lattice_matrix();
    f.atoms
        .iter()
        .enumerate()
        .map(|(i, (_, a))| {
            let d = Vector3::new(frac[0] - a[0], frac[1] - a[1], frac[2] - a[2]).map(|x| x - x.round());
            (i, (m * d).norm())
        })
        .filter(|(_, d)| *d < ON_ATOM)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Per-atom styles from the explicit highlights and the magnetic switch
/// (explicit ones win).
pub(super) fn atom_styles(f: &Field, highlights: &[Highlight], magnetic: Option<&HighlightMagnetic>, moments: Option<&[f64]>) -> Vec<AtomStyle> {
    let mut out = vec![AtomStyle::default(); f.atoms.len()];
    if let (Some(h), Some(m)) = (magnetic, moments) {
        for (i, s) in out.iter_mut().enumerate() {
            if m.get(i).is_some_and(|x| x.abs() > h.threshold) {
                *s = AtomStyle { color: Some(h.color), scale: h.scale };
            }
        }
    }
    for h in highlights {
        let i = match (h.index, h.frac) {
            (Some(n), _) => n.checked_sub(1),
            (None, Some(fr)) => atom_at(f, fr),
            _ => None,
        };
        if let Some(s) = i.and_then(|i| out.get_mut(i)) {
            *s = AtomStyle { color: h.color.or(s.color), scale: h.scale };
        }
    }
    out
}

/// The styles for the view's current highlights.
pub(super) fn view_styles(f: &Field, v: &View) -> Vec<AtomStyle> {
    atom_styles(f, &v.highlights, v.highlight_magnetic.as_ref(), v.site_moments.as_deref().map(|m| m.as_slice()))
}

/// Atom spheres as the view draws them.
pub(super) fn view_spheres(f: &Field, v: &View) -> Vec<(String, Vector3<f64>, f64)> {
    atom_spheres(f, &v.crop, v.atom_scale, &view_styles(f, v))
}

/// Half the edge of a marker's cube, Å.
const MARKER_HALF: f64 = 0.4;
/// Ring radius where no atom sits, and dot radius, Å.
const RING_R: f64 = 0.35;
const DOT_R: f64 = 0.12;

/// Rings, dots and labels for the markers, in a 2D frame.
#[derive(Default)]
pub(super) struct MarkerAnnotations {
    pub(super) marks: Vec<Mark>,
    pub(super) labels: Vec<MarkerLabel>,
    /// Labels of cube markers anchored above with no offset, at their
    /// original figure position (text, x, y), so older scenes export
    /// unchanged.
    pub(super) legacy: Vec<(String, f64, f64)>,
}

/// Lay out the markers. `project` maps Cartesian Å to (x, y, depth) in the
/// frame (y down; larger depth is nearer the viewer), `visible` says
/// whether a frame point is drawn, `per_a` is frame units per Å and `font`
/// the annotation font size. `legacy`: keep the old placement for plain
/// cube labels (exported figures).
#[allow(clippy::too_many_arguments)]
pub(super) fn marker_annotations(
    f: &Field,
    v: &View,
    project: &dyn Fn(Vector3<f64>) -> (f64, f64, f64),
    visible: &dyn Fn(f64, f64) -> bool,
    per_a: f64,
    font: f64,
    ink: [f64; 3],
    legacy: bool,
) -> MarkerAnnotations {
    let mut out = MarkerAnnotations::default();
    if v.markers.is_empty() {
        return out;
    }
    let styles = view_styles(f, v);
    let atoms: Vec<(usize, (f64, f64, f64), f64)> = if v.show_atoms {
        atom_images(f, &v.crop)
            .into_iter()
            .zip(atom_spheres(f, &v.crop, v.atom_scale, &styles))
            .map(|((i, p), (_, _, r))| (i, project(p), r * per_a))
            .collect()
    } else {
        Vec::new()
    };
    for m in &v.markers {
        let site = atom_at(f, m.frac);
        let site_r = site.map(|i| get_atom_cov(&f.atoms[i].0) * ATOM_SCALE * v.atom_scale * styles.get(i).map_or(1.0, |s| s.scale));
        let color = m.color.map_or(ink, |c| c.map(|x| x as f64));
        for p in marker_positions(f, &v.crop, m) {
            let (x, y, z) = project(p);
            if !visible(x, y) {
                continue;
            }
            let atom_r = if v.show_atoms { site_r.map_or(0.0, |r| r * per_a) } else { 0.0 };
            let clear = match m.style {
                MarkerStyle::Cube => {
                    if legacy && m.anchor == Anchor::Above && m.label_offset.is_none() {
                        if !m.label.is_empty() {
                            out.legacy.push((m.label.clone(), x, y - MARKER_HALF * 1.5 * per_a - font * 0.6));
                        }
                        continue;
                    }
                    MARKER_HALF * 1.5 * per_a
                }
                MarkerStyle::Ring => {
                    let r = site_r.map_or(RING_R, |r| r * 1.25) * per_a;
                    out.marks.push(Mark { x, y, r, filled: false, color });
                    r
                }
                MarkerStyle::Dot => {
                    let r = (DOT_R * per_a).max(1.0);
                    out.marks.push(Mark { x, y, r, filled: true, color });
                    r.max(atom_r)
                }
                MarkerStyle::None => atom_r,
            };
            if m.label.is_empty() {
                continue;
            }
            // Other atoms: in front of the point, or behind it. The atom at
            // the point itself is covered by `clear`.
            let (mut near, mut far) = (Vec::new(), Vec::new());
            for (i, (ax, ay, az), r) in &atoms {
                if Some(*i) == site && (ax - x).hypot(ay - y) < 0.5 * per_a {
                    continue;
                }
                if *az > z { near.push((*ax, *ay, *r)) } else { far.push((*ax, *ay, *r)) }
            }
            out.labels.push(MarkerLabel {
                text: m.label.clone(),
                x,
                y,
                clear,
                anchor: m.anchor,
                offset: m.label_offset.unwrap_or([0.0; 2]),
                near,
                far,
            });
        }
    }
    out
}

/// Cartesian positions of `m` on screen: its images inside the crop box, or
/// the one in the cell.
fn marker_positions(f: &Field, crop: &CropState, m: &Marker) -> Vec<Vector3<f64>> {
    let lat = f.lattice_matrix();
    if !crop.on {
        return vec![lat * Vector3::from(m.frac.map(|x| x.rem_euclid(1.0)))];
    }
    let (lo, hi) = shown_box(f, crop);
    let range = |i: usize| ((lo[i] - m.frac[i]).ceil() as i64)..=((hi[i] - m.frac[i]).floor() as i64);
    let mut out = Vec::new();
    for sx in range(0) {
        for sy in range(1) {
            for sz in range(2) {
                out.push(lat * Vector3::new(m.frac[0] + sx as f64, m.frac[1] + sy as f64, m.frac[2] + sz as f64));
            }
        }
    }
    out
}

/// Parse "x y z [label]" (fractional coordinates) or "#N [label]" (atom N,
/// from 1, in POSCAR order), as a marker of `style`. A cube keeps its label
/// above; other styles place it beside the point.
pub(super) fn parse_marker(text: &str, atoms: &[(String, [f64; 3])], style: MarkerStyle) -> Option<Marker> {
    let mut parts = text.split_whitespace().peekable();
    let frac = if let Some(n) = parts.peek().and_then(|t| t.strip_prefix('#')) {
        let n: usize = n.parse().ok()?;
        let fr = atoms.get(n.checked_sub(1)?)?.1;
        parts.next();
        fr
    } else {
        let mut frac = [0.0; 3];
        for x in &mut frac {
            *x = parts.next()?.parse().ok()?;
        }
        frac
    };
    let label = parts.collect::<Vec<_>>().join(" ");
    let mut m = Marker::new(frac, label);
    m.style = style;
    if style != MarkerStyle::Cube {
        m.anchor = Anchor::Auto;
    }
    Some(m)
}

/// What the orbital glyphs show.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum GlyphMode {
    Off,
    /// Occupied eigenvectors of the occupation matrices, coloured by phase.
    Orbitals,
    /// Each shell's ρ↑ − ρ↓, coloured by sign.
    SpinDensity,
}

/// Orbital glyph settings (saved in scenes).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct GlyphState {
    pub(super) mode: GlyphMode,
    /// Occupation (orbitals) or |shell moment| in μB (spin density).
    pub(super) threshold: f64,
    /// Isovalue as a fraction of the peak: lower gives fatter lobes.
    pub(super) fraction: f64,
    /// Reach of the largest lobe from its atom, Å.
    pub(super) size: f64,
    pub(super) color_pos: [f32; 3],
    pub(super) color_neg: [f32; 3],
}

impl Default for GlyphState {
    fn default() -> Self {
        Self {
            mode: GlyphMode::Off,
            threshold: 0.5,
            fraction: 0.3,
            size: 1.4,
            color_pos: [0.96, 0.62, 0.12],
            color_neg: [0.16, 0.58, 0.62],
        }
    }
}

/// Occupation matrices read from an OUTCAR.
pub(super) struct Orbitals {
    pub(super) blocks: Arc<Vec<OnsiteMatrix>>,
    pub(super) source: std::path::PathBuf,
}

/// Everything the glyph meshes depend on (colours are only materials).
#[derive(Clone, Copy, PartialEq)]
struct GlyphKey {
    mode: GlyphMode,
    threshold: f64,
    fraction: f64,
    size: f64,
    crop: CropState,
    blocks: usize,
    lattice: [[f64; 3]; 3],
    atoms: usize,
}

/// The fractional box on screen: the crop box, or one cell.
fn shown_box(f: &Field, c: &CropState) -> ([f64; 3], [f64; 3]) {
    if !c.on {
        return ([0.0; 3], [1.0; 3]);
    }
    let mut lo = [0.0; 3];
    let mut hi = [1.0; 3];
    for i in 0..3 {
        let len = Vector3::new(f.lattice[i][0], f.lattice[i][1], f.lattice[i][2]).norm().max(1e-9);
        let h = 0.5 * c.size.max(0.1) / len;
        lo[i] = c.center[i] - h;
        hi[i] = c.center[i] + h;
    }
    (lo, hi)
}

/// Grid cubes covering the shown box.
fn shown_region(f: &Field, c: &CropState) -> Region {
    if !c.on {
        return Region::cell(f.dims);
    }
    let (lo, hi) = shown_box(f, c);
    let n = f.dims.map(|d| d as f64);
    Region {
        lo: [0, 1, 2].map(|i| (lo[i] * n[i]).floor() as i64),
        hi: [0, 1, 2].map(|i| (hi[i] * n[i]).ceil() as i64),
    }
}

/// Corners of the shown box, Cartesian.
fn shown_corners(f: &Field, c: &CropState) -> Vec<Vector3<f64>> {
    let (lo, hi) = shown_box(f, c);
    let m = f.lattice_matrix();
    (0..8)
        .map(|k: u32| {
            let pick = |i: usize| if k & (1 << i) == 0 { lo[i] } else { hi[i] };
            m * Vector3::new(pick(0), pick(1), pick(2))
        })
        .collect()
}

/// Replace the field by its negative (see `View::flip`).
pub(super) fn negate_field(f: &mut Field) {
    let data = Arc::make_mut(&mut f.data);
    for v in data.iter_mut() {
        *v = -*v;
    }
    (f.min, f.max) = (-f.max, -f.min);
    f.hash = fnv1a(&f.data);
    f.label = match f.label.strip_suffix(SIGN_REVERSED) {
        Some(l) => l.to_string(),
        None => format!("{}{SIGN_REVERSED}", f.label),
    };
}

const SIGN_REVERSED: &str = ", sign reversed";

/// What the camera frames: the cell corners and atoms, or with a crop box,
/// the atoms, surfaces and markers actually drawn in it (its corners would
/// leave the empty parts of the box in the picture). (point, radius) pairs.
pub(super) fn content_points(v: &View) -> Vec<(Vector3<f64>, f64)> {
    let Some(f) = v.field.as_ref() else { return Vec::new() };
    let mut pts: Vec<(Vector3<f64>, f64)> = Vec::new();
    if v.show_atoms {
        pts.extend(view_spheres(f, v).into_iter().map(|(_, c, r)| (c, r)));
    }
    if !v.crop.on {
        pts.extend(shown_corners(f, &v.crop).into_iter().map(|c| (c, 0.0)));
        return pts;
    }
    let surfaces = v.lobes_now.iter().flatten().filter(|_| v.show_surfaces).map(|l| &l.mesh);
    let glyphs = v.glyph_meshes.iter().flatten().filter(|_| v.glyphs.mode != GlyphMode::Off).map(|m| m.as_ref());
    for mesh in surfaces.chain(glyphs) {
        let step = (mesh.positions.len() / 20_000).max(1);
        pts.extend(mesh.positions.iter().step_by(step).map(|p| (Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64), 0.0)));
    }
    for m in &v.markers {
        pts.extend(marker_positions(f, &v.crop, m).into_iter().map(|c| (c, MARKER_HALF * 1.8)));
    }
    if pts.is_empty() {
        pts.extend(shown_corners(f, &v.crop).into_iter().map(|c| (c, 0.0)));
    }
    pts
}

/// Bonds between the atoms on screen, by the default bond rules (VESTA's
/// list, else covalent radii), coloured by halves like the main view.
fn bond_instances(f: &Field, v: &View) -> Vec<BondInstance> {
    use crate::model::bonds::{default_rules, BondTable};
    let styles = view_styles(f, v);
    let spheres = atom_spheres(f, &v.crop, v.atom_scale, &styles);
    let elements: Vec<String> = spheres.iter().map(|s| s.0.clone()).collect();
    let table = BondTable::from_rules(&default_rules(&elements, 1.15));
    let colors: Vec<[f32; 3]> = atom_images(f, &v.crop).into_iter().map(|(i, _)| atom_color(f, v, &styles, i)).collect();
    let radius = (BOND_RADIUS * v.atom_scale.min(1.0)) as f32;
    let mut out = Vec::new();
    for i in 0..spheres.len() {
        for j in i + 1..spheres.len() {
            let (a, b) = (&spheres[i], &spheres[j]);
            let d = (a.1 - b.1).norm();
            if table.bonded(&a.0, &b.0, d) {
                out.push(BondInstance {
                    a: [a.1.x as f32, a.1.y as f32, a.1.z as f32],
                    b: [b.1.x as f32, b.1.y as f32, b.1.z as f32],
                    radius,
                    color_a: colors[i],
                    color_b: colors[j],
                });
            }
        }
    }
    out
}

/// Bond radius at atom size 1, Å.
const BOND_RADIUS: f64 = 0.09;

/// Camera centre and radius for the shown box.
fn shown_frame(f: &Field, c: &CropState) -> (Vector3<f64>, f64) {
    let corners = shown_corners(f, c);
    let center = corners.iter().sum::<Vector3<f64>>() / 8.0;
    let radius = corners.iter().map(|p| (p - center).norm()).fold(1.0, f64::max);
    (center, radius)
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
    pub(super) crop: CropState,
    pub(super) markers: Vec<Marker>,
    pub(super) highlights: Vec<Highlight>,
    pub(super) highlight_magnetic: Option<HighlightMagnetic>,
    /// |moment| per atom from the OUTCAR (μB), for `highlight_magnetic`.
    pub(super) site_moments: Option<Arc<Vec<f64>>>,
    pub(super) show_bonds: bool,
    /// Show −field: a spin density with all spins reversed (a symmetry of a
    /// collinear calculation), so pairs that converged spin-down compare
    /// directly with spin-up ones.
    pub(super) flip: bool,
    /// Multiplies the atom radii.
    pub(super) atom_scale: f64,
    pub(super) scheme: ColorScheme,
    pub(super) camera: Camera,
    needs_fit: bool,
    /// Refit once the surfaces being computed arrive (a new crop box is
    /// framed by what it contains).
    fit_after_mesh: bool,
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
    pub(super) show_surfaces: bool,
    pub(super) orbitals: Option<Orbitals>,
    pub(super) glyphs: GlyphState,
    /// Glyph meshes (positive, negative), in Å.
    pub(super) glyph_meshes: [Option<Arc<Mesh>>; 2],
    /// e.g. "Ce f", for keys.
    pub(super) glyph_label: Option<String>,
    glyph_key: Option<GlyphKey>,
    glyphs_dirty: bool,
    glyph_job: Option<task::JobHandle>,
    outcar_job: Option<task::JobHandle>,
    glyph_busy: bool,
    glyph_report: String,
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
    iso_unit: glib::WeakRef<gtk4::Label>,
    orb_status: glib::WeakRef<gtk4::Label>,
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
/// Atom images on screen as (atom index, Cartesian Å): every image inside
/// the crop box, or each atom in the cell plus its copies on the cell faces.
pub(super) fn atom_images(f: &Field, crop: &CropState) -> Vec<(usize, Vector3<f64>)> {
    let m = f.lattice_matrix();
    let mut out = Vec::new();
    if crop.on {
        let (lo, hi) = shown_box(f, crop);
        const TOL: f64 = 1e-3;
        for (i, (_, fr)) in f.atoms.iter().enumerate() {
            let range = |k: usize| ((lo[k] - fr[k] - TOL).ceil() as i64)..=((hi[k] - fr[k] + TOL).floor() as i64);
            for sx in range(0) {
                for sy in range(1) {
                    for sz in range(2) {
                        out.push((i, m * Vector3::new(fr[0] + sx as f64, fr[1] + sy as f64, fr[2] + sz as f64)));
                    }
                }
            }
        }
        return out;
    }
    for (i, (_, fr)) in f.atoms.iter().enumerate() {
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
        for sx in shifts(base.x) {
            for sy in shifts(base.y) {
                for sz in shifts(base.z) {
                    out.push((i, m * (base + Vector3::new(sx, sy, sz))));
                }
            }
        }
    }
    out
}

/// Atom images as (element, position, radius); `styles` (per atom, may be
/// empty) scale the radii.
pub(super) fn atom_spheres(f: &Field, crop: &CropState, scale: f64, styles: &[AtomStyle]) -> Vec<(String, Vector3<f64>, f64)> {
    atom_images(f, crop)
        .into_iter()
        .map(|(i, p)| {
            let el = &f.atoms[i].0;
            let s = styles.get(i).map_or(1.0, |s| s.scale);
            (el.clone(), p, get_atom_cov(el) * ATOM_SCALE * scale * s)
        })
        .collect()
}

/// Colour of atom `i` as drawn.
fn atom_color(f: &Field, v: &View, styles: &[AtomStyle], i: usize) -> [f32; 3] {
    if let Some(c) = styles.get(i).and_then(|s| s.color) {
        return c;
    }
    let c = get_element_color(&f.atoms[i].0, v.scheme);
    [c.0 as f32, c.1 as f32, c.2 as f32]
}

/// Show the field or its negative. The caller remeshes.
pub(super) fn set_flip(view: &Rc<RefCell<View>>, on: bool) {
    let mut v = view.borrow_mut();
    if v.flip == on {
        return;
    }
    v.flip = on;
    if let Some(f) = v.field.as_mut() {
        negate_field(f);
    }
    // The lobes swap sign.
    v.lobes = match v.lobes {
        Lobes::Positive => Lobes::Negative,
        Lobes::Negative => Lobes::Positive,
        Lobes::Both => Lobes::Both,
    };
    v.volume_dirty = true;
    v.lobes_now = [None, None];
    v.meshes_dirty = true;
    apply_iso_mode(&mut v);
}

/// Show `crop` (or the whole cell): recentre the camera on it and refit.
/// The caller remeshes.
pub(super) fn set_crop(view: &Rc<RefCell<View>>, crop: CropState) {
    let mut v = view.borrow_mut();
    v.crop = crop;
    if let Some(f) = v.field.as_ref() {
        let (center, radius) = shown_frame(f, &crop);
        let (rotation, zoom) = (v.camera.rotation, v.camera.zoom);
        v.camera = Camera::new(center, radius, rotation);
        v.camera.zoom = zoom;
    }
    v.needs_fit = true;
    v.fit_after_mesh = crop.on;
    v.scene_dirty = true;
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
    let region = shown_region(f, &v.crop);
    let spheres: Vec<([f64; 3], f64)> = if v.show_atoms {
        view_spheres(f, &v).into_iter().map(|(_, c, r)| ([c.x, c.y, c.z], r)).collect()
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
                if v.fit_after_mesh {
                    v.fit_after_mesh = false;
                    v.needs_fit = true;
                }
            }
            redraw(&ui2);
        },
    ));
    drop(v);
    redraw(ui);
    update_glyphs(view, ui);
}

/// Rebuild the orbital glyphs if anything they depend on changed.
pub(super) fn update_glyphs(view: &Rc<RefCell<View>>, ui: &Ui) {
    let mut v = view.borrow_mut();
    let key = match (v.field.as_ref(), v.orbitals.as_ref()) {
        (Some(f), Some(o)) if v.glyphs.mode != GlyphMode::Off => Some(GlyphKey {
            mode: v.glyphs.mode,
            threshold: v.glyphs.threshold,
            fraction: v.glyphs.fraction,
            size: v.glyphs.size,
            crop: v.crop,
            blocks: Arc::as_ptr(&o.blocks) as usize,
            lattice: f.lattice,
            atoms: f.atoms.len(),
        }),
        _ => None,
    };
    if key == v.glyph_key {
        return;
    }
    v.glyph_key = key;
    let (Some(_), Some(f), Some(o)) = (key, v.field.as_ref(), v.orbitals.as_ref()) else {
        v.glyph_meshes = [None, None];
        v.glyphs_dirty = true;
        v.glyph_job = None;
        v.glyph_busy = false;
        drop(v);
        redraw(ui);
        return;
    };
    let blocks = o.blocks.clone();
    if blocks.iter().any(|b| b.atom >= f.atoms.len()) {
        console::log_warn("Orbitals: the OUTCAR has more atoms than the CHGCAR; is it from the same run?");
    }
    let names: Vec<String> = f.atoms.iter().map(|a| a.0.clone()).collect();
    let placements = atom_images(f, &v.crop);
    let g = v.glyphs;
    let params = GlyphParams {
        kind: if g.mode == GlyphMode::SpinDensity { GlyphKind::SpinDensity } else { GlyphKind::Orbitals },
        threshold: g.threshold,
        fraction: g.fraction,
        size: g.size,
    };
    v.glyph_busy = true;
    let weak = Rc::downgrade(view);
    let ui2 = ui.clone();
    v.glyph_job = Some(task::spawn(
        move |cancel| build_glyph_set(&blocks, &names, &placements, &params, &cancel),
        move |set| {
            let Some(view) = weak.upgrade() else { return };
            {
                let mut v = view.borrow_mut();
                let nonempty = |m: Mesh| (!m.is_empty()).then(|| Arc::new(m));
                v.glyph_meshes = [nonempty(set.pos), nonempty(set.neg)];
                v.glyph_label = set.label;
                v.glyphs_dirty = true;
                v.glyph_busy = false;
                if v.fit_after_mesh && !v.busy {
                    v.needs_fit = true;
                }
                if v.glyph_report != set.report {
                    console::info_report(&set.report);
                    v.glyph_report = set.report;
                }
            }
            redraw(&ui2);
        },
    ));
}

/// Read the occupation matrices of `path` on a worker. `quiet`: an OUTCAR
/// without them (no DFT+U) is not worth a message.
pub(super) fn load_outcar(view: &Rc<RefCell<View>>, ui: &Ui, path: std::path::PathBuf, quiet: bool) {
    let weak = Rc::downgrade(view);
    let ui2 = ui.clone();
    let worker_path = path.clone();
    let n_atoms = view.borrow().field.as_ref().map_or(0, |f| f.atoms.len());
    let job = task::spawn(
        move |_cancel| {
            let text = std::fs::read_to_string(&worker_path).map_err(|e| e.to_string());
            // Site moments (the last "magnetization" table), for highlighting.
            let moments = text.as_ref().ok().filter(|_| n_atoms > 0).and_then(|t| {
                crate::io::magmom::parse_outcar(t, n_atoms, None).pop().map(|m| m.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).collect::<Vec<f64>>())
            });
            Some((text.and_then(|t| ldau::parse_onsite(&t)), moments))
        },
        move |(result, moments)| {
            let Some(view) = weak.upgrade() else { return };
            {
                // Moments from the OUTCAR's table, else the +U shells' own.
                let shells = result.as_ref().ok().filter(|b| !b.is_empty()).map(|blocks| {
                    let mut m = vec![0.0; n_atoms.max(blocks.iter().map(|b| b.atom + 1).max().unwrap_or(0))];
                    for b in blocks {
                        m[b.atom] = crate::physics::analysis::orbitals::shell_moment(b).abs();
                    }
                    m
                });
                let mut v = view.borrow_mut();
                v.site_moments = moments.or(shells).map(Arc::new);
                v.scene_dirty = true;
            }
            let name = path.display().to_string();
            let status = match result {
                Ok(blocks) if blocks.is_empty() => {
                    if !quiet {
                        console::log_warn(&format!("{name}: no DFT+U occupation matrices (set LDAUPRINT = 1 or 2)"));
                    }
                    view.borrow_mut().orbitals = None;
                    "No DFT+U occupation matrices in the OUTCAR".to_string()
                }
                Ok(blocks) => {
                    let l = blocks[0].l;
                    let msg = format!(
                        "{} DFT+U {}-shell{} from {}",
                        blocks.len(),
                        crate::physics::analysis::orbitals::shell_letter(l),
                        if blocks.len() == 1 { "" } else { "s" },
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    console::log_info(&format!("{name}: {msg}"));
                    view.borrow_mut().orbitals = Some(Orbitals { blocks: Arc::new(blocks), source: path.clone() });
                    msg
                }
                Err(e) => {
                    if !quiet {
                        console::log_error(&format!("{name}: {e}"));
                    }
                    view.borrow_mut().orbitals = None;
                    format!("Could not read the OUTCAR: {e}")
                }
            };
            if let Some(l) = ui2.orb_status.upgrade() {
                l.set_text(&status);
            }
            view.borrow_mut().outcar_job = None;
            update_glyphs(&view, &ui2);
        },
    );
    view.borrow_mut().outcar_job = Some(job);
}

/// Atoms and the edges of the cell (or of the crop box).
fn upload_scene(backend: &mut GlBackend, f: &Field, v: &View) {
    let mut atoms = Vec::new();
    if v.show_atoms {
        let styles = view_styles(f, v);
        let images = atom_images(f, &v.crop);
        for ((i, _), (_, c, r)) in images.into_iter().zip(atom_spheres(f, &v.crop, v.atom_scale, &styles)) {
            atoms.push(AtomInstance {
                pos: [c.x as f32, c.y as f32, c.z as f32],
                radius: r as f32,
                color: atom_color(f, v, &styles, i),
            });
        }
    }
    backend.set_atoms(&atoms);
    backend.set_bonds(&if v.show_atoms && v.show_bonds { bond_instances(f, v) } else { Vec::new() });
    let mut lines = Vec::new();
    if v.show_cell {
        let corners = shown_corners(f, &v.crop);
        let corner = |k: u32| {
            let c = corners[k as usize];
            [c.x as f32, c.y as f32, c.z as f32]
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
    // Cube markers: edges along the Cartesian axes. Rings, dots and labels
    // are drawn over the picture (`marker_annotations`).
    for m in v.markers.iter().filter(|m| m.style == MarkerStyle::Cube) {
        for p in marker_positions(f, &v.crop, m) {
            let corner = |k: u32| {
                let d = |i: u32| if k & (1 << i) == 0 { -MARKER_HALF } else { MARKER_HALF };
                [(p.x + d(0)) as f32, (p.y + d(1)) as f32, (p.z + d(2)) as f32]
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
    // Markers over the picture, as in exported figures.
    if !v.markers.is_empty() && w > 0.0 && h > 0.0 {
        let view = v.camera.view().cast::<f64>();
        let (hw, hh) = v.camera.half_extents(w, h);
        let project = |p: Vector3<f64>| {
            let q = view * nalgebra::Vector4::new(p.x, p.y, p.z, 1.0);
            ((q.x + hw) / (2.0 * hw) * w, (hh - q.y) / (2.0 * hh) * h, q.z)
        };
        let visible = |x: f64, y: f64| (0.0..=w).contains(&x) && (0.0..=h).contains(&y);
        let ink = [0.08, 0.08, 0.1];
        let a = marker_annotations(f, v, &project, &visible, w / (2.0 * hw), 12.0, ink, false);
        crate::rendering::figure::draw_marks_and_labels(cr, &a.marks, &a.labels, 12.0, 1.3, ink);
    }
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
    let _ = cr.show_text(&format!("isosurface {sign}{} {}{mode}", crate::rendering::figure::format_value(v.iso), f.unit()));
    cr.move_to(14.0, 58.0);
    let status = if v.busy { "computing…".to_string() } else { v.stats.clone() };
    let _ = cr.show_text(&format!("range {:.3} … {:.3} {}   {}", f.min, f.max, f.unit(), status));

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
            "{name}: {:.3} {} in {:.2} Å³ ({:.1}% of the charge)",
            e.electrons,
            f.integral_unit(),
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
    if v.field.is_none() {
        return;
    }
    let pts = content_points(v);
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
    apply_scene: Rc<dyn Fn(scene_file::Scene)>,
    /// A scene is waiting for its channel to load.
    scene_pending: Rc<Cell<bool>>,
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

    /// Load a scene file (`*.cview.json`) and apply it, switching the density
    /// channel if it records one. Returns the figure settings it holds.
    pub fn load_scene(&self, path: &std::path::Path) -> Result<Option<FigureSettings>, String> {
        let scene = scene_file::load(path)?;
        let figure = scene.figure.clone();
        (self.apply_scene)(scene);
        Ok(figure)
    }

    /// Density loaded, surfaces computed and on screen, nothing pending.
    pub fn ready(&self) -> bool {
        let v = self.view.borrow();
        v.field.is_some()
            && v.backend.is_some()
            && !v.busy
            && !v.glyph_busy
            && v.outcar_job.is_none()
            && !self.scene_pending.get()
            && v.lobes_now.iter().any(Option::is_some)
    }
}

/// Scripted figure export (`CVIEW_CD_FIGURE=out.pdf [CVIEW_CD_SCENE=scene.cview.json] cview CHGCAR`):
/// once the density is on screen apply the scene, wait for its surfaces,
/// export the figure (scene settings, or the defaults) and quit. A debugging
/// and batch aid, like `CVIEW_PLAYER_SNAPSHOT`.
pub fn run_figure_script(handle: Handle, out: std::path::PathBuf, scene: Option<std::path::PathBuf>) {
    let started = std::time::Instant::now();
    let mut scene = scene;
    let mut settings: Option<FigureSettings> = None;
    let quit = || {
        if let Some(app) = gtk4::gio::Application::default() {
            app.quit();
        }
    };
    glib::timeout_add_local(Duration::from_millis(200), move || {
        if started.elapsed() > Duration::from_secs(300) {
            console::log_error("Figure script: timed out waiting for the surfaces");
            quit();
            return glib::ControlFlow::Break;
        }
        if !handle.ready() {
            return glib::ControlFlow::Continue;
        }
        if let Some(p) = scene.take() {
            match handle.load_scene(&p) {
                Ok(f) => settings = f,
                Err(e) => {
                    console::log_error(&format!("Figure script: {}: {e}", p.display()));
                    quit();
                    return glib::ControlFlow::Break;
                }
            }
            return glib::ControlFlow::Continue;
        }
        match handle.export_figure(&out, &settings.clone().unwrap_or_default()) {
            Ok(msg) => console::log_info(&msg),
            Err(e) => console::log_error(&format!("Figure script: {e}")),
        }
        if let Some(r) = handle.report() {
            console::info_report(&r);
        }
        quit();
        glib::ControlFlow::Break
    });
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
        crop: CropState::default(),
        markers: Vec::new(),
        highlights: Vec::new(),
        highlight_magnetic: None,
        site_moments: None,
        show_bonds: true,
        flip: false,
        atom_scale: 1.0,
        scheme,
        camera: Camera::new(Vector3::zeros(), 1.0, UnitQuaternion::identity()),
        needs_fit: true,
        fit_after_mesh: false,
        backend: None,
        lobes_now: [None, None],
        meshes_dirty: false,
        scene_dirty: true,
        volume_dirty: true,
        job: None,
        stats: String::new(),
        busy: false,
        show_surfaces: true,
        orbitals: None,
        glyphs: GlyphState::default(),
        glyph_meshes: [None, None],
        glyph_label: None,
        glyph_key: None,
        glyphs_dirty: false,
        glyph_job: None,
        outcar_job: None,
        glyph_busy: false,
        glyph_report: String::new(),
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
    let iso_unit = gtk4::Label::new(Some("e/Å³"));
    iso_unit.set_width_chars(6);
    iso_unit.set_xalign(0.0);
    iso_row.append(&iso_unit);
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
    let bonds = gtk4::CheckButton::with_label("Bonds");
    bonds.set_active(true);
    bonds.set_tooltip_text(Some("Bonds between the atoms shown, by the default bond rules"));
    let surfaces = gtk4::CheckButton::with_label("Surfaces");
    surfaces.set_active(true);
    surfaces.set_tooltip_text(Some("The density isosurfaces (off to show only atoms and orbital glyphs)"));
    let toggles = gtk4::Box::new(Orientation::Horizontal, 12);
    toggles.append(&surfaces);
    toggles.append(&atoms);
    toggles.append(&bonds);
    toggles.append(&cell);
    b_disp.append(&toggles);
    let flip = gtk4::CheckButton::with_label("Reverse sign");
    flip.set_tooltip_text(Some(
        "Show −ρ: for a spin density, all spins reversed (a symmetry of a collinear calculation), \
         so a configuration that converged spin-down can be shown like a spin-up one",
    ));
    b_disp.append(&flip);
    let atom_size = gtk4::Scale::with_range(Orientation::Horizontal, 0.2, 2.0, 0.05);
    atom_size.set_value(1.0);
    atom_size.set_draw_value(false);
    atom_size.set_size_request(120, -1);
    atom_size.set_tooltip_text(Some("Atom radius, relative to the default"));
    b_disp.append(&row("Atom size", &atom_size));
    let crop_on = gtk4::CheckButton::with_label("Crop to a box");
    crop_on.set_tooltip_text(Some(
        "Show only the atoms and surfaces inside a box around a point (a defect, say); periodic images fill it",
    ));
    b_disp.append(&crop_on);
    let frac_spin = |v: f64| {
        let s = gtk4::SpinButton::with_range(-1.0, 2.0, 0.01);
        s.set_digits(4);
        s.set_value(v);
        s.set_width_chars(6);
        s
    };
    let (cx, cy, cz) = (frac_spin(0.5), frac_spin(0.5), frac_spin(0.5));
    let centre_row = gtk4::Box::new(Orientation::Horizontal, 4);
    let centre_label = gtk4::Label::new(Some("Centre (frac.)"));
    centre_label.set_halign(Align::Start);
    centre_label.set_hexpand(true);
    centre_row.append(&centre_label);
    for s in [&cx, &cy, &cz] {
        centre_row.append(s);
    }
    b_disp.append(&centre_row);
    let crop_size = gtk4::SpinButton::with_range(1.0, 60.0, 0.5);
    crop_size.set_digits(1);
    crop_size.set_value(CropState::default().size);
    b_disp.append(&row("Box edge (Å)", &crop_size));
    let atom_no = gtk4::SpinButton::with_range(1.0, 99999.0, 1.0);
    atom_no.set_tooltip_text(Some("Atom number, in file order from 1 (as in VASP's OUTCAR)"));
    let centre_on = gtk4::Button::with_label("Centre on atom");
    let atom_row = gtk4::Box::new(Orientation::Horizontal, 4);
    atom_row.append(&atom_no);
    atom_row.append(&centre_on);
    atom_row.set_halign(Align::End);
    b_disp.append(&atom_row);
    let marker_entry = gtk4::Entry::new();
    marker_entry.set_placeholder_text(Some("x y z label, or #N label (atom N)"));
    marker_entry.set_tooltip_text(Some(
        "Mark a point by fractional coordinates (\"0.25 0.25 0.25 V_O\") or an atom by its number in file order (\"#16 NN\"). \
         _ starts a subscript in the label.",
    ));
    marker_entry.set_hexpand(true);
    let marker_add = gtk4::Button::with_label("Mark");
    let marker_clear = gtk4::Button::with_label("Clear");
    marker_clear.set_tooltip_text(Some("Remove all markers"));
    let marker_style = gtk4::DropDown::from_strings(&["Cube", "Label only", "Ring", "Dot"]);
    marker_style.set_tooltip_text(Some(
        "Cube: a wire cube for an empty site, label above. Ring, Dot, Label only: for atoms, label placed beside the point clear of the atoms in front.",
    ));
    let marker_row = gtk4::Box::new(Orientation::Horizontal, 4);
    marker_row.append(&marker_entry);
    marker_row.append(&marker_style);
    marker_row.append(&marker_add);
    marker_row.append(&marker_clear);
    b_disp.append(&marker_row);
    let highlight_mag = gtk4::CheckButton::with_label("Highlight magnetic sites");
    highlight_mag.set_tooltip_text(Some("Colour the atoms whose moment in the OUTCAR exceeds 0.5 μB (e.g. Ce³⁺ polarons)"));
    b_disp.append(&highlight_mag);
    // Enabled only while cropping.
    let crop_deps: Vec<glib::WeakRef<gtk4::Widget>> =
        [centre_row.upcast_ref::<gtk4::Widget>(), crop_size.upcast_ref(), atom_row.upcast_ref()]
            .iter()
            .map(|w| {
                w.set_sensitive(false);
                w.downgrade()
            })
            .collect();
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

    let (g_orb, b_orb) = section("Orbitals (DFT+U)");
    let orb_status = gtk4::Label::new(Some("No OUTCAR loaded"));
    orb_status.add_css_class("dim-label");
    orb_status.set_wrap(true);
    orb_status.set_xalign(0.0);
    b_orb.append(&orb_status);
    let orb_mode = gtk4::DropDown::from_strings(&["Off", "Occupied orbitals", "Shell spin density"]);
    orb_mode.set_tooltip_text(Some(
        "Occupied orbitals: eigenvectors of the on-site occupation matrices, coloured by the sign of ψ. \
         Shell spin density: ρ↑ − ρ↓ of each +U shell, coloured by sign.",
    ));
    b_orb.append(&row("Show", &orb_mode));
    let orb_thr = gtk4::SpinButton::with_range(0.0, 2.0, 0.05);
    orb_thr.set_digits(2);
    orb_thr.set_value(GlyphState::default().threshold);
    orb_thr.set_tooltip_text(Some("Least occupation of an orbital, or least |moment| (μB) of a shell, to draw"));
    b_orb.append(&row("Threshold", &orb_thr));
    let orb_size = gtk4::SpinButton::with_range(0.3, 4.0, 0.1);
    orb_size.set_digits(2);
    orb_size.set_value(GlyphState::default().size);
    orb_size.set_tooltip_text(Some("How far the largest lobe reaches from its atom"));
    b_orb.append(&row("Size (Å)", &orb_size));
    let orb_level = gtk4::SpinButton::with_range(0.05, 0.9, 0.05);
    orb_level.set_digits(2);
    orb_level.set_value(GlyphState::default().fraction);
    orb_level.set_tooltip_text(Some("Isovalue as a fraction of the peak; lower gives fatter lobes"));
    b_orb.append(&row("Lobe level", &orb_level));
    let rgba = |c: [f32; 3]| gtk4::gdk::RGBA::new(c[0], c[1], c[2], 1.0);
    let orb_cp = gtk4::ColorButton::with_rgba(&rgba(GlyphState::default().color_pos));
    let orb_cn = gtk4::ColorButton::with_rgba(&rgba(GlyphState::default().color_neg));
    let orb_colors = gtk4::Box::new(Orientation::Horizontal, 4);
    orb_colors.append(&orb_cp);
    orb_colors.append(&orb_cn);
    b_orb.append(&row("Colours (+, −)", &orb_colors));
    let orb_load = gtk4::Button::with_label("Load OUTCAR…");
    orb_load.set_tooltip_text(Some("A VASP OUTCAR of a DFT+U run with LDAUPRINT = 1 or 2 (the one beside the CHGCAR is read automatically)"));
    b_orb.append(&orb_load);
    controls.append(&g_orb);

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
        iso_unit: iso_unit.downgrade(),
        orb_status: orb_status.downgrade(),
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
            if v.backend.is_none() || v.field.is_none() {
                return glib::Propagation::Proceed;
            }
            if v.needs_fit && w > 0 && h > 0 {
                let pts: Vec<Vector3<f64>> = content_points(v).into_iter().map(|(p, _)| p).collect();
                v.camera.fit(&pts, 0.5, w as f64 / h as f64);
                v.needs_fit = false;
            }
            let Some(backend) = v.backend.as_mut() else { return glib::Propagation::Proceed };
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
                    let (center, radius) = shown_frame(f, &v.crop);
                    // A tilted three-quarter view shows depth on first open.
                    let rotation = if first { UnitQuaternion::from_euler_angles(-0.35, 0.55, 0.0) } else { v.camera.rotation };
                    v.camera = Camera::new(center, radius, rotation);
                    v.needs_fit = true;
                }
                v.field = field;
                if v.flip {
                    if let Some(f) = v.field.as_mut() {
                        negate_field(f);
                    }
                }
                apply_iso_mode(&mut v);
            }
            if let Some(p) = ui.placeholder.upgrade() {
                p.set_visible(view.borrow().field.is_none());
            }
            if let (Some(l), Some(f)) = (ui.iso_unit.upgrade(), view.borrow().field.as_ref()) {
                l.set_text(f.unit());
            }
            // The OUTCAR beside a newly loaded CHGCAR, for orbital glyphs.
            let sibling = state.borrow().path_a.as_deref().and_then(ldau::sibling_outcar);
            if let Some(p) = sibling {
                let known = view.borrow().orbitals.as_ref().is_some_and(|o| o.source == p);
                if !known && view.borrow().outcar_job.is_none() {
                    load_outcar(&view, &ui, p, true);
                }
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

    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        bonds.connect_toggled(move |c| {
            if syncing.get() {
                return;
            }
            let mut v = view.borrow_mut();
            v.show_bonds = c.is_active();
            v.scene_dirty = true;
            drop(v);
            redraw(&ui);
        });
    }
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        flip.connect_toggled(move |c| {
            if syncing.get() {
                return;
            }
            set_flip(&view, c.is_active());
            sync_iso_controls(&view, &ui, &syncing);
            remesh(&view, &ui);
        });
    }

    // ---- atom size and crop box ----
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        atom_size.connect_value_changed(move |sc| {
            if syncing.get() {
                return;
            }
            {
                let mut v = view.borrow_mut();
                v.atom_scale = sc.value();
                v.scene_dirty = true;
            }
            // Atoms occlude the surfaces.
            if view.borrow().ao && view.borrow().show_atoms {
                remesh(&view, &ui);
            } else {
                redraw(&ui);
            }
        });
    }
    {
        let read = {
            let (on, x, y, z, size) = (crop_on.downgrade(), cx.downgrade(), cy.downgrade(), cz.downgrade(), crop_size.downgrade());
            move || -> Option<CropState> {
                Some(CropState {
                    on: on.upgrade()?.is_active(),
                    center: [x.upgrade()?.value(), y.upgrade()?.value(), z.upgrade()?.value()],
                    size: size.upgrade()?.value(),
                })
            }
        };
        let deps = crop_deps.clone();
        let update = {
            let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
            Rc::new(move || {
                if syncing.get() {
                    return;
                }
                let Some(c) = read() else { return };
                for w in deps.iter().filter_map(|w| w.upgrade()) {
                    w.set_sensitive(c.on);
                }
                set_crop(&view, c);
                remesh(&view, &ui);
            })
        };
        let u = update.clone();
        crop_on.connect_toggled(move |_| u());
        for s in [&cx, &cy, &cz, &crop_size] {
            let u = update.clone();
            s.connect_value_changed(move |_| u());
        }
        let (view, syncing) = (view.clone(), syncing.clone());
        let (an, x, y, z) = (atom_no.downgrade(), cx.downgrade(), cy.downgrade(), cz.downgrade());
        centre_on.connect_clicked(move |_| {
            let Some(n) = an.upgrade().map(|s| s.value() as usize) else { return };
            let fr = {
                let v = view.borrow();
                let Some(f) = v.field.as_ref() else { return };
                match f.atoms.get(n.wrapping_sub(1)) {
                    Some((_, fr)) => *fr,
                    None => {
                        console::log_warn(&format!("Centre on atom: there are {} atoms", f.atoms.len()));
                        return;
                    }
                }
            };
            syncing.set(true);
            for (w, c) in [(&x, fr[0]), (&y, fr[1]), (&z, fr[2])] {
                if let Some(w) = w.upgrade() {
                    w.set_value(c);
                }
            }
            syncing.set(false);
            update();
        });
    }

    // ---- surfaces toggle and orbital glyphs ----
    {
        let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
        surfaces.connect_toggled(move |c| {
            if syncing.get() {
                return;
            }
            let mut v = view.borrow_mut();
            v.show_surfaces = c.is_active();
            v.meshes_dirty = true;
            drop(v);
            redraw(&ui);
        });
    }
    {
        let read = {
            let (m, t, sz, lv, cp, cn) =
                (orb_mode.downgrade(), orb_thr.downgrade(), orb_size.downgrade(), orb_level.downgrade(), orb_cp.downgrade(), orb_cn.downgrade());
            move || -> Option<GlyphState> {
                let c = |b: &gtk4::ColorButton| {
                    let r = b.rgba();
                    [r.red(), r.green(), r.blue()]
                };
                Some(GlyphState {
                    mode: match m.upgrade()?.selected() {
                        1 => GlyphMode::Orbitals,
                        2 => GlyphMode::SpinDensity,
                        _ => GlyphMode::Off,
                    },
                    threshold: t.upgrade()?.value(),
                    fraction: lv.upgrade()?.value(),
                    size: sz.upgrade()?.value(),
                    color_pos: c(&cp.upgrade()?),
                    color_neg: c(&cn.upgrade()?),
                })
            }
        };
        let update = {
            let (view, ui, syncing) = (view.clone(), ui.clone(), syncing.clone());
            Rc::new(move || {
                if syncing.get() {
                    return;
                }
                let Some(g) = read() else { return };
                {
                    let mut v = view.borrow_mut();
                    v.glyphs = g;
                    v.glyphs_dirty = true;
                }
                update_glyphs(&view, &ui);
                redraw(&ui);
            })
        };
        let u = update.clone();
        orb_mode.connect_selected_notify(move |_| u());
        for s in [&orb_thr, &orb_size, &orb_level] {
            let u = update.clone();
            s.connect_value_changed(move |_| u());
        }
        for b in [&orb_cp, &orb_cn] {
            let u = update.clone();
            b.connect_color_set(move |_| u());
        }
        let (view, ui) = (view.clone(), ui.clone());
        orb_load.connect_clicked(move |b| {
            let parent = b.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
            let d = gtk4::FileChooserNative::new(Some("Open OUTCAR"), parent.as_ref(), gtk4::FileChooserAction::Open, Some("Open"), Some("Cancel"));
            let (view, ui) = (view.clone(), ui.clone());
            d.connect_response(move |d, r| {
                if r == gtk4::ResponseType::Accept {
                    if let Some(path) = d.file().and_then(|f| f.path()) {
                        load_outcar(&view, &ui, path, false);
                    }
                }
                d.destroy();
            });
            crate::utils::file_chooser::show(&d);
        });
    }

    // ---- markers ----
    {
        let (view2, ui2, entry, style_w) = (view.clone(), ui.clone(), marker_entry.downgrade(), marker_style.downgrade());
        let add = move || {
            let (view, ui) = (&view2, &ui2);
            let Some(e) = entry.upgrade() else { return };
            let style = match style_w.upgrade().map_or(0, |d| d.selected()) {
                1 => MarkerStyle::None,
                2 => MarkerStyle::Ring,
                3 => MarkerStyle::Dot,
                _ => MarkerStyle::Cube,
            };
            let atoms = view.borrow().field.as_ref().map(|f| f.atoms.clone()).unwrap_or_default();
            match parse_marker(&e.text(), &atoms, style) {
                Some(m) => {
                    let mut v = view.borrow_mut();
                    v.markers.push(m);
                    v.scene_dirty = true;
                    drop(v);
                    e.set_text("");
                    redraw(ui);
                }
                None => console::log_warn("Marker: type three fractional coordinates or #N (an atom number), then an optional label"),
            }
        };
        let add = Rc::new(add);
        let a = add.clone();
        marker_add.connect_clicked(move |_| a());
        marker_entry.connect_activate(move |_| add());
        let (view3, ui3, syncing3) = (view.clone(), ui.clone(), syncing.clone());
        highlight_mag.connect_toggled(move |c| {
            if syncing3.get() {
                return;
            }
            let mut v = view3.borrow_mut();
            v.highlight_magnetic = c.is_active().then(HighlightMagnetic::default);
            if c.is_active() && v.site_moments.is_none() {
                console::log_warn("Highlight magnetic sites: no moments yet (load the run's OUTCAR)");
            }
            v.scene_dirty = true;
            drop(v);
            redraw(&ui3);
        });
        let (view, ui) = (view.clone(), ui.clone());
        marker_clear.connect_clicked(move |_| {
            let mut v = view.borrow_mut();
            v.markers.clear();
            v.scene_dirty = true;
            drop(v);
            redraw(&ui);
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
    // Applying a scene: switch the channel first if it records another one.
    // The page picks the new channel up on its next poll, which resets the
    // camera and isovalue, so the scene itself is applied after that.
    let scene_pending = Rc::new(Cell::new(false));
    let apply_scene: Rc<dyn Fn(scene_file::Scene)> = {
        let (view, ui, syncing, state, pending) = (view.clone(), ui.clone(), syncing.clone(), state.clone(), scene_pending.clone());
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
            atom_size: atom_size.downgrade(),
            bonds: bonds.downgrade(),
            flip: flip.downgrade(),
            surfaces: surfaces.downgrade(),
            highlight_mag: highlight_mag.downgrade(),
            orb_mode: orb_mode.downgrade(),
            orb_thr: orb_thr.downgrade(),
            orb_size: orb_size.downgrade(),
            orb_level: orb_level.downgrade(),
            orb_cp: orb_cp.downgrade(),
            orb_cn: orb_cn.downgrade(),
            crop_on: crop_on.downgrade(),
            crop_center: [cx.downgrade(), cy.downgrade(), cz.downgrade()],
            crop_size: crop_size.downgrade(),
            crop_deps: crop_deps.clone(),
        };
        Rc::new(move |scene: scene_file::Scene| {
            let want = scene.channel;
            let switching = match want {
                Some(ch) if state.borrow().channel != ch => {
                    let mut st = state.borrow_mut();
                    if ch == DensityChannel::Total || st.chgcar_a.as_ref().is_some_and(|c| c.spin_polarized) {
                        st.channel = ch;
                        st.recompute();
                        true
                    } else {
                        console::log_warn(&format!(
                            "Scene shows the {} density, but this CHGCAR is not spin-polarised",
                            ch.label().to_lowercase()
                        ));
                        false
                    }
                }
                _ => false,
            };
            let finish = {
                let (view, ui, syncing, widgets, pending) = (view.clone(), ui.clone(), syncing.clone(), widgets.clone(), pending.clone());
                move || {
                    scene_file::apply(&scene, &view, &widgets, &syncing);
                    sync_iso_controls(&view, &ui, &syncing);
                    remesh(&view, &ui);
                    pending.set(false);
                }
            };
            pending.set(true);
            if !switching {
                finish();
                return;
            }
            let view = Rc::downgrade(&view);
            let pending = pending.clone();
            let mut tries = 0;
            glib::timeout_add_local(Duration::from_millis(50), move || {
                tries += 1;
                let Some(v) = view.upgrade() else { return glib::ControlFlow::Break };
                if v.borrow().field.as_ref().is_some_and(|f| Some(f.channel) == want) {
                    finish();
                    return glib::ControlFlow::Break;
                }
                if tries > 200 {
                    console::log_error("Scene not applied: the density channel did not switch");
                    pending.set(false);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        })
    };
    {
        let apply_scene = apply_scene.clone();
        load_scene.connect_clicked(move |b| {
            let apply_scene = apply_scene.clone();
            scene_file::load_dialog(b, move |scene| apply_scene(scene));
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

    let handle = Handle { view: view.clone(), ui: ui.clone(), apply_scene, scene_pending };
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
        upload_scene(backend, f, v);
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
                lobe.as_ref().filter(|_| v.show_surfaces).map(|l| MeshData {
                    positions: &l.mesh.positions,
                    normals: &l.mesh.normals,
                    indices: &l.mesh.indices,
                    ao: l.ao.as_deref(),
                }),
            );
        }
        v.meshes_dirty = false;
    }
    if v.glyphs_dirty {
        for (k, mesh) in v.glyph_meshes.iter().enumerate() {
            backend.set_mesh(
                GLYPH_SLOT + k,
                mesh.as_ref().filter(|_| v.glyphs.mode != GlyphMode::Off).map(|m| MeshData {
                    positions: &m.positions,
                    normals: &m.normals,
                    indices: &m.indices,
                    ao: None,
                }),
            );
        }
        v.glyphs_dirty = false;
    }
    backend.set_mesh_material(0, Material { color: v.color_pos, opacity: v.opacity });
    backend.set_mesh_material(1, Material { color: v.color_neg, opacity: v.opacity });
    backend.set_mesh_material(GLYPH_SLOT, Material { color: v.glyphs.color_pos, opacity: 1.0 });
    backend.set_mesh_material(GLYPH_SLOT + 1, Material { color: v.glyphs.color_neg, opacity: 1.0 });
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
    if f.channel == DensityChannel::Magnetization {
        out += &format!("  Net moment    : {total:.4} μB  (∫m dV over the cell)\n");
    } else {
        out += &format!("  Net charge    : {total:.4} e  (∫ρ dV over the cell)\n");
    }
    out += &format!("  Isovalue      : {:.6} {}\n", v.iso, f.unit());
    for (slot, name) in [(0usize, "ρ > +iso"), (1, "ρ < −iso")] {
        if let Some(l) = &v.lobes_now[slot] {
            let e = l.enclosed;
            out += &format!(
                "  {name:<13} : {:.4} {} in {:.3} Å³  ({:.2}% of the cell, {:.2}% of that sign's charge), {} triangles\n",
                e.electrons,
                f.integral_unit(),
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
            channel: DensityChannel::Total,
            min,
            max,
            default_iso: 0.1,
            atoms: vec![],
            histogram: vec![],
            hash: 0,
        }
    }

    #[test]
    fn markers_and_highlights_find_their_atom() {
        let mut f = field(0.0, 1.0);
        f.atoms = vec![("O".into(), [0.0, 0.0, 0.0]), ("Ce".into(), [0.5, 0.5, 0.5])];
        // Exactly on, 0.2 Å off, across the cell edge, and 0.5 Å off (4 Å cell).
        assert_eq!(atom_at(&f, [0.5, 0.5, 0.5]), Some(1));
        assert_eq!(atom_at(&f, [0.55, 0.5, 0.5]), Some(1));
        assert_eq!(atom_at(&f, [0.99, 0.0, 1.0]), Some(0));
        assert_eq!(atom_at(&f, [0.625, 0.5, 0.5]), None);
        // Magnetic switch, then an explicit highlight on top of it.
        let mag = HighlightMagnetic { threshold: 0.5, color: [1.0, 0.0, 0.0], scale: 1.0 };
        let hl = [Highlight { frac: Some([0.0, 0.0, 0.01]), index: None, color: None, scale: 1.5 }];
        let s = atom_styles(&f, &hl, Some(&mag), Some(&[0.6, 0.98]));
        assert_eq!(s[0], AtomStyle { color: Some([1.0, 0.0, 0.0]), scale: 1.5 });
        assert_eq!(s[1], AtomStyle { color: Some([1.0, 0.0, 0.0]), scale: 1.0 });
        let s = atom_styles(&f, &[Highlight { frac: None, index: Some(2), color: Some([0.0, 1.0, 0.0]), scale: 1.0 }], None, None);
        assert_eq!((s[0], s[1].color), (AtomStyle::default(), Some([0.0, 1.0, 0.0])));
        // Highlight scale reaches the drawn radius.
        let r = atom_spheres(&f, &CropState::default(), 1.0, &[AtomStyle::default(), AtomStyle { color: None, scale: 2.0 }]);
        let ce = r.iter().find(|a| a.0 == "Ce").unwrap().2;
        assert!((ce - 2.0 * get_atom_cov("Ce") * ATOM_SCALE).abs() < 1e-12);
    }

    #[test]
    fn crop_box_meshes_and_shows_only_its_part() {
        let mut f = field(0.0, 1.0);
        f.dims = [40, 40, 40];
        f.atoms = vec![("O".into(), [0.0, 0.0, 0.0]), ("Ce".into(), [0.5, 0.5, 0.5])];
        // Whole cell: the corner atom appears at all 8 corners.
        let all = atom_spheres(&f, &CropState::default(), 1.0, &[]);
        assert_eq!(all.iter().filter(|a| a.0 == "O").count(), 8);
        // A 2 Å box around the cell corner: one O (its images meet there), no Ce.
        let crop = CropState { on: true, center: [1.0, 1.0, 1.0], size: 2.0 };
        let some = atom_spheres(&f, &crop, 0.5, &[]);
        assert_eq!(some.len(), 1);
        assert!((some[0].1 - Vector3::new(4.0, 4.0, 4.0)).norm() < 1e-9);
        // ±1 Å = ±0.25 of a 4 Å cell = ±10 grid steps, past the cell edge.
        let r = shown_region(&f, &crop);
        assert_eq!((r.lo, r.hi), ([30; 3], [50; 3]));
        let (c, _) = shown_frame(&f, &crop);
        assert!((c - Vector3::new(4.0, 4.0, 4.0)).norm() < 1e-9);
        assert_eq!(shown_region(&f, &CropState::default()), Region::cell(f.dims));
    }

    #[test]
    fn markers_parse_and_repeat_inside_the_box() {
        let m = parse_marker("0.25 0.5 1  V_O", &[], MarkerStyle::Cube).unwrap();
        assert_eq!((m.frac, m.label.as_str()), ([0.25, 0.5, 1.0], "V_O"));
        assert_eq!((m.style, m.anchor), (MarkerStyle::Cube, Anchor::Above));
        assert!(parse_marker("0.25 x 1", &[], MarkerStyle::Cube).is_none());
        // By atom number; atoms get their label beside them.
        let atoms = vec![("O".to_string(), [0.0; 3]), ("Ce".to_string(), [0.5, 0.25, 0.125])];
        let on_atom = parse_marker("#2 Ce_NN", &atoms, MarkerStyle::Ring).unwrap();
        assert_eq!((on_atom.frac, on_atom.label.as_str(), on_atom.anchor), ([0.5, 0.25, 0.125], "Ce_NN", Anchor::Auto));
        assert!(parse_marker("#3 x", &atoms, MarkerStyle::Ring).is_none());
        assert!(parse_marker("#0", &atoms, MarkerStyle::Ring).is_none());
        let f = field(0.0, 1.0);
        // Out of the box: wrapped into the cell.
        assert_eq!(marker_positions(&f, &CropState::default(), &m), vec![Vector3::new(1.0, 2.0, 0.0)]);
        // A box wider than the cell along a holds two images.
        let crop = CropState { on: true, center: [0.25, 0.5, 1.0], size: 5.0 };
        assert_eq!(marker_positions(&f, &crop, &m).len(), 1);
        let crop = CropState { on: true, center: [0.75, 0.5, 1.0], size: 4.5 };
        assert_eq!(marker_positions(&f, &crop, &m).len(), 2);
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
