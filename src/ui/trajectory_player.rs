// src/ui/trajectory_player.rs
//
// Trajectory Player: plays every frame of a relaxation or MD file, or of a
// micromagnetic series (OVF snapshots), on the GPU (OpenGL via
// `rendering::gl`), in a window of its own. Frames that carry magnetic
// moments show them as arrows, styled as in the main view. The main view keeps
// showing one frame through Cairo; "Show in Main View" copies the current
// frame there, which is how measurement, analysis and vector export reach
// any frame.
//
// Layout:  [ GL canvas + text overlay ]
//          [ energy-per-frame strip   ]   (when the file has energies;
//                                          otherwise ⟨m⟩ per frame when it
//                                          has moments)
//          [ transport and options    ]

use crate::model::elements::get_element_color;
use crate::model::trajectory::Trajectory;
use crate::rendering::gl::{loader, ArrowInstance, AtomInstance, BondInstance, Camera, GlBackend, GpuBackend};
use crate::state::AppState;
use crate::utils::console;
use gtk4::prelude::*;
use gtk4::{glib, ApplicationWindow, Notebook};
use nalgebra::{Matrix3, Vector3};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Same drag feel as the main view (`interactions.rs`).
const DRAG_DEG_PER_PX: f64 = 0.4;
/// Boundary images, as in the main view: atoms within this fraction of a
/// cell face are repeated on the opposite face.
const IMAGE_TOL: f64 = 0.05;
/// Above this many drawn atoms, bonds are not searched per frame.
const MAX_ATOMS_FOR_BONDS: usize = 50_000;
/// Moments shorter than this draw no arrow (as `painter::MomentArrow`).
const MIN_ARROW: f64 = 0.01;

/// Per-atom appearance, taken from the tab when the window opens so the
/// player matches the main view.
struct Look {
    colors: Vec<[f32; 3]>,
    radii: Vec<f32>,
    /// Bond length range per species pair (row-major, species × species),
    /// from the tab's bond rules; None = never bonded.
    bond_range: Vec<Option<(f64, f64)>>,
    bond_radius: f32,
    bond_color: [f32; 3],
    background: [f32; 3],
    /// Moment arrows, when the frames carry moments and the tab shows them.
    moments: Option<MomentLook>,
}

/// How arrows (and, on a mesh, cells) are coloured.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ArrowColor {
    /// Hue from the in-plane angle, white at +z, black at −z.
    Direction,
    /// m_z on a blue (−) / white / red (+) scale.
    Mz,
    /// Up or down along the magnetic axis.
    UpDown,
    /// The tab's moment colour.
    Single,
}

impl ArrowColor {
    const LABELS: [&'static str; 4] = ["Direction", "m_z (blue–red)", "Up / down", "Single colour"];
    const ALL: [ArrowColor; 4] = [ArrowColor::Direction, ArrowColor::Mz, ArrowColor::UpDown, ArrowColor::Single];
}

/// A regular mesh of sites (an OVF magnetization): grid index per site.
struct Grid {
    idx: Vec<[i64; 3]>,
    /// Smallest cell edge, Å.
    spacing: f64,
    /// Sites along each axis.
    counts: [i64; 3],
    /// The axis with fewest sites (a film's normal).
    normal: usize,
}

impl Grid {
    /// The grid `positions` sit on, when every site is on one.
    fn of(positions: &[[f64; 3]]) -> Option<Grid> {
        if positions.len() < 2 {
            return None;
        }
        let mut lo = [0.0; 3];
        let mut step = [0.0; 3];
        let mut spacing = f64::MAX;
        for d in 0..3 {
            let mut v: Vec<f64> = positions.iter().map(|p| p[d]).collect();
            v.sort_by(f64::total_cmp);
            lo[d] = v[0];
            let tol = 1e-6 * (v[v.len() - 1] - v[0]).max(1.0);
            let gap = v.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > tol).fold(f64::MAX, f64::min);
            step[d] = if gap == f64::MAX { 0.0 } else { gap };
            if step[d] > 0.0 {
                spacing = spacing.min(step[d]);
            }
        }
        if spacing == f64::MAX {
            return None;
        }
        let mut counts = [1i64; 3];
        let idx: Vec<[i64; 3]> = positions
            .iter()
            .map(|p| {
                [0, 1, 2].map(|d| {
                    let i = if step[d] > 0.0 { ((p[d] - lo[d]) / step[d]).round() as i64 } else { 0 };
                    counts[d] = counts[d].max(i + 1);
                    i
                })
            })
            .collect();
        let normal = (0..3).min_by_key(|&d| counts[d]).unwrap_or(2);
        Some(Grid { idx, spacing, counts, normal })
    }

    /// Arrow pitch that leaves about `n` arrows along the longest in-plane
    /// side.
    fn stride_for(&self, n: i64) -> usize {
        let longest = (0..3).filter(|&d| d != self.normal).map(|d| self.counts[d]).max().unwrap_or(1);
        (longest + n - 1).div_euclid(n).max(1) as usize
    }
}

/// How moments are drawn: from the tab's style, then the player's own
/// controls. The colour scheme is set once for the whole trajectory, so it
/// doesn't switch mid-playback when a frame happens to be collinear.
struct MomentLook {
    /// Å per unit of moment (`RenderStyle::moment_scale`).
    scale: f64,
    /// A mesh glyph field (OVF): arrows centred on sites, no spheres.
    arrows_only: bool,
    /// Shaft radius at width 1, Å.
    shaft_r: f64,
    show_arrows: bool,
    /// Multipliers from the Length and Width sliders.
    length: f64,
    width: f64,
    color: ArrowColor,
    /// Carries the colours for `painter::moment_color`.
    style: crate::config::RenderStyle,
    axis: Option<[f64; 3]>,
    collinear: bool,
    /// Present for a mesh: allows thinning and the colour map.
    grid: Option<Grid>,
    /// Arrows on every `stride`-th cell in-plane, scaled up to match.
    stride: usize,
    /// Colour every cell by its moment (a colour map), arrows in grey on top.
    cells: bool,
}

/// Colour-map tile radius, in cell spacings: a flat disc this size covers
/// its square cell, corners included (½√2 ≈ 0.71).
const TILE: f64 = 0.72;

/// Arrow colour over the colour map: readable on black, white and hues.
const ARROW_OVER_MAP: [f32; 3] = [0.62, 0.62, 0.64];

impl MomentLook {
    fn new(traj: &Trajectory, style: &crate::config::RenderStyle, bond_radius: f64) -> Option<Self> {
        use crate::config::MomentColorMode;
        use crate::model::structure::{collinear_with, moment_axis};
        let all = || traj.frames.iter().filter_map(|f| f.magmoms.as_ref()).flatten().copied();
        all().next()?;
        let axis = moment_axis(all());
        let collinear = axis.map_or(true, |n| collinear_with(all(), n));
        let largest = all().map(|m| (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt()).fold(0.0, f64::max);
        let arrows_only = style.moment_arrows_only;
        // Painter proportions: a shaft half a bond wide; an arrow on its own
        // is a mesh glyph, as thick as its length suggests.
        let shaft_r = if arrows_only { 0.08 * largest * style.moment_scale } else { (0.5 * bond_radius).max(0.04) };
        let color = match style.moment_color_mode {
            MomentColorMode::Auto if collinear => ArrowColor::UpDown,
            MomentColorMode::Auto | MomentColorMode::Direction => ArrowColor::Direction,
            MomentColorMode::UpDown => ArrowColor::UpDown,
            MomentColorMode::Single => ArrowColor::Single,
        };
        let grid = if arrows_only { traj.frames.first().and_then(|f| Grid::of(&f.positions)) } else { None };
        // A big mesh reads as a colour map with sparse arrows; a small one
        // as arrows on every cell.
        let (stride, cells) = match &grid {
            Some(g) if g.stride_for(48) > 1 => (g.stride_for(48), true),
            _ => (1, false),
        };
        let mut look = MomentLook {
            scale: style.moment_scale,
            arrows_only,
            shaft_r,
            show_arrows: style.show_moments,
            length: 1.0,
            width: 1.0,
            color,
            style: crate::config::RenderStyle {
                moment_color: style.moment_color,
                moment_color_down: style.moment_color_down,
                ..Default::default()
            },
            axis,
            collinear,
            grid,
            stride,
            cells,
        };
        look.set_color(color);
        Some(look)
    }

    fn rgb_of(&self, m: [f64; 3]) -> [f32; 3] {
        if self.color == ArrowColor::Mz {
            let n = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt().max(1e-12);
            let t = (m[2] / n).clamp(-1.0, 1.0) as f32;
            let (end, t) = if t >= 0.0 { ([0.80, 0.15, 0.13], t) } else { ([0.13, 0.30, 0.80], -t) };
            return [0, 1, 2].map(|k| 0.97 + (end[k] - 0.97) * t);
        }
        // `style.moment_color_mode` follows `color` (`set_color`).
        rgb(crate::rendering::painter::moment_color(m, self.axis, self.collinear, &self.style))
    }

    fn set_color(&mut self, c: ArrowColor) {
        use crate::config::MomentColorMode;
        self.color = c;
        self.style.moment_color_mode = match c {
            ArrowColor::Direction | ArrowColor::Mz => MomentColorMode::Direction,
            ArrowColor::UpDown => MomentColorMode::UpDown,
            ArrowColor::Single => MomentColorMode::Single,
        };
    }

    /// Whether site `k` carries an arrow at the current thinning.
    fn on_pitch(&self, k: usize) -> bool {
        match &self.grid {
            Some(g) if self.stride > 1 => {
                let s = self.stride as i64;
                (0..3).all(|d| d == g.normal || g.idx[k][d] % s == 0)
            }
            _ => true,
        }
    }
}

struct Player {
    traj: Rc<Trajectory>,
    /// Mean moment per frame (None for frames without moments), for the
    /// ⟨m⟩ strip and readout.
    mean_m: Vec<Option<[f64; 3]>>,
    file_name: String,
    look: Look,
    idx: usize,
    playing: bool,
    fps: f64,
    looping: bool,
    /// Bumped on every play/pause so a superseded timer stops itself.
    generation: u64,
    show_images: bool,
    show_bonds: bool,
    camera: Camera,
    backend: Option<GlBackend>,
    /// Bumped when a look control changes, to rebuild the frame.
    look_gen: u64,
    /// What is on the GPU now: (frame, images, bonds, look_gen, lift side).
    uploaded: Option<(usize, bool, bool, u64, i8)>,
    /// Fit the zoom to the window on the next draw (needs the canvas size).
    needs_fit: bool,
    pending_png: Option<std::path::PathBuf>,
}

/// Widgets that follow the frame. Weak, so timers never keep a closed
/// window alive.
#[derive(Clone)]
struct Ui {
    area: glib::WeakRef<gtk4::GLArea>,
    hud: glib::WeakRef<gtk4::DrawingArea>,
    strip: glib::WeakRef<gtk4::DrawingArea>,
    scale: glib::WeakRef<gtk4::Scale>,
    readout: glib::WeakRef<gtk4::Label>,
    play: glib::WeakRef<gtk4::Button>,
}

fn rgb(c: (f64, f64, f64)) -> [f32; 3] {
    [c.0 as f32, c.1 as f32, c.2 as f32]
}

fn cell_matrix(l: &[[f64; 3]; 3]) -> Matrix3<f64> {
    // Columns are the lattice vectors: cart = M * frac.
    Matrix3::new(l[0][0], l[1][0], l[2][0], l[0][1], l[1][1], l[2][1], l[0][2], l[1][2], l[2][2])
}

/// What one frame puts on the GPU.
struct FrameGeometry {
    atoms: Vec<AtomInstance>,
    bonds: Vec<BondInstance>,
    arrows: Vec<ArrowInstance>,
    lines: Vec<[f32; 3]>,
    /// The atoms are colour-map tiles, drawn unlit.
    flat: bool,
}

/// Atoms (with boundary images), bonds, moment arrows and cell edges of
/// frame `idx`.
///
/// `lift` (±1) is the side of a film the viewer is on, along its normal:
/// arrows over the colour map sit on that side.
fn build_frame(traj: &Trajectory, look: &Look, idx: usize, images: bool, bonds: bool, lift: f64) -> FrameGeometry {
    let f = &traj.frames[idx];
    let m = cell_matrix(&f.lattice);
    let inv = m.try_inverse();

    // (position, atom index) for every drawn sphere.
    let mut spheres: Vec<([f64; 3], usize)> = Vec::with_capacity(f.positions.len());
    for (k, p) in f.positions.iter().enumerate() {
        spheres.push((*p, k));
        let (true, Some(inv)) = (images && traj.is_periodic, inv) else { continue };
        let fr = inv * Vector3::new(p[0], p[1], p[2]);
        let shifts = |x: f64| -> Vec<f64> {
            let mut v = vec![0.0];
            if x.abs() < IMAGE_TOL {
                v.push(1.0);
            }
            if (x - 1.0).abs() < IMAGE_TOL {
                v.push(-1.0);
            }
            v
        };
        for sx in shifts(fr.x) {
            for sy in shifts(fr.y) {
                for sz in shifts(fr.z) {
                    if sx == 0.0 && sy == 0.0 && sz == 0.0 {
                        continue;
                    }
                    let c = m * (fr + Vector3::new(sx, sy, sz));
                    spheres.push(([c.x, c.y, c.z], k));
                }
            }
        }
    }

    let moment = |k: usize| -> Option<[f64; 3]> {
        look.moments.as_ref()?;
        f.magmoms.as_ref()?.get(k).copied()
    };

    let norm = |m: [f64; 3]| (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
    let map_on = look.moments.as_ref().is_some_and(|ml| ml.cells && ml.grid.is_some());

    // An arrow through each sphere that has a moment: it sticks out of the
    // sphere by ½·|m|·scale at each end, as in the main view; on its own
    // (arrows only) it is |m|·scale long, centred on the site. Thinned
    // arrows are `stride` times longer and thicker. Over the colour map
    // they are lifted off the film towards the viewer (`lift`).
    let mut arrows = Vec::new();
    if let Some(ml) = look.moments.as_ref().filter(|ml| ml.show_arrows) {
        let grow = ml.stride as f64;
        let shaft_r = ml.shaft_r * ml.width * grow;
        let lift = match (&ml.grid, map_on) {
            (Some(g), true) => {
                let mut v = [0.0; 3];
                v[g.normal] = lift * (TILE * g.spacing + 2.4 * shaft_r);
                v
            }
            _ => [0.0; 3],
        };
        for (p, k) in &spheres {
            let Some(m) = moment(*k) else { continue };
            let len = norm(m);
            if len < MIN_ARROW || !ml.on_pitch(*k) {
                continue;
            }
            let u = m.map(|x| x / len);
            let r = if ml.arrows_only { 0.0 } else { look.radii[*k] as f64 };
            let half = r + 0.5 * len * ml.scale * ml.length * grow;
            arrows.push(ArrowInstance {
                base: [0, 1, 2].map(|d| (p[d] + lift[d] - u[d] * half) as f32),
                axis: [0, 1, 2].map(|d| (2.0 * u[d] * half) as f32),
                shaft_r: shaft_r as f32,
                color: if map_on { ARROW_OVER_MAP } else { ml.rgb_of(m) },
            });
        }
    }

    // Spheres. On a mesh glyph field they are hidden, or with the colour map
    // on, every cell is a tile coloured by its moment. Otherwise atoms as
    // usual. Bonds still pair up `spheres`.
    let ml = look.moments.as_ref();
    let hide = |k: usize| {
        ml.is_some_and(|ml| ml.arrows_only && !map_on) && moment(k).is_some_and(|m| norm(m) >= MIN_ARROW)
    };
    let atoms: Vec<AtomInstance> = spheres
        .iter()
        .map(|(p, k)| match (ml, map_on) {
            (Some(ml), true) => AtomInstance {
                pos: [p[0] as f32, p[1] as f32, p[2] as f32],
                radius: (TILE * ml.grid.as_ref().map_or(1.0, |g| g.spacing)) as f32,
                color: match moment(*k).filter(|m| norm(*m) >= MIN_ARROW) {
                    Some(m) => ml.rgb_of(m),
                    None => look.colors[*k],
                },
            },
            _ => AtomInstance {
                pos: [p[0] as f32, p[1] as f32, p[2] as f32],
                radius: look.radii[*k],
                color: look.colors[*k],
            },
        })
        .collect();

    let mut bond_list = Vec::new();
    if bonds && spheres.len() <= MAX_ATOMS_FOR_BONDS {
        let ns = look.radii.len();
        let longest = look.bond_range.iter().flatten().map(|r| r.1).fold(0.0, f64::max);
        let cutoff = longest.max(0.5);
        let key = |p: &[f64; 3]| [0, 1, 2].map(|d| (p[d] / cutoff).floor() as i64);
        let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, (p, _)) in spheres.iter().enumerate() {
            grid.entry(key(p)).or_default().push(i);
        }
        for (i, (p, ki)) in spheres.iter().enumerate() {
            let c = key(p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(cell) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else { continue };
                        for &j in cell {
                            if j <= i {
                                continue;
                            }
                            let (q, kj) = &spheres[j];
                            let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                            let bonded = matches!(
                                look.bond_range[ki * ns + kj],
                                Some((lo, hi)) if d >= crate::model::bonds::MIN_SEPARATION && d > lo && d <= hi
                            );
                            if bonded {
                                bond_list.push(BondInstance {
                                    a: atoms[i].pos,
                                    b: atoms[j].pos,
                                    radius: look.bond_radius,
                                    color_a: look.bond_color,
                                    color_b: look.bond_color,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let corner = |a: f64, b: f64, c: f64| -> [f32; 3] {
        let v = m * Vector3::new(a, b, c);
        [v.x as f32, v.y as f32, v.z as f32]
    };
    let mut lines = Vec::with_capacity(24);
    for (a, b) in [
        ((0., 0., 0.), (1., 0., 0.)),
        ((0., 0., 0.), (0., 1., 0.)),
        ((0., 0., 0.), (0., 0., 1.)),
        ((1., 0., 0.), (1., 1., 0.)),
        ((1., 0., 0.), (1., 0., 1.)),
        ((0., 1., 0.), (1., 1., 0.)),
        ((0., 1., 0.), (0., 1., 1.)),
        ((0., 0., 1.), (1., 0., 1.)),
        ((0., 0., 1.), (0., 1., 1.)),
        ((1., 1., 0.), (1., 1., 1.)),
        ((1., 0., 1.), (1., 1., 1.)),
        ((0., 1., 1.), (1., 1., 1.)),
    ] {
        if traj.is_periodic {
            lines.push(corner(a.0, a.1, a.2));
            lines.push(corner(b.0, b.1, b.2));
        }
    }
    let atoms = atoms.into_iter().zip(&spheres).filter(|(_, (_, k))| !hide(*k)).map(|(a, _)| a).collect();
    FrameGeometry { atoms, bonds: bond_list, arrows, lines, flat: map_on }
}

/// Mean moment of every frame that has moments.
fn mean_moments(traj: &Trajectory) -> Vec<Option<[f64; 3]>> {
    traj.frames
        .iter()
        .map(|f| {
            let m = f.magmoms.as_ref().filter(|m| !m.is_empty())?;
            let n = m.len() as f64;
            Some(m.iter().fold([0.0; 3], |s, v| [s[0] + v[0], s[1] + v[1], s[2] + v[2]]).map(|x| x / n))
        })
        .collect()
}

/// A simulation time in the unit that suits it.
fn format_time(t: f64) -> String {
    let a = t.abs();
    let (v, unit) = if a == 0.0 {
        (0.0, "ns")
    } else if a < 1e-12 {
        (t * 1e15, "fs")
    } else if a < 1e-9 {
        (t * 1e12, "ps")
    } else if a < 1e-6 {
        (t * 1e9, "ns")
    } else if a < 1e-3 {
        (t * 1e6, "µs")
    } else {
        (t, "s")
    };
    format!("{v:.3} {unit}")
}

/// Centre and radius that hold every frame's cell (and the first frame's
/// atoms, for molecules and unwrapped coordinates), so the view stays put.
fn framing(traj: &Trajectory, look: &Look) -> (Vector3<f64>, f64) {
    let f0 = &traj.frames[0];
    let m0 = cell_matrix(&f0.lattice);
    let center = if traj.is_periodic {
        m0 * Vector3::new(0.5, 0.5, 0.5)
    } else {
        let n = f0.positions.len().max(1) as f64;
        f0.positions.iter().fold(Vector3::zeros(), |s, p| s + Vector3::new(p[0], p[1], p[2])) / n
    };
    let rmax = look.radii.iter().cloned().fold(0.0f32, f32::max) as f64;
    let mut radius: f64 = 1.0;
    for f in [&traj.frames[0], &traj.frames[traj.len() - 1]] {
        let m = cell_matrix(&f.lattice);
        if traj.is_periodic {
            for c in 0..8 {
                let v = m * Vector3::new((c & 1) as f64, ((c >> 1) & 1) as f64, ((c >> 2) & 1) as f64);
                radius = radius.max((v - center).norm());
            }
        }
        for p in f.positions.iter() {
            radius = radius.max((Vector3::new(p[0], p[1], p[2]) - center).norm() + rmax);
        }
    }
    (center, radius)
}

/// Push the player's frame into every widget that shows it.
fn refresh(ui: &Ui, player: &Rc<RefCell<Player>>) {
    let (idx, n, step, time, playing) = {
        let p = player.borrow();
        let f = &p.traj.frames[p.idx];
        (p.idx, p.traj.len(), f.step, f.time, p.playing)
    };
    if let Some(s) = ui.scale.upgrade() {
        if s.value() as usize != idx {
            s.set_value(idx as f64);
        }
    }
    if let Some(l) = ui.readout.upgrade() {
        l.set_text(&match (time, step) {
            (Some(t), _) => format!("{} / {}   {}", idx + 1, n, format_time(t)),
            (None, Some(s)) => format!("{} / {}   step {}", idx + 1, n, s),
            (None, None) => format!("{} / {}", idx + 1, n),
        });
    }
    if let Some(b) = ui.play.upgrade() {
        b.set_icon_name(if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
    }
    if let Some(a) = ui.area.upgrade() {
        a.queue_render();
    }
    for d in [&ui.hud, &ui.strip] {
        if let Some(d) = d.upgrade() {
            d.queue_draw();
        }
    }
}

fn go_to(ui: &Ui, player: &Rc<RefCell<Player>>, f: impl FnOnce(usize, usize) -> usize) {
    {
        let mut p = player.borrow_mut();
        let n = p.traj.len();
        p.idx = f(p.idx, n).min(n - 1);
    }
    refresh(ui, player);
}

fn start_playing(ui: &Ui, player: &Rc<RefCell<Player>>) {
    let (generation, fps) = {
        let mut p = player.borrow_mut();
        if p.idx + 1 >= p.traj.len() && !p.looping {
            p.idx = 0;
        }
        p.generation += 1;
        p.playing = true;
        (p.generation, p.fps.clamp(1.0, 120.0))
    };
    let weak: Weak<RefCell<Player>> = Rc::downgrade(player);
    let ui_t = ui.clone();
    glib::timeout_add_local(Duration::from_secs_f64(1.0 / fps), move || {
        let Some(player) = weak.upgrade() else { return glib::ControlFlow::Break };
        let stop = {
            let mut p = player.borrow_mut();
            if !p.playing || p.generation != generation {
                return glib::ControlFlow::Break;
            }
            if p.idx + 1 < p.traj.len() {
                p.idx += 1;
                false
            } else if p.looping {
                p.idx = 0;
                false
            } else {
                p.playing = false;
                true
            }
        };
        refresh(&ui_t, &player);
        if stop {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    refresh(ui, player);
}

fn stop_playing(ui: &Ui, player: &Rc<RefCell<Player>>) {
    {
        let mut p = player.borrow_mut();
        p.playing = false;
        p.generation += 1;
    }
    refresh(ui, player);
}

/// Rebuild the frame after a look control changed.
fn restyle(ui: &Ui, player: &Rc<RefCell<Player>>, f: impl FnOnce(&mut MomentLook)) {
    {
        let mut p = player.borrow_mut();
        let Some(ml) = p.look.moments.as_mut() else { return };
        f(ml);
        p.look_gen += 1;
    }
    refresh(ui, player);
}

/// Camera rotation that looks down a world axis from its + side, in the
/// main view's convention (screen y down, larger z farther): x shows y
/// right and z up, y shows x right and z up (from −y, a front view), z
/// shows x right and y up (a film from above).
fn look_along(axis: usize) -> nalgebra::UnitQuaternion<f64> {
    // Columns: where world x, y, z go in view space.
    let m = match axis {
        0 => Matrix3::new(0.0, 1.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0),
        1 => Matrix3::new(1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0),
        _ => Matrix3::new(1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0),
    };
    nalgebra::UnitQuaternion::from_rotation_matrix(&nalgebra::Rotation3::from_matrix_unchecked(m))
}

/// "View x y z": look down a Cartesian axis and refit.
fn view_buttons(player: &Rc<RefCell<Player>>, ui: &Ui) -> gtk4::Box {
    let b = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    b.set_margin_start(6);
    let label = gtk4::Label::new(Some("View"));
    label.set_margin_end(6);
    b.append(&label);
    for (axis, name) in ["x", "y", "z"].iter().enumerate() {
        let btn = gtk4::Button::with_label(name);
        btn.set_tooltip_text(Some(&format!("Look down the {name} axis")));
        let (player, ui) = (player.clone(), ui.clone());
        btn.connect_clicked(move |_| {
            {
                let mut p = player.borrow_mut();
                p.camera.rotation = look_along(axis);
                p.needs_fit = true;
            }
            refresh(&ui, &player);
        });
        b.append(&btn);
    }
    b
}

/// Moment controls, for a trajectory whose frames carry moments.
fn moments_row(player: &Rc<RefCell<Player>>, ui: &Ui) -> Option<gtk4::Box> {
    let (show, color, grid_counts, stride, cells) = {
        let p = player.borrow();
        let ml = p.look.moments.as_ref()?;
        (ml.show_arrows, ml.color, ml.grid.as_ref().map(|g| g.counts), ml.stride, ml.cells)
    };
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_margin_start(8);
    row.set_margin_end(8);
    row.set_margin_bottom(6);

    let arrows = gtk4::CheckButton::with_label("Arrows");
    arrows.set_active(show);
    {
        let (player, ui) = (player.clone(), ui.clone());
        arrows.connect_toggled(move |b| restyle(&ui, &player, |ml| ml.show_arrows = b.is_active()));
    }
    row.append(&arrows);

    let slider = |name: &str, tip: &str, lo: f64, hi: f64, set: fn(&mut MomentLook, f64)| {
        let l = gtk4::Label::new(Some(name));
        l.set_margin_start(6);
        row.append(&l);
        let s = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, lo.ln(), hi.ln(), 0.01);
        s.set_value(0.0);
        s.set_draw_value(false);
        s.set_size_request(90, -1);
        s.set_tooltip_text(Some(tip));
        let (player, ui) = (player.clone(), ui.clone());
        s.connect_value_changed(move |s| {
            let v = s.value().exp();
            restyle(&ui, &player, |ml| set(ml, v));
        });
        row.append(&s);
    };
    slider("Length", "Arrow length (×0.2 – ×4)", 0.2, 4.0, |ml, v| ml.length = v);
    slider("Width", "Arrow thickness (×0.25 – ×4)", 0.25, 4.0, |ml, v| ml.width = v);

    if let Some(counts) = grid_counts {
        let l = gtk4::Label::new(Some("Every"));
        l.set_margin_start(6);
        row.append(&l);
        let longest = counts.iter().copied().max().unwrap_or(1).max(1) as f64;
        let every = gtk4::SpinButton::with_range(1.0, longest, 1.0);
        every.set_value(stride as f64);
        every.set_tooltip_text(Some(
            "Draw an arrow on every n-th cell in the plane; the arrows grow to match",
        ));
        let (player, ui) = (player.clone(), ui.clone());
        every.connect_value_changed(move |s| {
            let n = s.value() as usize;
            restyle(&ui, &player, |ml| ml.stride = n.max(1));
        });
        row.append(&every);
    }

    let l = gtk4::Label::new(Some("Colour"));
    l.set_margin_start(6);
    row.append(&l);
    let dd = gtk4::DropDown::from_strings(&ArrowColor::LABELS);
    dd.set_selected(ArrowColor::ALL.iter().position(|c| *c == color).unwrap_or(0) as u32);
    dd.set_tooltip_text(Some(
        "Direction: hue from the in-plane angle, white at +z, black at −z (mumax3's colours)",
    ));
    {
        let (player, ui) = (player.clone(), ui.clone());
        dd.connect_selected_notify(move |d| {
            let c = ArrowColor::ALL[(d.selected() as usize).min(ArrowColor::ALL.len() - 1)];
            restyle(&ui, &player, |ml| ml.set_color(c));
        });
    }
    row.append(&dd);

    if grid_counts.is_some() {
        let map = gtk4::CheckButton::with_label("Colour map");
        map.set_active(cells);
        map.set_tooltip_text(Some(
            "Colour every cell by its moment; arrows are drawn in grey on top",
        ));
        let (player, ui) = (player.clone(), ui.clone());
        map.connect_toggled(move |b| restyle(&ui, &player, |ml| ml.cells = b.is_active()));
        row.append(&map);
    }
    Some(row)
}

/// GL framebuffer (RGBA, bottom row first) to a PNG.
fn save_png(path: &std::path::Path, rgba: &[u8], w: i32, h: i32) -> Result<(), String> {
    let mut surf = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let stride = surf.stride() as usize;
    {
        let mut data = surf.data().map_err(|e| e.to_string())?;
        for y in 0..h as usize {
            let src = &rgba[(h as usize - 1 - y) * w as usize * 4..][..w as usize * 4];
            let dst = &mut data[y * stride..][..w as usize * 4];
            for x in 0..w as usize {
                // Cairo ARGB32 is native-endian 0xAARRGGBB: B, G, R, A in memory on little-endian.
                let (r, g, b) = (src[x * 4], src[x * 4 + 1], src[x * 4 + 2]);
                let px = u32::from_be_bytes([255, r, g, b]).to_ne_bytes();
                dst[x * 4..x * 4 + 4].copy_from_slice(&px);
            }
        }
    }
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    surf.write_to_png(&mut file).map_err(|e| e.to_string())
}

fn draw_hud(cr: &gtk4::cairo::Context, player: &Player) {
    let f = &player.traj.frames[player.idx];
    let bg = player.look.background;
    let dark = 0.299 * bg[0] + 0.587 * bg[1] + 0.114 * bg[2] < 0.5;
    let fg = if dark { (0.95, 0.95, 0.97) } else { (0.08, 0.08, 0.10) };
    cr.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);

    let mut lines = vec![(
        17.0,
        match f.step {
            Some(s) => format!("Frame {} / {}   ·   step {}", player.idx + 1, player.traj.len(), s),
            None => format!("Frame {} / {}", player.idx + 1, player.traj.len()),
        },
    )];
    if let Some(e) = f.energy {
        let mut t = format!("E = {e:.5} eV");
        if let Some(e0) = player.traj.frames[0].energy {
            t += &format!("    ΔE = {:+.5} eV", e - e0);
        }
        lines.push((13.0, t));
    }
    if let Some(fm) = f.max_force {
        lines.push((13.0, format!("max |F| = {fm:.4} eV/Å")));
    }
    if let Some(t) = f.time {
        lines.push((13.0, format!("t = {}", format_time(t))));
    }
    if let Some(m) = player.mean_m[player.idx] {
        lines.push((13.0, format!("<m> = ({:+.3}, {:+.3}, {:+.3})", m[0], m[1], m[2])));
    }
    let mut y = 26.0;
    for (size, text) in lines {
        cr.set_font_size(size);
        cr.set_source_rgba(fg.0, fg.1, fg.2, if size > 14.0 { 0.95 } else { 0.75 });
        cr.move_to(14.0, y);
        let _ = cr.show_text(&text);
        y += size + 7.0;
    }
}

/// Top of the energy axis. A relaxation can have a wild trial step hundreds
/// of eV above the rest (a VASP overshoot at large ISIF steps), which would
/// flatten the curve that matters. The range is taken from the bulk of the
/// frames; anything far above it is pinned to the top edge and labelled.
fn robust_top(energies: &[f64]) -> f64 {
    let mut v: Vec<f64> = energies.iter().copied().filter(|e| e.is_finite()).collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let lo = v[0];
    let hi = v[v.len() - 1];
    let p90 = v[((v.len() - 1) as f64 * 0.9).round() as usize];
    let limit = p90 + 2.0 * (p90 - lo);
    v.iter().copied().filter(|&e| e <= limit).fold(lo, f64::max).min(hi)
}

/// Left and right margins of the strip plot, px (the click mapping uses them).
const STRIP_LEFT: f64 = 60.0;
const STRIP_RIGHT: f64 = 14.0;

/// Energy per frame when the file has energies, else ⟨m⟩ per frame.
fn draw_strip(cr: &gtk4::cairo::Context, w: f64, h: f64, player: &Player) {
    if player.traj.frames.iter().any(|f| f.energy.is_some()) {
        draw_energy_strip(cr, w, h, player);
    } else {
        draw_moment_strip(cr, w, h, player);
    }
}

/// The three components of the mean moment per frame, on an axis symmetric
/// about zero (±1 for an OVF magnetization), with the current frame marked.
fn draw_moment_strip(cr: &gtk4::cairo::Context, w: f64, h: f64, player: &Player) {
    let top = player.mean_m.iter().flatten().flatten().fold(0.0f64, |a, x| a.max(x.abs()));
    if top <= 0.0 {
        return;
    }
    let top = if top <= 1.0 + 1e-6 && top > 0.2 { 1.0 } else { top };
    let n = player.traj.len().max(2) as f64;
    let (l, r, t, b) = (STRIP_LEFT, w - STRIP_RIGHT, 10.0, h - 18.0);
    let xf = |i: usize| l + (r - l) * i as f64 / (n - 1.0);
    let yf = |v: f64| t + (b - t) * 0.5 * (1.0 - v / top);

    cr.set_source_rgb(0.98, 0.98, 0.99);
    cr.paint().ok();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    cr.set_line_width(1.0);
    cr.move_to(l, yf(0.0).round() + 0.5);
    cr.line_to(r, yf(0.0).round() + 0.5);
    cr.stroke().ok();

    const COLORS: [(f64, f64, f64); 3] = [(0.85, 0.25, 0.20), (0.20, 0.62, 0.30), (0.20, 0.42, 0.85)];
    for (d, c) in COLORS.iter().enumerate() {
        cr.set_source_rgb(c.0, c.1, c.2);
        cr.set_line_width(1.5);
        let mut started = false;
        for (i, m) in player.mean_m.iter().enumerate() {
            match m {
                Some(m) if started => cr.line_to(xf(i), yf(m[d])),
                Some(m) => {
                    cr.move_to(xf(i), yf(m[d]));
                    started = true;
                }
                None => {}
            }
        }
        cr.stroke().ok();
    }

    let x = xf(player.idx);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    cr.set_line_width(1.2);
    cr.move_to(x, t);
    cr.line_to(x, b);
    cr.stroke().ok();
    if let Some(m) = player.mean_m[player.idx] {
        for (d, c) in COLORS.iter().enumerate() {
            cr.set_source_rgb(c.0, c.1, c.2);
            cr.arc(x, yf(m[d]), 3.0, 0.0, std::f64::consts::TAU);
            cr.fill().ok();
        }
    }

    cr.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(10.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.7);
    cr.move_to(6.0, t + 8.0);
    let _ = cr.show_text(&format!("+{}", fmt_axis(top)));
    cr.move_to(6.0, b);
    let _ = cr.show_text(&format!("−{}", fmt_axis(top)));
    cr.move_to(l, h - 4.0);
    let _ = cr.show_text("<m> per frame:");
    let mut xl = l + 82.0;
    for (name, c) in ["x", "y", "z"].iter().zip(COLORS) {
        cr.set_source_rgb(c.0, c.1, c.2);
        cr.move_to(xl, h - 4.0);
        let _ = cr.show_text(&format!("m{name}"));
        xl += 26.0;
    }
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.7);
    cr.move_to(xl + 4.0, h - 4.0);
    let _ = cr.show_text("(click to jump)");
}

fn fmt_axis(v: f64) -> String {
    if v == 1.0 {
        "1".to_string()
    } else {
        format!("{v:.3}")
    }
}

/// Energy (relative to the lowest) per frame, with the current frame marked.
fn draw_energy_strip(cr: &gtk4::cairo::Context, w: f64, h: f64, player: &Player) {
    let pts: Vec<(usize, f64)> = player
        .traj
        .frames
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.energy.map(|e| (i, e)))
        .collect();
    if pts.is_empty() {
        return;
    }
    let lo = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let hi = robust_top(&pts.iter().map(|p| p.1).collect::<Vec<_>>());
    let span = (hi - lo).max(1e-9);
    let n = player.traj.len().max(2) as f64;
    let (l, r, t, b) = (STRIP_LEFT, w - STRIP_RIGHT, 10.0, h - 18.0);
    let xf = |i: usize| l + (r - l) * i as f64 / (n - 1.0);
    // Values above the robust range sit on the top edge.
    let yf = |e: f64| b - (b - t) * ((e - lo) / span).min(1.0);

    cr.set_source_rgb(0.98, 0.98, 0.99);
    cr.paint().ok();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
    cr.set_line_width(1.0);
    cr.move_to(l, b + 0.5);
    cr.line_to(r, b + 0.5);
    cr.stroke().ok();

    cr.set_source_rgb(0.20, 0.42, 0.85);
    cr.set_line_width(1.6);
    for (k, &(i, e)) in pts.iter().enumerate() {
        if k == 0 {
            cr.move_to(xf(i), yf(e));
        } else {
            cr.line_to(xf(i), yf(e));
        }
    }
    cr.stroke().ok();

    // Off-scale frames: a small triangle at the top and the true value.
    cr.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(9.0);
    for &(i, e) in pts.iter().filter(|p| p.1 > hi + 1e-9) {
        let x = xf(i);
        cr.set_source_rgb(0.20, 0.42, 0.85);
        cr.move_to(x - 4.0, t + 5.0);
        cr.line_to(x + 4.0, t + 5.0);
        cr.line_to(x, t - 1.0);
        cr.close_path();
        cr.fill().ok();
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.65);
        cr.move_to(x + 6.0, t + 7.0);
        let _ = cr.show_text(&format!("+{:.0} eV", e - lo));
    }

    let x = xf(player.idx);
    cr.set_source_rgba(0.85, 0.25, 0.20, 0.8);
    cr.set_line_width(1.2);
    cr.move_to(x, t);
    cr.line_to(x, b);
    cr.stroke().ok();
    if let Some(e) = player.traj.frames[player.idx].energy {
        cr.arc(x, yf(e), 3.5, 0.0, std::f64::consts::TAU);
        cr.fill().ok();
    }

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.7);
    cr.select_font_face("Sans", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(10.0);
    cr.move_to(6.0, t + 8.0);
    let _ = cr.show_text(&format!("+{:.3}", hi - lo));
    cr.move_to(6.0, b);
    let _ = cr.show_text("0 eV");
    cr.move_to(l, h - 4.0);
    let _ = cr.show_text("E − E_min per frame (click to jump)");
}

/// Open the player for the active tab's trajectory.
pub fn show(parent: &ApplicationWindow, state: Rc<RefCell<AppState>>, notebook: &Notebook) {
    let player = {
        let st = state.borrow();
        let Some(tab) = st.tabs.get(st.active_tab_index) else { return };
        let Some(traj) = tab.trajectory.clone() else {
            console::log_info("This structure has a single frame; there is nothing to play.");
            return;
        };
        let scheme = st.config.color_scheme;
        let colors = traj
            .species
            .iter()
            .map(|el| rgb(tab.style.element_colors.get(el).copied().unwrap_or_else(|| get_element_color(el, scheme))))
            .collect();
        let radii = traj
            .species
            .iter()
            .map(|el| (tab.base_radius(el) * tab.style.atom_scale) as f32)
            .collect();
        let look = Look {
            colors,
            radii,
            bond_range: {
                let table = tab.bond_table();
                traj.species
                    .iter()
                    .flat_map(|a| traj.species.iter().map(|b| table.range(a, b)))
                    .collect()
            },
            bond_radius: tab.style.bond_radius as f32,
            bond_color: rgb(tab.style.bond_color),
            background: rgb(tab.style.background_color),
            moments: MomentLook::new(&traj, &tab.style, tab.style.bond_radius),
        };
        let (center, radius) = framing(&traj, &look);
        Player {
            mean_m: mean_moments(&traj),
            idx: tab.trajectory_frame.min(traj.len() - 1),
            file_name: tab.file_name.clone(),
            camera: Camera::new(center, radius, tab.view.rotation),
            look,
            traj,
            playing: false,
            fps: 12.0,
            looping: true,
            generation: 0,
            show_images: tab.view.show_full_unit_cell,
            show_bonds: tab.view.show_bonds,
            backend: None,
            look_gen: 0,
            uploaded: None,
            needs_fit: true,
            pending_png: None,
        }
    };
    let n = player.traj.len();
    let has_strip = player.traj.frames.iter().any(|f| f.energy.is_some()) || player.mean_m.iter().any(Option::is_some);
    let title = format!("Trajectory Player — {} ({} frames, {})", player.file_name, n, player.traj.format);
    let player = Rc::new(RefCell::new(player));

    let window = gtk4::Window::builder()
        .title(title)
        .transient_for(parent)
        .default_width(960)
        .default_height(760)
        .build();

    // ---- canvas ----
    let area = gtk4::GLArea::new();
    area.set_required_version(3, 3);
    area.set_has_depth_buffer(true);
    area.set_hexpand(true);
    area.set_vexpand(true);

    let hud = gtk4::DrawingArea::new();
    hud.set_can_target(false);
    let error = gtk4::Label::new(None);
    error.set_wrap(true);
    error.set_visible(false);
    error.set_margin_start(40);
    error.set_margin_end(40);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&hud);
    overlay.add_overlay(&error);

    let strip = gtk4::DrawingArea::new();
    strip.set_content_height(84);
    strip.set_visible(has_strip);

    // ---- controls: transport + slider, then options ----
    let row = || {
        let b = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        b.set_margin_start(8);
        b.set_margin_end(8);
        b.set_margin_top(4);
        b.set_margin_bottom(4);
        b
    };
    let transport = row();
    let options = row();
    let button = |icon: &str, tip: &str| {
        let b = gtk4::Button::from_icon_name(icon);
        b.set_tooltip_text(Some(tip));
        b.add_css_class("flat");
        b
    };
    let first = button("media-skip-backward-symbolic", "First frame (Home)");
    let prev = button("media-seek-backward-symbolic", "Previous frame (,)");
    let play = button("media-playback-start-symbolic", "Play / pause (Space)");
    let next = button("media-seek-forward-symbolic", "Next frame (.)");
    let last = button("media-skip-forward-symbolic", "Last frame (End)");
    let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, (n.max(2) - 1) as f64, 1.0);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_size_request(200, -1);
    let readout = gtk4::Label::new(None);
    readout.set_width_chars(18);
    readout.set_xalign(1.0);
    let fps = gtk4::SpinButton::with_range(1.0, 120.0, 1.0);
    fps.set_value(player.borrow().fps);
    fps.set_tooltip_text(Some("Frames per second"));
    let looping = gtk4::ToggleButton::with_label("Loop");
    looping.set_active(true);
    let images = gtk4::CheckButton::with_label("Images");
    images.set_active(player.borrow().show_images);
    images.set_tooltip_text(Some("Repeat atoms on the cell faces, as the main view does"));
    let bonds = gtk4::CheckButton::with_label("Bonds");
    bonds.set_active(player.borrow().show_bonds);
    let save = gtk4::Button::with_label("Save Image…");
    save.set_tooltip_text(Some("PNG of the 3D view at screen resolution"));
    let to_main = gtk4::Button::with_label("Show in Main View");
    to_main.set_tooltip_text(Some(
        "Put this frame in the main window, for measurement, analysis and vector export (undo restores the previous one)",
    ));
    for w in [first.upcast_ref::<gtk4::Widget>(), prev.upcast_ref(), play.upcast_ref(), next.upcast_ref(), last.upcast_ref(), scale.upcast_ref(), readout.upcast_ref()] {
        transport.append(w);
    }
    options.append(&gtk4::Label::new(Some("fps")));
    for w in [fps.upcast_ref::<gtk4::Widget>(), looping.upcast_ref(), images.upcast_ref(), bonds.upcast_ref()] {
        options.append(w);
    }
    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    options.append(&spacer);
    options.append(&save);
    options.append(&to_main);

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&overlay);
    root.append(&strip);
    root.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
    root.append(&transport);
    root.append(&options);
    window.set_child(Some(&root));

    let ui = Ui {
        area: area.downgrade(),
        hud: hud.downgrade(),
        strip: strip.downgrade(),
        scale: scale.downgrade(),
        readout: readout.downgrade(),
        play: play.downgrade(),
    };

    // View along an axis, next to the display options; the moment controls
    // as a row of their own under them.
    options.insert_child_after(&view_buttons(&player, &ui), Some(&bonds));
    if let Some(row) = moments_row(&player, &ui) {
        root.insert_child_after(&row, Some(&options));
    }

    // ---- GL lifecycle ----
    {
        let player = player.clone();
        let error = error.downgrade();
        area.connect_realize(move |a| {
            a.make_current();
            let result = match a.error() {
                Some(e) => Err(e.to_string()),
                None => loader::create_context().and_then(GlBackend::new),
            };
            match result {
                Ok(b) => {
                    let mut p = player.borrow_mut();
                    p.backend = Some(b);
                    p.uploaded = None;
                    // Debugging aid: save the first frame drawn, to check the
                    // GL path on a machine where screenshots are awkward.
                    if let Some(path) = std::env::var_os("CVIEW_GL_SNAPSHOT") {
                        p.pending_png = Some(path.into());
                    }
                }
                Err(e) => {
                    console::log_error(&format!("Trajectory Player: OpenGL 3.3 is not available ({e})"));
                    if let Some(l) = error.upgrade() {
                        l.set_text(&format!(
                            "OpenGL 3.3 is not available on this system, so frames cannot be played here.\n\n{e}\n\nThe main window still shows the structure."
                        ));
                        l.set_visible(true);
                    }
                }
            }
        });
    }
    {
        let player = player.clone();
        area.connect_unrealize(move |a| {
            a.make_current();
            if let Some(mut b) = player.borrow_mut().backend.take() {
                b.destroy();
            }
        });
    }
    {
        let player = player.clone();
        area.connect_render(move |a, _| {
            let mut guard = player.borrow_mut();
            let p = &mut *guard;
            let Some(backend) = p.backend.as_mut() else { return glib::Propagation::Stop };
            // Which side of a film the viewer is on (for arrows over the
            // colour map): the film normal's depth in view space.
            let lift: i8 = match p.look.moments.as_ref().and_then(|ml| ml.grid.as_ref()) {
                Some(g) => {
                    let mut n = Vector3::zeros();
                    n[g.normal] = 1.0;
                    if (p.camera.rotation * n).z > 0.0 { -1 } else { 1 }
                }
                None => 1,
            };
            let want = (p.idx, p.show_images, p.show_bonds, p.look_gen, lift);
            if p.uploaded != Some(want) {
                let FrameGeometry { atoms, bonds, arrows, lines, flat } =
                    build_frame(&p.traj, &p.look, p.idx, p.show_images, p.show_bonds, lift as f64);
                backend.set_atoms(&atoms);
                backend.set_bonds(&bonds);
                backend.set_arrows(&arrows);
                backend.set_flat_atoms(flat);
                let bg = p.look.background;
                let dark = 0.299 * bg[0] + 0.587 * bg[1] + 0.114 * bg[2] < 0.5;
                backend.set_lines(&lines, if dark { [0.75, 0.75, 0.78] } else { [0.30, 0.30, 0.33] });
                p.uploaded = Some(want);
            }
            let sf = a.scale_factor();
            let (w, h) = (a.width() * sf, a.height() * sf);
            if p.needs_fit && w > 0 && h > 0 {
                let f = &p.traj.frames[p.idx];
                let m = cell_matrix(&f.lattice);
                let mut pts: Vec<Vector3<f64>> = f.positions.iter().map(|q| Vector3::new(q[0], q[1], q[2])).collect();
                if p.traj.is_periodic {
                    pts.extend((0..8).map(|c| m * Vector3::new((c & 1) as f64, ((c >> 1) & 1) as f64, ((c >> 2) & 1) as f64)));
                }
                let pad = p.look.radii.iter().cloned().fold(0.0f32, f32::max) as f64;
                p.camera.fit(&pts, pad, w as f64 / h as f64);
                p.needs_fit = false;
            }
            backend.draw(&p.camera, w, h, p.look.background);
            if let Some(path) = p.pending_png.take() {
                let pixels = backend.read_pixels(w, h);
                match save_png(&path, &pixels, w, h) {
                    Ok(()) => console::log_info(&format!("Saved {} ({}x{})", path.display(), w, h)),
                    Err(e) => console::log_error(&format!("Could not save {}: {e}", path.display())),
                }
            }
            glib::Propagation::Stop
        });
    }

    // ---- overlays ----
    {
        let player = player.clone();
        hud.set_draw_func(move |_, cr, _, _| draw_hud(cr, &player.borrow()));
    }
    {
        let player = player.clone();
        strip.set_draw_func(move |_, cr, w, h| draw_strip(cr, w as f64, h as f64, &player.borrow()));
    }
    {
        // Click or drag on the strip jumps to that frame.
        let seek = {
            let (player, ui) = (player.clone(), ui.clone());
            move |da: &gtk4::DrawingArea, x: f64| {
                let w = da.width() as f64;
                let n = player.borrow().traj.len();
                let t = ((x - STRIP_LEFT) / (w - STRIP_LEFT - STRIP_RIGHT)).clamp(0.0, 1.0);
                let i = (t * (n - 1) as f64).round() as usize;
                go_to(&ui, &player, |_, _| i);
            }
        };
        let drag = gtk4::GestureDrag::new();
        let s1 = seek.clone();
        drag.connect_drag_begin(move |g, x, _| {
            if let Ok(da) = g.widget().downcast::<gtk4::DrawingArea>() {
                s1(&da, x);
            }
        });
        drag.connect_drag_update(move |g, dx, _| {
            if let (Ok(da), Some((x0, _))) = (g.widget().downcast::<gtk4::DrawingArea>(), g.start_point()) {
                seek(&da, x0 + dx);
            }
        });
        strip.add_controller(drag);
    }

    // ---- camera ----
    {
        let drag = gtk4::GestureDrag::new();
        let prev_off = Rc::new(std::cell::Cell::new((0.0, 0.0)));
        let p0 = prev_off.clone();
        drag.connect_drag_begin(move |_, _, _| p0.set((0.0, 0.0)));
        let (pl, area_w) = (player.clone(), area.downgrade());
        drag.connect_drag_update(move |_, x, y| {
            let (px, py) = prev_off.get();
            prev_off.set((x, y));
            pl
                .borrow_mut()
                .camera
                .rotate_screen_deg((x - px) * DRAG_DEG_PER_PX, (y - py) * DRAG_DEG_PER_PX);
            if let Some(a) = area_w.upgrade() {
                a.queue_render();
            }
        });
        area.add_controller(drag);

        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let (player, area_w) = (player.clone(), area.downgrade());
        scroll.connect_scroll(move |_, _, dy| {
            player.borrow_mut().camera.zoom_by(1.1f64.powf(-dy));
            if let Some(a) = area_w.upgrade() {
                a.queue_render();
            }
            glib::Propagation::Stop
        });
        area.add_controller(scroll);
    }

    // ---- transport ----
    for (btn, f) in [
        (&first, (|_, _| 0) as fn(usize, usize) -> usize),
        (&last, |_, n| n - 1),
        (&next, |i, n| (i + 1) % n),
        (&prev, |i, n| (i + n - 1) % n),
    ] {
        let (player, ui) = (player.clone(), ui.clone());
        btn.connect_clicked(move |_| go_to(&ui, &player, f));
    }
    {
        let (player, ui) = (player.clone(), ui.clone());
        play.connect_clicked(move |_| {
            let playing = player.borrow().playing;
            if playing {
                stop_playing(&ui, &player);
            } else {
                start_playing(&ui, &player);
            }
        });
    }
    {
        let (player, ui) = (player.clone(), ui.clone());
        scale.connect_value_changed(move |s| {
            let i = s.value() as usize;
            if player.borrow().idx != i {
                go_to(&ui, &player, |_, _| i);
            }
        });
    }
    {
        let (player, ui) = (player.clone(), ui.clone());
        fps.connect_value_changed(move |s| {
            let playing = {
                let mut p = player.borrow_mut();
                p.fps = s.value();
                p.playing
            };
            if playing {
                start_playing(&ui, &player); // restart at the new rate
            }
        });
    }
    {
        let player = player.clone();
        looping.connect_toggled(move |b| player.borrow_mut().looping = b.is_active());
    }
    {
        let (player, ui) = (player.clone(), ui.clone());
        images.connect_toggled(move |b| {
            player.borrow_mut().show_images = b.is_active();
            refresh(&ui, &player);
        });
    }
    {
        let (player, ui) = (player.clone(), ui.clone());
        bonds.connect_toggled(move |b| {
            player.borrow_mut().show_bonds = b.is_active();
            refresh(&ui, &player);
        });
    }
    {
        let (player, area_w, win) = (player.clone(), area.downgrade(), window.downgrade());
        save.connect_clicked(move |_| {
            let Some(win) = win.upgrade() else { return };
            let dialog = gtk4::FileChooserNative::new(
                Some("Save Frame Image"),
                Some(&win),
                gtk4::FileChooserAction::Save,
                Some("Save"),
                Some("Cancel"),
            );
            let filter = gtk4::FileFilter::new();
            filter.set_name(Some("PNG image"));
            filter.add_pattern("*.png");
            dialog.add_filter(&filter);
            dialog.set_current_name(&format!("frame_{:04}.png", player.borrow().idx + 1));
            let (player, area_w) = (player.clone(), area_w.clone());
            dialog.connect_response(move |d, r| {
                if r == gtk4::ResponseType::Accept {
                    if let Some(mut path) = d.file().and_then(|f| f.path()) {
                        if path.extension().is_none() {
                            path.set_extension("png");
                        }
                        player.borrow_mut().pending_png = Some(path);
                        if let Some(a) = area_w.upgrade() {
                            a.queue_render();
                        }
                    }
                }
                d.destroy();
            });
            crate::utils::file_chooser::show(&dialog);
        });
    }
    {
        let (player, st, nb) = (player.clone(), Rc::downgrade(&state), notebook.downgrade());
        to_main.connect_clicked(move |_| {
            let Some(st) = st.upgrade() else { return };
            let (traj, idx) = {
                let p = player.borrow();
                (p.traj.clone(), p.idx)
            };
            let Some(s) = traj.structure_at(idx) else { return };
            let mut guard = st.borrow_mut();
            // Find the tab by its trajectory, not by index: tabs may have
            // been closed or reordered since the window opened.
            let Some(tab) = guard
                .tabs
                .iter_mut()
                .find(|t| t.trajectory.as_ref().is_some_and(|t| Rc::ptr_eq(t, &traj)))
            else {
                console::log_warn("Trajectory Player: the tab this trajectory came from is closed");
                return;
            };
            // Frames of one run normally keep the atom order; a frame with a
            // different atom count (LAMMPS with insertions) does not.
            let same_atoms = tab.structure.as_ref().is_some_and(|c| c.atoms.len() == s.atoms.len());
            let edit = if same_atoms { crate::state::Edit::Geometry } else { crate::state::Edit::Reindexed };
            tab.commit(&format!("Trajectory frame {} of {}", idx + 1, traj.len()), s.clone(), edit);
            tab.trajectory_frame = idx;
            drop(guard);
            if let Some(da) = nb.upgrade().and_then(|nb| crate::ui::get_active_drawing_area(&nb)) {
                da.queue_draw();
            }
        });
    }

    // ---- keyboard (capture phase, so focused buttons don't eat Space) ----
    {
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let (first, last, next, prev, play) = (first.downgrade(), last.downgrade(), next.downgrade(), prev.downgrade(), play.downgrade());
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk4::gdk::Key;
            let target = match key {
                Key::space => &play,
                Key::period | Key::Right => &next,
                Key::comma | Key::Left => &prev,
                Key::Home | Key::less => &first,
                Key::End | Key::greater => &last,
                _ => return glib::Propagation::Proceed,
            };
            if let Some(b) = target.upgrade() {
                b.emit_clicked();
            }
            glib::Propagation::Stop
        });
        window.add_controller(keys);
    }

    // Stop the timer when the window goes away.
    {
        let player = player.clone();
        window.connect_close_request(move |_| {
            let mut p = player.borrow_mut();
            p.playing = false;
            p.generation += 1;
            glib::Propagation::Proceed
        });
    }

    refresh(&ui, &player);
    window.present();

    // Debugging aid: CVIEW_PLAYER_SNAPSHOT=file.png saves the whole window
    // (canvas, strip and controls) once it has drawn, then quits.
    if let Some(path) = std::env::var_os("CVIEW_PLAYER_SNAPSHOT") {
        let win = window.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            let Some(win) = win.upgrade() else { return };
            let path = std::path::PathBuf::from(path);
            match save_window_png(&win, &path) {
                Ok(()) => console::log_info(&format!("Saved {}", path.display())),
                Err(e) => console::log_error(&format!("Could not save {}: {e}", path.display())),
            }
            if let Some(app) = gtk4::gio::Application::default() {
                app.quit();
            }
        });
    }
}

/// The window as it is on screen, to a PNG.
fn save_window_png(win: &gtk4::Window, path: &std::path::Path) -> Result<(), String> {
    let paintable = gtk4::WidgetPaintable::new(Some(win));
    let (w, h) = (win.width() as f64, win.height() as f64);
    let snap = gtk4::Snapshot::new();
    paintable.snapshot(&snap, w, h);
    let node = snap.to_node().ok_or("nothing drawn")?;
    let renderer = win.renderer().ok_or("no renderer")?;
    let texture = renderer.render_texture(&node, None);
    texture.save_to_png(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::trajectory::Frame;

    fn look(n: usize) -> Look {
        Look {
            colors: vec![[1.0; 3]; n],
            radii: vec![0.5; n],
            bond_range: vec![Some((0.0, 1.61)); n * n],
            bond_radius: 0.1,
            bond_color: [0.5; 3],
            background: [1.0; 3],
            moments: None,
        }
    }

    #[test]
    fn energy_axis_ignores_a_wild_step_but_not_a_normal_curve() {
        // The overshoot at step 5 of a real ISIF=3 relaxation.
        let e = [-58.08, -71.64, -72.54, -84.10, 455.26, -58.67, -67.38, -77.61, -90.73];
        assert_eq!(robust_top(&e), -58.08);
        let smooth = [-80.0, -85.0, -88.0, -89.5, -90.0];
        assert_eq!(robust_top(&smooth), -80.0);
        assert_eq!(robust_top(&[-1.0]), -1.0);
    }

    #[test]
    fn corner_atom_gets_seven_images_and_bonds_need_neighbours() {
        let t = Trajectory {
            species: vec!["C".into(), "C".into()],
            frames: vec![Frame {
                lattice: [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
                positions: vec![[0.0, 0.0, 0.0], [5.0, 5.0, 5.0]].into(),
                energy: None,
                max_force: None,
                step: None,
                time: None,
                magmoms: None,
            }],
            is_periodic: true,
            format: "test",
        };
        let g = build_frame(&t, &look(2), 0, true, true, 1.0);
        assert_eq!(g.atoms.len(), 1 + 7 + 1);
        assert!(g.bonds.is_empty(), "atoms 8.7 Å apart are not bonded");
        assert_eq!(g.lines.len(), 24);
        assert!(g.arrows.is_empty());
        let g = build_frame(&t, &look(2), 0, false, false, 1.0);
        assert_eq!(g.atoms.len(), 2);
    }

    #[test]
    fn arrows_only_frames_draw_arrows_and_no_spheres() {
        let t = Trajectory {
            species: vec!["Cell".into(), "Cell".into()],
            frames: vec![Frame {
                lattice: [[20.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
                positions: vec![[5.0, 5.0, 5.0], [15.0, 5.0, 5.0]].into(),
                energy: None,
                max_force: None,
                step: None,
                time: Some(1e-9),
                magmoms: Some(vec![[0.0, 0.0, 1.0], [0.0, 0.0, 0.0]]),
            }],
            is_periodic: true,
            format: "OVF",
        };
        let style = crate::config::RenderStyle { moment_arrows_only: true, moment_scale: 9.0, ..Default::default() };
        let mut l = look(2);
        l.moments = MomentLook::new(&t, &style, 0.1);
        let g = build_frame(&t, &l, 0, false, false, 1.0);
        assert_eq!(g.arrows.len(), 1, "a zero moment draws no arrow");
        // Centred on the cell, 9 Å long, along +z.
        assert_eq!(g.arrows[0].base, [5.0, 5.0, 0.5]);
        assert_eq!(g.arrows[0].axis, [0.0, 0.0, 9.0]);
        assert_eq!(g.atoms.len(), 1, "the cell with an arrow has no sphere");
        assert_eq!(mean_moments(&t)[0], Some([0.0, 0.0, 0.5]));
        assert_eq!(format_time(1.25e-9), "1.250 ns");
    }
}
