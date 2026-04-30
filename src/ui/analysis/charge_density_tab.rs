// src/ui/analysis/charge_density_tab.rs
// 2D charge density visualization tab
// Self-contained: heatmap + isolines + atom overlay rendered via Cairo
// All rendering consolidated here — no separate rendering_charge_density module

use crate::io::chgcar::{self, ChgcarData};
use crate::model::elements::{ColorScheme, get_element_color};
use crate::physics::analysis::charge_density::{
    auto_thresholds, extract_isolines, extract_slice, extract_slice_hkl, project_atoms_fractional,
    project_atoms_hkl, DensityChannel, DensitySlice, Isoline, ProjectedAtom, SlicePlane,
    ThresholdMode,
};

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Box, Button, CheckButton, ComboBoxText, DrawingArea, FileChooserAction,
    FileChooserNative, FileFilter, Frame, Label, Orientation, ResponseType, Scale, Separator,
    SpinButton,
};
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// State — public so window.rs / menu actions can inject loaded data
// ---------------------------------------------------------------------------

pub struct ChargeDensityState {
    pub chgcar_a: Option<ChgcarData>,
    pub chgcar_b: Option<ChgcarData>,
    pub difference_mode: bool,
    pub channel: DensityChannel,
    pub plane: SlicePlane,
    pub slice_pos: f64,
    pub use_hkl: bool,
    pub hkl: [i32; 3],
    pub hkl_offset: f64,
    pub n_iso_levels: usize,
    pub custom_thresholds: Vec<f64>,
    pub colormap: ColormapChoice,
    pub threshold_mode: ThresholdMode,
    pub normalize: bool,
    pub show_atoms: bool,
    pub atom_tolerance: f64,
    pub cached_slice: Option<DensitySlice>,
    pub cached_isolines: Vec<Isoline>,
    pub cached_atoms: Vec<ProjectedAtom>,
    // 3D crystal preview
    pub show_3d_atoms: bool,
    pub show_3d_cell: bool,
    pub rot_3d_x: f64,
    pub rot_3d_y: f64,
}

impl Default for ChargeDensityState {
    fn default() -> Self {
        Self {
            chgcar_a: None,
            chgcar_b: None,
            difference_mode: false,
            channel: DensityChannel::Total,
            plane: SlicePlane::XY,
            slice_pos: 0.5,
            use_hkl: false,
            hkl: [0, 0, 1],
            hkl_offset: 0.5,
            n_iso_levels: 8,
            custom_thresholds: Vec::new(),
            colormap: ColormapChoice::Viridis,
            threshold_mode: ThresholdMode::Linear,
            normalize: true,
            show_atoms: true,
            atom_tolerance: 0.05,
            cached_slice: None,
            cached_isolines: Vec::new(),
            cached_atoms: Vec::new(),
            show_3d_atoms: false,
            show_3d_cell: false,
            rot_3d_x: 0.45, // ~25° tilt for good initial view
            rot_3d_y: -0.60,
        }
    }
}

impl ChargeDensityState {
    pub fn recompute(&mut self) {
        self.cached_slice = None;
        self.cached_isolines.clear();
        self.cached_atoms.clear();

        let chgcar = match self.active_chgcar() {
            Some(c) => c,
            None => return,
        };

        let slice = if self.use_hkl {
            match extract_slice_hkl(
                &chgcar,
                self.channel,
                self.hkl,
                self.hkl_offset,
                self.normalize,
            ) {
                Some(s) => s,
                None => return,
            }
        } else {
            match extract_slice(
                &chgcar,
                self.channel,
                self.plane,
                self.slice_pos,
                self.normalize,
            ) {
                Some(s) => s,
                None => return,
            }
        };

        let thresholds = if self.custom_thresholds.is_empty() {
            auto_thresholds(&slice, self.n_iso_levels, self.threshold_mode)
        } else {
            self.custom_thresholds.clone()
        };

        self.cached_isolines = extract_isolines(&slice, &thresholds);

        // Project atoms
        if self.show_atoms {
            if self.use_hkl {
                let lat = &chgcar.lattice;
                let avg_len = (vec_len(&lat[0]) + vec_len(&lat[1]) + vec_len(&lat[2])) / 3.0;
                let tol_ang = self.atom_tolerance * avg_len;
                self.cached_atoms = project_atoms_hkl(&chgcar, self.hkl, self.hkl_offset, tol_ang);
            } else {
                self.cached_atoms = project_atoms_fractional(
                    &chgcar,
                    self.plane,
                    self.slice_pos,
                    self.atom_tolerance,
                );
            }
        }

        self.cached_slice = Some(slice);
    }

    fn active_chgcar(&self) -> Option<ChgcarData> {
        if self.difference_mode {
            let a = self.chgcar_a.as_ref()?;
            let b = self.chgcar_b.as_ref()?;
            chgcar::compute_difference(a, b).ok()
        } else {
            self.chgcar_a.clone()
        }
    }
}

fn vec_len(v: &[f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

// ---------------------------------------------------------------------------
// Colormap
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColormapChoice {
    Viridis,
    Plasma,
    BlueWhiteRed,
    Grayscale,
}

fn colormap_rgb(choice: ColormapChoice, t: f64) -> (f64, f64, f64) {
    let t = t.clamp(0.0, 1.0);
    match choice {
        ColormapChoice::Viridis => {
            let stops: &[(f64, (f64, f64, f64))] = &[
                (0.00, (0.267, 0.005, 0.329)),
                (0.13, (0.283, 0.141, 0.457)),
                (0.25, (0.254, 0.265, 0.530)),
                (0.38, (0.207, 0.372, 0.553)),
                (0.50, (0.164, 0.471, 0.558)),
                (0.63, (0.127, 0.566, 0.551)),
                (0.75, (0.190, 0.660, 0.498)),
                (0.88, (0.432, 0.761, 0.380)),
                (1.00, (0.993, 0.906, 0.144)),
            ];
            lerp_stops(t, stops)
        }
        ColormapChoice::Plasma => {
            let stops: &[(f64, (f64, f64, f64))] = &[
                (0.00, (0.050, 0.030, 0.528)),
                (0.13, (0.299, 0.006, 0.627)),
                (0.25, (0.494, 0.011, 0.657)),
                (0.38, (0.659, 0.126, 0.600)),
                (0.50, (0.796, 0.236, 0.494)),
                (0.63, (0.904, 0.369, 0.373)),
                (0.75, (0.973, 0.528, 0.259)),
                (0.88, (0.994, 0.710, 0.161)),
                (1.00, (0.940, 0.975, 0.131)),
            ];
            lerp_stops(t, stops)
        }
        ColormapChoice::BlueWhiteRed => {
            if t < 0.5 {
                let u = t * 2.0;
                (u, u, 1.0)
            } else {
                let u = (t - 0.5) * 2.0;
                (1.0, 1.0 - u, 1.0 - u)
            }
        }
        ColormapChoice::Grayscale => (t, t, t),
    }
}

fn lerp_stops(t: f64, stops: &[(f64, (f64, f64, f64))]) -> (f64, f64, f64) {
    if stops.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    if t <= stops[0].0 {
        return stops[0].1;
    }
    if t >= stops[stops.len() - 1].0 {
        return stops[stops.len() - 1].1;
    }
    for i in 1..stops.len() {
        let (t0, c0) = stops[i - 1];
        let (t1, c1) = stops[i];
        if t <= t1 {
            let u = (t - t0) / (t1 - t0);
            return (
                c0.0 + u * (c1.0 - c0.0),
                c0.1 + u * (c1.1 - c0.1),
                c0.2 + u * (c1.2 - c0.2),
            );
        }
    }
    stops[stops.len() - 1].1
}

// ---------------------------------------------------------------------------
// Cairo drawing — consolidated single renderer
// Screen vs export modes: on-screen uses light text on dark bg;
// export uses dark text on white bg with larger fonts for publication quality.
// ---------------------------------------------------------------------------

/// Rendering parameters that differ between screen (dark bg) and export (white bg)
struct PlotStyle {
    font_axis_label: f64,
    font_tick_label: f64,
    font_annotation: f64,
    font_colorbar: f64,
    font_atom_label: f64,
    isoline_width: f64,
    border_width: f64,
    tick_length: f64,
    /// Text/line color for axes, ticks, borders
    fg_color: (f64, f64, f64),
    /// Slightly lighter color for secondary text (tick values)
    fg_secondary: (f64, f64, f64),
    /// Whether to draw a halo behind the plane annotation for visibility
    annotation_halo: bool,
}

impl PlotStyle {
    /// On-screen rendering: light text on dark background
    fn screen() -> Self {
        Self {
            font_axis_label: 10.0,
            font_tick_label: 8.0,
            font_annotation: 10.0,
            font_colorbar: 9.0,
            font_atom_label: 9.0,
            isoline_width: 1.2,
            border_width: 1.0,
            tick_length: 4.0,
            fg_color: (0.85, 0.85, 0.85),
            fg_secondary: (0.7, 0.7, 0.7),
            annotation_halo: false,
        }
    }

    /// Publication-quality export: dark text on white background,
    /// with user-customisable font sizes from ExportPlotSettings.
    fn export(cfg: &crate::config::ExportPlotSettings) -> Self {
        Self {
            font_axis_label: cfg.font_size_axis_label,
            font_tick_label: cfg.font_size_tick_label,
            font_annotation: cfg.font_size_annotation,
            font_colorbar: cfg.font_size_colorbar,
            font_atom_label: 10.0,
            isoline_width: cfg.isoline_line_width,
            border_width: 1.5,
            tick_length: 6.0,
            fg_color: (0.1, 0.1, 0.1),
            fg_secondary: (0.25, 0.25, 0.25),
            annotation_halo: true,
        }
    }

    /// Hardcoded export defaults (used when no config is available)
    fn export_default() -> Self {
        Self::export(&crate::config::ExportPlotSettings::default())
    }
}

struct PlotLayout {
    plot_x: f64,
    plot_y: f64,
    plot_w: f64,
    plot_h: f64,
    cb_x: f64,
    cb_w: f64,
}

impl PlotLayout {
    fn compute(width: f64, height: f64, ps: &PlotStyle) -> Self {
        // Scale margins proportionally to font sizes for publication quality
        let margin = (ps.font_axis_label * 1.1).clamp(12.0, 28.0);
        let cb_w = (ps.font_colorbar * 2.2).clamp(22.0, 40.0);
        // Rotated (vertical) colorbar labels need only ~1.5x font height of width
        let label_w = (ps.font_colorbar * 1.8).clamp(18.0, 36.0);
        let axis_margin_left = (ps.font_tick_label * 4.0).clamp(40.0, 80.0);
        let axis_margin_bottom = (ps.font_tick_label * 3.0).clamp(28.0, 60.0);
        let plot_x = margin + axis_margin_left;
        let plot_y = margin;
        let plot_w = (width - cb_w - label_w - margin * 2.0 - axis_margin_left - 6.0).max(10.0);
        let plot_h = (height - margin * 2.0 - axis_margin_bottom).max(10.0);
        let cb_x = plot_x + plot_w + 6.0;
        PlotLayout {
            plot_x,
            plot_y,
            plot_w,
            plot_h,
            cb_x,
            cb_w,
        }
    }
}

fn draw_scene(
    cr: &cairo::Context,
    width: f64,
    height: f64,
    slice: &DensitySlice,
    isolines: &[Isoline],
    atoms: &[ProjectedAtom],
    colormap: ColormapChoice,
    is_export: bool,
    export_settings: Option<&crate::config::ExportPlotSettings>,
    clip_to_cell: bool,
    color_scheme: ColorScheme,
) {
    let ps = if is_export {
        match export_settings {
            Some(cfg) => PlotStyle::export(cfg),
            None => PlotStyle::export_default(),
        }
    } else {
        PlotStyle::screen()
    };
    let ly = PlotLayout::compute(width, height, &ps);
    let n_rows = slice.n_rows;
    let n_cols = slice.n_cols;
    let range = slice.data_max - slice.data_min;

    // ── Cell boundary clipping for HKL slices ──
    let use_clip = clip_to_cell && slice.cell_boundary_uv.len() >= 3;
    if use_clip {
        // Fill the plot area with a subtle background first, so the void
        // outside the polygon is gray (not harsh white) in exports.
        if is_export {
            cr.set_source_rgb(0.92, 0.92, 0.92);
        } else {
            cr.set_source_rgb(0.15, 0.15, 0.15);
        }
        cr.rectangle(ly.plot_x, ly.plot_y, ly.plot_w, ly.plot_h);
        let _ = cr.fill();

        // Now clip to the polygon for heatmap + isolines + atoms
        cr.save().ok();
        cr.new_path();
        let (px, py) = (
            ly.plot_x + slice.cell_boundary_uv[0].0 * ly.plot_w,
            ly.plot_y + slice.cell_boundary_uv[0].1 * ly.plot_h,
        );
        cr.move_to(px, py);
        for p in &slice.cell_boundary_uv[1..] {
            cr.line_to(ly.plot_x + p.0 * ly.plot_w, ly.plot_y + p.1 * ly.plot_h);
        }
        cr.close_path();
        cr.clip();
    }

    // ── Heatmap via bilinear-interpolated ImageSurface ──
    let pw = ly.plot_w as i32;
    let ph = ly.plot_h as i32;

    if n_rows > 0 && n_cols > 0 && pw > 0 && ph > 0 {
        if let Ok(mut surf) = cairo::ImageSurface::create(cairo::Format::Rgb24, pw, ph) {
            {
                let stride = surf.stride() as usize;
                let mut data = surf.data().expect("ImageSurface data lock failed");

                for py in 0..ph as usize {
                    for px in 0..pw as usize {
                        let gx = (px as f64 / (pw - 1).max(1) as f64) * (n_cols - 1) as f64;
                        let gy = (py as f64 / (ph - 1).max(1) as f64) * (n_rows - 1) as f64;

                        let col0 = gx.floor() as usize;
                        let row0 = gy.floor() as usize;
                        let col1 = (col0 + 1).min(n_cols - 1);
                        let row1 = (row0 + 1).min(n_rows - 1);

                        let tx = gx - gx.floor();
                        let ty = gy - gy.floor();

                        // Bilinear interpolation
                        let v = (1.0 - tx) * (1.0 - ty) * slice.data[row0][col0]
                            + tx * (1.0 - ty) * slice.data[row0][col1]
                            + (1.0 - tx) * ty * slice.data[row1][col0]
                            + tx * ty * slice.data[row1][col1];

                        let t = if range.abs() < 1e-12 {
                            0.5
                        } else {
                            (v - slice.data_min) / range
                        };
                        let (r, g, b) = colormap_rgb(colormap, t);

                        // Cairo Rgb24: B G R X byte order
                        let off = py * stride + px * 4;
                        data[off] = (b * 255.0) as u8;
                        data[off + 1] = (g * 255.0) as u8;
                        data[off + 2] = (r * 255.0) as u8;
                        data[off + 3] = 0xff;
                    }
                }
            }

            let _ = cr.set_source_surface(&surf, ly.plot_x, ly.plot_y);
            let _ = cr.paint();
        }
    }

    // ── Dashed isolines ──
    let palette: &[(f64, f64, f64)] = &[
        (1.0, 1.0, 1.0),
        (1.0, 1.0, 0.0),
        (1.0, 0.5, 0.0),
        (0.0, 1.0, 1.0),
        (1.0, 0.0, 1.0),
        (0.5, 1.0, 0.0),
        (1.0, 0.0, 0.0),
        (0.0, 0.5, 1.0),
    ];
    cr.set_line_width(ps.isoline_width);
    let dash_len = if is_export { 8.0 } else { 6.0 };
    let dash_gap = if is_export { 4.0 } else { 3.0 };
    for (i, iso) in isolines.iter().enumerate() {
        let (r, g, b) = palette[i % palette.len()];
        cr.set_source_rgba(r, g, b, 0.85);
        cr.set_dash(&[dash_len, dash_gap], (i as f64) * 2.0);
        for &((x1, y1), (x2, y2)) in &iso.segments {
            cr.move_to(ly.plot_x + x1 * ly.plot_w, ly.plot_y + y1 * ly.plot_h);
            cr.line_to(ly.plot_x + x2 * ly.plot_w, ly.plot_y + y2 * ly.plot_h);
            let _ = cr.stroke();
        }
    }
    cr.set_dash(&[], 0.0);

    // ── Atom overlay ──
    if !atoms.is_empty() {
        cr.set_font_size(ps.font_atom_label);
        let atom_radius = if is_export { 8.0 } else { 6.0 };
        for atom in atoms {
            let ax = ly.plot_x + atom.x * ly.plot_w;
            let ay = ly.plot_y + atom.y * ly.plot_h;

            // Clamp to plot area
            if ax < ly.plot_x
                || ax > ly.plot_x + ly.plot_w
                || ay < ly.plot_y
                || ay > ly.plot_y + ly.plot_h
            {
                continue;
            }

            let alpha = (1.0 - atom.distance_ang * 2.0).clamp(0.3, 1.0);
            let (er, eg, eb) = get_element_color(&atom.element, color_scheme);

            // Filled circle with dark outline
            cr.arc(ax, ay, atom_radius, 0.0, 2.0 * std::f64::consts::PI);
            cr.set_source_rgba(er, eg, eb, alpha);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(0.0, 0.0, 0.0, alpha);
            cr.set_line_width(if is_export { 1.5 } else { 1.0 });
            let _ = cr.stroke();

            // Label — with contrast outline on export for readability
            if is_export {
                cr.set_source_rgba(0.0, 0.0, 0.0, alpha * 0.7);
                for &(dx, dy) in &[(-0.8, 0.0), (0.8, 0.0), (0.0, -0.8), (0.0, 0.8)] {
                    cr.move_to(ax + atom_radius + 2.0 + dx, ay + 3.0 + dy);
                    let _ = cr.show_text(&atom.element);
                }
            }
            cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
            cr.move_to(ax + atom_radius + 2.0, ay + 3.0);
            let _ = cr.show_text(&atom.element);
        }
    }

    // ── Restore clip and draw cell boundary outline ──
    if use_clip {
        cr.restore().ok();
        // Polygon outline: dark for export (white bg), light for screen (dark bg)
        if is_export {
            cr.set_source_rgba(0.2, 0.2, 0.2, 0.9);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
        }
        cr.set_line_width(if is_export { 2.0 } else { 1.5 });
        let (px, py) = (
            ly.plot_x + slice.cell_boundary_uv[0].0 * ly.plot_w,
            ly.plot_y + slice.cell_boundary_uv[0].1 * ly.plot_h,
        );
        cr.move_to(px, py);
        for p in &slice.cell_boundary_uv[1..] {
            cr.line_to(ly.plot_x + p.0 * ly.plot_w, ly.plot_y + p.1 * ly.plot_h);
        }
        cr.close_path();
        let _ = cr.stroke();
    }

    // ── Plot border (skip when clipped — polygon boundary replaces it) ──
    if !use_clip {
        cr.set_source_rgb(ps.fg_color.0, ps.fg_color.1, ps.fg_color.2);
        cr.set_line_width(ps.border_width);
        cr.rectangle(ly.plot_x, ly.plot_y, ly.plot_w, ly.plot_h);
        let _ = cr.stroke();
    }

    // ── Axis labels and ticks ──
    draw_axis_labels(cr, &ly, slice, &ps);

    // ── Colourbar ──
    draw_colorbar(cr, &ly, slice, colormap, &ps);

    // ── Plane annotation (top-left inside plot) ──
    cr.set_font_size(ps.font_annotation);
    if ps.annotation_halo {
        // White halo for readability on varying heatmap backgrounds
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        for &(dx, dy) in &[
            (-1.0, 0.0),
            (1.0, 0.0),
            (0.0, -1.0),
            (0.0, 1.0),
            (-1.0, -1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (1.0, 1.0),
        ] {
            cr.move_to(ly.plot_x + 8.0 + dx, ly.plot_y + 18.0 + dy);
            let _ = cr.show_text(&slice.plane_annotation);
        }
        cr.set_source_rgb(0.0, 0.0, 0.0);
    } else {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.8);
    }
    cr.move_to(ly.plot_x + 8.0, ly.plot_y + 18.0);
    let _ = cr.show_text(&slice.plane_annotation);
}

fn draw_axis_labels(cr: &cairo::Context, ly: &PlotLayout, slice: &DensitySlice, ps: &PlotStyle) {
    let (r, g, b) = ps.fg_color;
    cr.set_source_rgb(r, g, b);
    cr.set_font_size(ps.font_axis_label);

    // X-axis label — centered below ticks
    let x_label_w = cr
        .text_extents(&slice.x_label)
        .map(|te| te.width())
        .unwrap_or(0.0);
    let x_label_x = ly.plot_x + ly.plot_w * 0.5 - x_label_w * 0.5;
    let x_label_y = ly.plot_y + ly.plot_h + ps.tick_length + ps.font_tick_label + 16.0;
    cr.move_to(x_label_x, x_label_y);
    let _ = cr.show_text(&slice.x_label);

    // Y-axis label (rotated) — centered alongside ticks
    cr.save().ok();
    let y_label_w = cr
        .text_extents(&slice.y_label)
        .map(|te| te.width())
        .unwrap_or(0.0);
    let y_label_x = ly.plot_x - ps.tick_length - 30.0 - ps.font_tick_label * 0.3;
    let y_label_y = ly.plot_y + ly.plot_h * 0.5 + y_label_w * 0.5;
    cr.move_to(y_label_x, y_label_y);
    cr.rotate(-std::f64::consts::FRAC_PI_2);
    let _ = cr.show_text(&slice.y_label);
    cr.restore().ok();

    // Tick marks — 5 ticks along each axis
    let (sr, sg, sb) = ps.fg_secondary;
    cr.set_font_size(ps.font_tick_label);
    cr.set_source_rgb(sr, sg, sb);
    let n_ticks = 5usize;
    for i in 0..=n_ticks {
        let frac = i as f64 / n_ticks as f64;

        // X-axis ticks
        let tx = ly.plot_x + frac * ly.plot_w;
        let val_x = frac * slice.x_extent_ang;
        cr.move_to(tx, ly.plot_y + ly.plot_h);
        cr.line_to(tx, ly.plot_y + ly.plot_h + ps.tick_length);
        let _ = cr.stroke();
        let tick_str = format!("{:.1}", val_x);
        let tick_w = cr
            .text_extents(&tick_str)
            .map(|te| te.width())
            .unwrap_or(0.0);
        cr.move_to(
            tx - tick_w * 0.5,
            ly.plot_y + ly.plot_h + ps.tick_length + ps.font_tick_label + 2.0,
        );
        let _ = cr.show_text(&tick_str);

        // Y-axis ticks
        let ty = ly.plot_y + frac * ly.plot_h;
        let val_y = frac * slice.y_extent_ang;
        cr.move_to(ly.plot_x - ps.tick_length, ty);
        cr.line_to(ly.plot_x, ty);
        let _ = cr.stroke();
        let tick_str_y = format!("{:.1}", val_y);
        let (tw_y, th_y) = cr
            .text_extents(&tick_str_y)
            .map(|te| (te.width(), te.height()))
            .unwrap_or((0.0, 0.0));
        cr.move_to(ly.plot_x - ps.tick_length - tw_y - 3.0, ty + th_y * 0.4);
        let _ = cr.show_text(&tick_str_y);
    }
}

fn draw_colorbar(
    cr: &cairo::Context,
    ly: &PlotLayout,
    slice: &DensitySlice,
    colormap: ColormapChoice,
    ps: &PlotStyle,
) {
    let steps = 256usize;
    let step_h = ly.plot_h / steps as f64;
    for i in 0..steps {
        let t = 1.0 - i as f64 / (steps - 1) as f64;
        let (r, g, b) = colormap_rgb(colormap, t);
        cr.set_source_rgb(r, g, b);
        cr.rectangle(
            ly.cb_x,
            ly.plot_y + i as f64 * step_h,
            ly.cb_w,
            step_h + 1.0,
        );
        let _ = cr.fill();
    }
    let (br, bg, bb) = ps.fg_secondary;
    cr.set_source_rgb(br, bg, bb);
    cr.set_line_width(0.8);
    cr.rectangle(ly.cb_x, ly.plot_y, ly.cb_w, ly.plot_h);
    let _ = cr.stroke();

    let (fr, fg, fb) = ps.fg_color;
    cr.set_source_rgb(fr, fg, fb);
    cr.set_font_size(ps.font_colorbar);
    for (frac, value) in &[
        (0.0f64, slice.data_max),
        (0.5, (slice.data_min + slice.data_max) * 0.5),
        (1.0, slice.data_min),
    ] {
        let y = ly.plot_y + frac * ly.plot_h;
        let label = fmt_val(*value);
        let tw = cr.text_extents(&label).map(|te| te.width()).unwrap_or(0.0);
        // Rotated label parallel to colorbar (vertical text reading bottom-to-top)
        cr.save().ok();
        cr.translate(ly.cb_x + ly.cb_w + ps.font_colorbar + 2.0, y + tw * 0.5);
        cr.rotate(-std::f64::consts::FRAC_PI_2);
        cr.move_to(0.0, 0.0);
        let _ = cr.show_text(&label);
        cr.restore().ok();
    }
}

fn fmt_val(v: f64) -> String {
    let abs = v.abs();
    if abs == 0.0 {
        "0".into()
    } else if !(0.01..100.0).contains(&abs) {
        format!("{:.2e}", v)
    } else {
        format!("{:.4}", v)
    }
}

// ---------------------------------------------------------------------------
// 3D crystal preview renderer (orthographic projection via Cairo)
// Drawn as an inset overlay inside the main 2D plot area.
// ---------------------------------------------------------------------------

/// Inset rectangle position and size: bottom-left of the plot area.
fn inset_rect(plot_w: f64, plot_h: f64) -> (f64, f64, f64, f64) {
    let iw = (plot_w * 0.32).clamp(160.0, 260.0);
    let ih = (plot_h * 0.35).clamp(130.0, 220.0);
    let margin = 8.0;
    let ix = margin;
    let iy = plot_h - ih - margin;
    (ix, iy, iw, ih)
}

fn draw_3d_preview(
    cr: &cairo::Context,
    width: f64,
    height: f64,
    state: &ChargeDensityState,
    color_scheme: ColorScheme,
) {
    // Background is drawn by the caller (rounded rect); we just draw content.
    let chgcar = match &state.chgcar_a {
        Some(c) => c,
        None => return,
    };

    let lat = &chgcar.lattice;

    // Rotation matrix from Euler angles (Y then X)
    let (sx, cx) = state.rot_3d_x.sin_cos();
    let (sy, cy) = state.rot_3d_y.sin_cos();

    let frac_to_cart = |f: [f64; 3]| -> [f64; 3] {
        [
            f[0] * lat[0][0] + f[1] * lat[1][0] + f[2] * lat[2][0],
            f[0] * lat[0][1] + f[1] * lat[1][1] + f[2] * lat[2][1],
            f[0] * lat[0][2] + f[1] * lat[1][2] + f[2] * lat[2][2],
        ]
    };

    let center = frac_to_cart([0.5, 0.5, 0.5]);

    let rotate_project = |cart: [f64; 3]| -> (f64, f64, f64) {
        let c = [
            cart[0] - center[0],
            cart[1] - center[1],
            cart[2] - center[2],
        ];
        // Rotate around Y then X
        let x1 = c[0] * cy + c[2] * sy;
        let y1 = c[1];
        let z1 = -c[0] * sy + c[2] * cy;
        let x2 = x1;
        let y2 = y1 * cx - z1 * sx;
        let z2 = y1 * sx + z1 * cx;
        (x2, y2, z2)
    };

    // Bounding box from all 8 cell corners
    let corners_frac: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let corners_cart: Vec<[f64; 3]> = corners_frac.iter().map(|f| frac_to_cart(*f)).collect();

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for c in &corners_cart {
        let (rx, ry, _) = rotate_project(*c);
        min_x = min_x.min(rx);
        max_x = max_x.max(rx);
        min_y = min_y.min(ry);
        max_y = max_y.max(ry);
    }

    let margin = 24.0;
    let range_x = (max_x - min_x).max(1e-6);
    let range_y = (max_y - min_y).max(1e-6);
    let scale = ((width - 2.0 * margin) / range_x).min((height - 2.0 * margin) / range_y);

    let project = |cart: [f64; 3]| -> (f64, f64) {
        let (rx, ry, _) = rotate_project(cart);
        (width / 2.0 + rx * scale, height / 2.0 + ry * scale)
    };

    // ── Draw cell wireframe ──
    if state.show_3d_cell {
        let edges: [(usize, usize); 12] = [
            (0, 1),
            (0, 2),
            (0, 3),
            (1, 4),
            (1, 5),
            (2, 4),
            (2, 6),
            (3, 5),
            (3, 6),
            (4, 7),
            (5, 7),
            (6, 7),
        ];
        cr.set_source_rgba(0.55, 0.65, 0.75, 0.6);
        cr.set_line_width(1.2);
        for &(i, j) in &edges {
            let (x1, y1) = project(corners_cart[i]);
            let (x2, y2) = project(corners_cart[j]);
            cr.move_to(x1, y1);
            cr.line_to(x2, y2);
            let _ = cr.stroke();
        }

        // Axis labels at the ends of a, b, c
        cr.set_font_size(11.0);
        for (idx, label) in [(1, "a"), (2, "b"), (3, "c")] {
            let (px, py) = project(corners_cart[idx]);
            let (ox, oy) = project(corners_cart[0]);
            let dx = px - ox;
            let dy = py - oy;
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            cr.set_source_rgba(0.9, 0.9, 0.4, 0.9);
            cr.move_to(px + dx / len * 10.0, py + dy / len * 10.0);
            let _ = cr.show_text(label);
        }
    }

    // ── Draw slice plane ──
    let plane_poly = compute_slice_plane_polygon(state, lat);
    if plane_poly.len() >= 3 {
        let projected: Vec<(f64, f64)> = plane_poly
            .iter()
            .map(|f| project(frac_to_cart(*f)))
            .collect();
        cr.move_to(projected[0].0, projected[0].1);
        for p in &projected[1..] {
            cr.line_to(p.0, p.1);
        }
        cr.close_path();
        cr.set_source_rgba(0.3, 0.7, 1.0, 0.22);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(0.4, 0.8, 1.0, 0.65);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }

    // ── Draw atoms (painter's algorithm — sort by depth, far first) ──
    if state.show_3d_atoms {
        let mut atom_draw_list: Vec<(f64, f64, f64, String)> = chgcar
            .atoms
            .iter()
            .map(|a| {
                let cart = frac_to_cart(a.frac_coords);
                let (px, py, pz) = rotate_project(cart);
                let sx = width / 2.0 + px * scale;
                let sy = height / 2.0 + py * scale;
                (sx, sy, pz, a.element.clone())
            })
            .collect();
        // Sort far-to-near (larger z = farther in our convention)
        atom_draw_list.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        let atom_r = 5.0;
        for (ax, ay, _, elem) in &atom_draw_list {
            let (r, g, b) = get_element_color(elem, color_scheme);
            cr.arc(*ax, *ay, atom_r, 0.0, 2.0 * std::f64::consts::PI);
            cr.set_source_rgb(r, g, b);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
            cr.set_line_width(0.8);
            let _ = cr.stroke();
        }
    }
}

/// Compute the intersection polygon of the current slice plane with the unit cell.
/// Returns fractional coordinates of the polygon vertices, angularly sorted.
fn compute_slice_plane_polygon(state: &ChargeDensityState, lat: &[[f64; 3]; 3]) -> Vec<[f64; 3]> {
    if state.use_hkl {
        // Use the MillerPlane intersection logic for HKL planes
        use crate::model::miller::MillerPlane;
        let h = state.hkl[0];
        let k = state.hkl[1];
        let l = state.hkl[2];
        if h == 0 && k == 0 && l == 0 {
            return Vec::new();
        }
        // Map offset [0,1] to the MillerPlane shift convention: h*fx + k*fy + l*fz = d
        // Cell corners give d in range [d_min, d_max]
        let hf = h as f64;
        let kf = k as f64;
        let lf = l as f64;
        let corner_ds: [f64; 8] = [0.0, hf, kf, lf, hf + kf, hf + lf, kf + lf, hf + kf + lf];
        let d_min = corner_ds.iter().cloned().fold(f64::INFINITY, f64::min);
        let d_max = corner_ds.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let shift = d_min + state.hkl_offset.clamp(0.0, 1.0) * (d_max - d_min);
        let plane = MillerPlane::new(h, k, l, shift);
        plane.get_intersection_points()
    } else {
        // Fractional axis planes: XY/XZ/YZ — always a parallelogram
        let p = state.slice_pos.clamp(0.0, 1.0);
        match state.plane {
            SlicePlane::XY => {
                // z = p: corners (0,0,p) (1,0,p) (1,1,p) (0,1,p)
                vec![[0.0, 0.0, p], [1.0, 0.0, p], [1.0, 1.0, p], [0.0, 1.0, p]]
            }
            SlicePlane::XZ => {
                // y = p: corners (0,p,0) (1,p,0) (1,p,1) (0,p,1)
                vec![[0.0, p, 0.0], [1.0, p, 0.0], [1.0, p, 1.0], [0.0, p, 1.0]]
            }
            SlicePlane::YZ => {
                // x = p: corners (p,0,0) (p,1,0) (p,1,1) (p,0,1)
                vec![[p, 0.0, 0.0], [p, 1.0, 0.0], [p, 1.0, 1.0], [p, 0.0, 1.0]]
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GTK error dialog helper
// ---------------------------------------------------------------------------

fn show_error_dialog(parent: Option<&gtk4::Window>, title: &str, message: &str) {
    let dialog = gtk4::MessageDialog::new(
        parent,
        gtk4::DialogFlags::MODAL | gtk4::DialogFlags::DESTROY_WITH_PARENT,
        gtk4::MessageType::Error,
        gtk4::ButtonsType::Close,
        message,
    );
    dialog.set_title(Some(title));
    dialog.connect_response(|d, _| d.close());
    dialog.present();
}

// ---------------------------------------------------------------------------
// Public builder
// ---------------------------------------------------------------------------

pub fn build(app_state: Option<Rc<RefCell<crate::state::AppState>>>) -> Box {
    let state = Rc::new(RefCell::new(ChargeDensityState::default()));

    // Capture export_plot config for use in export closure
    let export_cfg = app_state
        .as_ref()
        .map(|s| s.borrow().config.export_plot.clone())
        .unwrap_or_default();

    // Apply default colormap from preferences
    {
        let mut s = state.borrow_mut();
        s.colormap = match export_cfg.default_colormap {
            0 => ColormapChoice::Viridis,
            1 => ColormapChoice::Plasma,
            2 => ColormapChoice::BlueWhiteRed,
            3 => ColormapChoice::Grayscale,
            _ => ColormapChoice::Viridis,
        };
    }

    let root = Box::new(Orientation::Horizontal, 10);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);

    // ---- LEFT: drawing area ----
    let left_pane = Box::new(Orientation::Vertical, 5);
    left_pane.set_hexpand(true);
    left_pane.set_vexpand(true);

    let frame_plot = Frame::new(Some(" Charge Density Slice "));
    let drawing_area = DrawingArea::new();
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(true);
    drawing_area.set_content_width(600);
    drawing_area.set_content_height(500);
    frame_plot.set_child(Some(&drawing_area));
    left_pane.append(&frame_plot);

    let status_label = Label::new(Some("No CHGCAR loaded."));
    status_label.set_halign(Align::Start);
    status_label.add_css_class("dim-label");
    left_pane.append(&status_label);

    root.append(&left_pane);

    // ---- RIGHT: controls (scrollable) ----
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_width_request(290);
    scroll.set_vexpand(true);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let right_pane = Box::new(Orientation::Vertical, 8);
    right_pane.set_margin_start(4);
    right_pane.set_margin_end(4);

    // ── Data Files section ──
    let lbl_files = Label::new(Some("Data Files"));
    lbl_files.add_css_class("title-4");
    lbl_files.set_halign(Align::Start);
    right_pane.append(&lbl_files);

    let btn_load_a = Button::with_label("Load CHGCAR (primary)");
    right_pane.append(&btn_load_a);

    let label_file_a = Label::new(Some("—"));
    label_file_a.set_halign(Align::Start);
    label_file_a.add_css_class("dim-label");
    right_pane.append(&label_file_a);

    let check_diff = CheckButton::with_label("Difference mode  (ρ_A − ρ_B)");
    right_pane.append(&check_diff);

    let btn_load_b = Button::with_label("Load CHGCAR (secondary)");
    btn_load_b.set_sensitive(false);
    right_pane.append(&btn_load_b);

    let label_file_b = Label::new(Some("—"));
    label_file_b.set_halign(Align::Start);
    label_file_b.add_css_class("dim-label");
    right_pane.append(&label_file_b);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── 3D Preview controls ──
    let lbl_3d = Label::new(Some("3D Preview"));
    lbl_3d.add_css_class("title-4");
    lbl_3d.set_halign(Align::Start);
    right_pane.append(&lbl_3d);

    let check_3d_atoms = CheckButton::with_label("Show Atoms");
    right_pane.append(&check_3d_atoms);

    let check_3d_cell = CheckButton::with_label("Show Cell Box");
    right_pane.append(&check_3d_cell);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Normalization ──
    let check_normalize = CheckButton::with_label("Normalize to e/ų");
    check_normalize.set_active(true);
    right_pane.append(&check_normalize);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Slice section ──
    let lbl_slice = Label::new(Some("Slice"));
    lbl_slice.add_css_class("title-4");
    lbl_slice.set_halign(Align::Start);
    right_pane.append(&lbl_slice);

    let radio_frac = CheckButton::with_label("Fractional plane");
    radio_frac.set_active(true);
    let radio_hkl = CheckButton::with_label("HKL plane");
    radio_hkl.set_group(Some(&radio_frac));
    let mode_row = Box::new(Orientation::Horizontal, 12);
    mode_row.append(&radio_frac);
    mode_row.append(&radio_hkl);
    right_pane.append(&mode_row);

    // Fractional controls
    let frac_box = Box::new(Orientation::Vertical, 4);
    let plane_row = Box::new(Orientation::Horizontal, 6);
    plane_row.append(&Label::new(Some("Plane:")));
    let plane_combo = ComboBoxText::new();
    plane_combo.append_text("XY  (⊥ c-axis)");
    plane_combo.append_text("XZ  (⊥ b-axis)");
    plane_combo.append_text("YZ  (⊥ a-axis)");
    plane_combo.set_active(Some(0));
    plane_combo.set_hexpand(true);
    plane_row.append(&plane_combo);
    frac_box.append(&plane_row);

    let pos_label = Label::new(Some("Position: 0.50"));
    pos_label.set_halign(Align::Start);
    frac_box.append(&pos_label);
    let pos_adj = gtk4::Adjustment::new(0.5, 0.0, 1.0, 0.005, 0.1, 0.0);
    let pos_scale = Scale::new(Orientation::Horizontal, Some(&pos_adj));
    pos_scale.set_draw_value(false);
    pos_scale.set_hexpand(true);
    frac_box.append(&pos_scale);
    right_pane.append(&frac_box);

    // HKL controls
    let hkl_box = Box::new(Orientation::Vertical, 4);
    hkl_box.set_sensitive(false);
    let hkl_row = Box::new(Orientation::Horizontal, 4);
    hkl_row.append(&Label::new(Some("h:")));
    let spin_h = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_h.set_value(0.0);
    spin_h.set_width_chars(3);
    hkl_row.append(&spin_h);
    hkl_row.append(&Label::new(Some("k:")));
    let spin_k = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_k.set_value(0.0);
    spin_k.set_width_chars(3);
    hkl_row.append(&spin_k);
    hkl_row.append(&Label::new(Some("l:")));
    let spin_l = SpinButton::with_range(-10.0, 10.0, 1.0);
    spin_l.set_value(1.0);
    spin_l.set_width_chars(3);
    hkl_row.append(&spin_l);
    hkl_box.append(&hkl_row);

    let off_label = Label::new(Some("Offset: 0.50"));
    off_label.set_halign(Align::Start);
    hkl_box.append(&off_label);
    let off_adj = gtk4::Adjustment::new(0.5, 0.0, 1.0, 0.005, 0.1, 0.0);
    let off_scale = Scale::new(Orientation::Horizontal, Some(&off_adj));
    off_scale.set_draw_value(false);
    off_scale.set_hexpand(true);
    hkl_box.append(&off_scale);
    right_pane.append(&hkl_box);

    {
        let fb = frac_box.clone();
        let hb = hkl_box.clone();
        radio_hkl.connect_toggled(move |r| {
            let hkl = r.is_active();
            fb.set_sensitive(!hkl);
            hb.set_sensitive(hkl);
        });
    }

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Density Channel ──
    let lbl_ch = Label::new(Some("Density Channel"));
    lbl_ch.add_css_class("title-4");
    lbl_ch.set_halign(Align::Start);
    right_pane.append(&lbl_ch);

    let ch_box = Box::new(Orientation::Vertical, 4);
    let radio_total = CheckButton::with_label("Total  (ρ↑ + ρ↓)");
    radio_total.set_active(true);
    let radio_up = CheckButton::with_label("Spin-up  (ρ↑)");
    let radio_down = CheckButton::with_label("Spin-down  (ρ↓)");
    let radio_mag = CheckButton::with_label("Magnetization  (ρ↑ − ρ↓)");
    radio_up.set_group(Some(&radio_total));
    radio_down.set_group(Some(&radio_total));
    radio_mag.set_group(Some(&radio_total));
    radio_up.set_sensitive(false);
    radio_down.set_sensitive(false);
    radio_mag.set_sensitive(false);
    ch_box.append(&radio_total);
    ch_box.append(&radio_up);
    ch_box.append(&radio_down);
    ch_box.append(&radio_mag);
    right_pane.append(&ch_box);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Isolines ──
    let lbl_iso = Label::new(Some("Isolines"));
    lbl_iso.add_css_class("title-4");
    lbl_iso.set_halign(Align::Start);
    right_pane.append(&lbl_iso);

    let iso_count_label = Label::new(Some("Levels: 8"));
    iso_count_label.set_halign(Align::Start);
    right_pane.append(&iso_count_label);
    let adj_iso = gtk4::Adjustment::new(8.0, 0.0, 30.0, 1.0, 5.0, 0.0);
    let iso_scale = Scale::new(Orientation::Horizontal, Some(&adj_iso));
    iso_scale.set_draw_value(false);
    iso_scale.set_hexpand(true);
    right_pane.append(&iso_scale);

    let spin_iso = SpinButton::new(Some(&adj_iso), 1.0, 0);
    spin_iso.set_visible(false);

    // Threshold spacing mode
    let spacing_row = Box::new(Orientation::Horizontal, 8);
    let radio_linear = CheckButton::with_label("Linear");
    radio_linear.set_active(true);
    let radio_log = CheckButton::with_label("Logarithmic");
    radio_log.set_group(Some(&radio_linear));
    spacing_row.append(&Label::new(Some("Spacing:")));
    spacing_row.append(&radio_linear);
    spacing_row.append(&radio_log);
    right_pane.append(&spacing_row);

    let lbl_custom = Label::new(Some("Custom (comma-separated):"));
    lbl_custom.set_halign(Align::Start);
    lbl_custom.add_css_class("dim-label");
    right_pane.append(&lbl_custom);
    let iso_entry = gtk4::Entry::new();
    iso_entry.set_placeholder_text(Some("e.g. 0.1, 0.5, 1.0"));
    right_pane.append(&iso_entry);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Atom Overlay ──
    let lbl_atoms = Label::new(Some("Atom Overlay"));
    lbl_atoms.add_css_class("title-4");
    lbl_atoms.set_halign(Align::Start);
    right_pane.append(&lbl_atoms);

    let check_atoms = CheckButton::with_label("Show atoms on slice");
    check_atoms.set_active(true);
    right_pane.append(&check_atoms);

    let tol_label = Label::new(Some("Tolerance: 0.05 (frac)"));
    tol_label.set_halign(Align::Start);
    right_pane.append(&tol_label);
    let tol_adj = gtk4::Adjustment::new(0.05, 0.01, 0.50, 0.01, 0.05, 0.0);
    let tol_scale = Scale::new(Orientation::Horizontal, Some(&tol_adj));
    tol_scale.set_draw_value(false);
    tol_scale.set_hexpand(true);
    right_pane.append(&tol_scale);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Colourmap ──
    let lbl_cmap = Label::new(Some("Colourmap"));
    lbl_cmap.add_css_class("title-4");
    lbl_cmap.set_halign(Align::Start);
    right_pane.append(&lbl_cmap);

    let cmap_combo = ComboBoxText::new();
    cmap_combo.append_text("Viridis");
    cmap_combo.append_text("Plasma");
    cmap_combo.append_text("Blue–White–Red");
    cmap_combo.append_text("Grayscale");
    cmap_combo.set_active(Some(export_cfg.default_colormap as u32));
    right_pane.append(&cmap_combo);

    right_pane.append(&Separator::new(Orientation::Horizontal));

    // ── Action buttons ──
    let btn_update = Button::with_label("Update Plot");
    btn_update.add_css_class("suggested-action");
    right_pane.append(&btn_update);

    let btn_export = Button::with_label("Export PNG / PDF");
    right_pane.append(&btn_export);

    scroll.set_child(Some(&right_pane));
    root.append(&scroll);

    // ========================================================================
    // Signal connections
    // ========================================================================

    // Draw function
    {
        let st = state.clone();
        let app_st_draw = app_state.clone();
        drawing_area.set_draw_func(move |_, cr, w, h| {
            let wf = w as f64;
            let hf = h as f64;
            cr.set_source_rgb(0.1, 0.1, 0.1);
            cr.rectangle(0.0, 0.0, wf, hf);
            let _ = cr.fill();
            let st = st.borrow();
            let scheme = app_st_draw
                .as_ref()
                .map(|s| s.borrow().config.color_scheme)
                .unwrap_or_default();
            if let Some(slice) = &st.cached_slice {
                draw_scene(
                    cr,
                    wf,
                    hf,
                    slice,
                    &st.cached_isolines,
                    &st.cached_atoms,
                    st.colormap,
                    false,
                    None,
                    st.show_3d_cell,
                    scheme,
                );
                // ── 3D inset overlay (bottom-left corner) ──
                if st.show_3d_atoms || st.show_3d_cell {
                    let (ix, iy, iw, ih) = inset_rect(wf, hf);
                    // Rounded-rect background
                    let r = 6.0;
                    cr.new_sub_path();
                    cr.arc(ix + iw - r, iy + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
                    cr.arc(
                        ix + iw - r,
                        iy + ih - r,
                        r,
                        0.0,
                        std::f64::consts::FRAC_PI_2,
                    );
                    cr.arc(
                        ix + r,
                        iy + ih - r,
                        r,
                        std::f64::consts::FRAC_PI_2,
                        std::f64::consts::PI,
                    );
                    cr.arc(
                        ix + r,
                        iy + r,
                        r,
                        std::f64::consts::PI,
                        3.0 * std::f64::consts::FRAC_PI_2,
                    );
                    cr.close_path();
                    cr.set_source_rgba(0.08, 0.08, 0.11, 0.88);
                    let _ = cr.fill_preserve();
                    cr.set_source_rgba(0.4, 0.5, 0.6, 0.5);
                    cr.set_line_width(1.0);
                    let _ = cr.stroke();
                    // Clip + translate for the 3D draw
                    cr.save().ok();
                    cr.rectangle(ix, iy, iw, ih);
                    cr.clip();
                    cr.translate(ix, iy);
                    draw_3d_preview(cr, iw, ih, &st, scheme);
                    cr.restore().ok();
                }
            } else {
                cr.set_source_rgb(0.55, 0.55, 0.55);
                cr.set_font_size(15.0);
                cr.move_to(wf / 2.0 - 120.0, hf / 2.0);
                let _ = cr.show_text("Load a CHGCAR to visualize");
            }
        });
    }

    // Inject primary CHGCAR data
    let inject_primary = {
        let st = state.clone();
        let lbl = label_file_a.clone();
        let status = status_label.clone();
        let ru = radio_up.clone();
        let rd = radio_down.clone();
        let rm = radio_mag.clone();
        let da = drawing_area.clone();
        Rc::new(move |data: ChgcarData, filename: String| {
            let spin = data.spin_polarized;
            let n_atoms = data.atoms.len();
            status.set_text(&format!(
                "Grid {}×{}×{} | {} atoms | {}",
                data.grid[0],
                data.grid[1],
                data.grid[2],
                n_atoms,
                if spin {
                    "Spin-polarized"
                } else {
                    "Non-spin-polarized"
                }
            ));
            lbl.set_text(&filename);
            ru.set_sensitive(spin);
            rd.set_sensitive(spin);
            rm.set_sensitive(spin);
            {
                let mut s = st.borrow_mut();
                if !spin {
                    s.channel = DensityChannel::Total;
                }
                s.chgcar_a = Some(data);
                s.recompute();
            }
            da.queue_draw();
        })
    };

    // Load primary CHGCAR button
    {
        let inject = inject_primary.clone();
        btn_load_a.connect_clicked(move |btn| {
            let native = chgcar_chooser(
                btn.root()
                    .and_then(|r| r.downcast::<gtk4::Window>().ok())
                    .as_ref(),
            );
            let inject2 = inject.clone();
            let btn_weak = btn.downgrade();
            native.connect_response(move |d, resp| {
                if resp == ResponseType::Accept {
                    if let Some(path) = d.file().and_then(|f| f.path()) {
                        let filename = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        match chgcar::parse(path.to_str().unwrap_or("")) {
                            Ok(data) => inject2(data, filename),
                            Err(e) => {
                                let parent_win = btn_weak
                                    .upgrade()
                                    .and_then(|b| b.root())
                                    .and_then(|r| r.downcast::<gtk4::Window>().ok());
                                show_error_dialog(
                                    parent_win.as_ref(),
                                    "CHGCAR Load Error",
                                    &format!("Failed to parse CHGCAR file:\n{}", e),
                                );
                            }
                        }
                    }
                }
            });
            native.show();
        });
    }

    // Difference mode toggle
    {
        let btn_b = btn_load_b.clone();
        check_diff.connect_toggled(move |cb| btn_b.set_sensitive(cb.is_active()));
    }

    // Load secondary CHGCAR
    {
        let st = state.clone();
        let lbl = label_file_b.clone();
        let status = status_label.clone();
        let da = drawing_area.clone();

        btn_load_b.connect_clicked(move |btn| {
            let native = chgcar_chooser(
                btn.root()
                    .and_then(|r| r.downcast::<gtk4::Window>().ok())
                    .as_ref(),
            );
            let st2 = st.clone();
            let lbl2 = lbl.clone();
            let status2 = status.clone();
            let da2 = da.clone();
            let btn_weak = btn.downgrade();

            native.connect_response(move |d, resp| {
                if resp == ResponseType::Accept {
                    if let Some(path) = d.file().and_then(|f| f.path()) {
                        match chgcar::parse(path.to_str().unwrap_or("")) {
                            Ok(data) => {
                                lbl2.set_text(
                                    &path.file_name().unwrap_or_default().to_string_lossy(),
                                );
                                status2.set_text("Secondary CHGCAR loaded. Click 'Update Plot'.");
                                st2.borrow_mut().chgcar_b = Some(data);
                            }
                            Err(e) => {
                                let parent_win = btn_weak
                                    .upgrade()
                                    .and_then(|b| b.root())
                                    .and_then(|r| r.downcast::<gtk4::Window>().ok());
                                show_error_dialog(
                                    parent_win.as_ref(),
                                    "CHGCAR Load Error",
                                    &format!("Failed to parse secondary CHGCAR:\n{}", e),
                                );
                            }
                        }
                        da2.queue_draw();
                    }
                }
            });
            native.show();
        });
    }

    // ── 3D inset: mouse drag to rotate (on the main drawing area) ──
    {
        let st = state.clone();
        let da = drawing_area.clone();
        let drag = gtk4::GestureDrag::new();
        let drag_start_rot = Rc::new(RefCell::new((0.0f64, 0.0f64)));
        let drag_in_inset = Rc::new(RefCell::new(false));

        let ds = drag_start_rot.clone();
        let di = drag_in_inset.clone();
        let st2 = st.clone();
        let da2 = da.clone();
        drag.connect_drag_begin(move |_, x, y| {
            let wf = da2.width() as f64;
            let hf = da2.height() as f64;
            let (ix, iy, iw, ih) = inset_rect(wf, hf);
            let s = st2.borrow();
            let in_inset = (s.show_3d_atoms || s.show_3d_cell)
                && s.chgcar_a.is_some()
                && x >= ix
                && x <= ix + iw
                && y >= iy
                && y <= iy + ih;
            *di.borrow_mut() = in_inset;
            if in_inset {
                *ds.borrow_mut() = (s.rot_3d_y, s.rot_3d_x);
            }
        });

        let ds2 = drag_start_rot.clone();
        let di2 = drag_in_inset.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            if !*di2.borrow() {
                return;
            }
            let start = *ds2.borrow();
            let mut s = st.borrow_mut();
            s.rot_3d_y = start.0 + dx * 0.008;
            s.rot_3d_x = start.1 + dy * 0.008;
            drop(s);
            da.queue_draw();
        });

        drawing_area.add_controller(drag);
    }

    // ── 3D preview: checkbox toggles ──
    {
        let st = state.clone();
        let da = drawing_area.clone();
        check_3d_atoms.connect_toggled(move |cb| {
            st.borrow_mut().show_3d_atoms = cb.is_active();
            da.queue_draw();
        });
    }
    {
        let st = state.clone();
        let da = drawing_area.clone();
        check_3d_cell.connect_toggled(move |cb| {
            st.borrow_mut().show_3d_cell = cb.is_active();
            da.queue_draw();
        });
    }

    // Live update: position slider
    {
        let st = state.clone();
        let da = drawing_area.clone();
        let status = status_label.clone();
        let lbl = pos_label.clone();
        pos_adj.connect_value_changed(move |adj| {
            lbl.set_text(&format!("Position: {:.2}", adj.value()));
            let mut s = st.borrow_mut();
            if s.chgcar_a.is_some() {
                s.slice_pos = adj.value();
                s.use_hkl = false;
                s.recompute();
                if let Some(slice) = &s.cached_slice {
                    status.set_text(&format!(
                        "pos={:.3} | [{:.3e} … {:.3e}] | {} atoms",
                        adj.value(),
                        slice.data_min,
                        slice.data_max,
                        s.cached_atoms.len()
                    ));
                }
                da.queue_draw();
            }
        });
    }

    // Live update: HKL offset slider
    {
        let st = state.clone();
        let da = drawing_area.clone();
        let status = status_label.clone();
        let lbl = off_label.clone();
        off_adj.connect_value_changed(move |adj| {
            lbl.set_text(&format!("Offset: {:.2}", adj.value()));
            let mut s = st.borrow_mut();
            if s.chgcar_a.is_some() && s.use_hkl {
                s.hkl_offset = adj.value();
                s.recompute();
                if let Some(slice) = &s.cached_slice {
                    status.set_text(&format!(
                        "HKL offset={:.3} | [{:.3e} … {:.3e}] | {} atoms",
                        adj.value(),
                        slice.data_min,
                        slice.data_max,
                        s.cached_atoms.len()
                    ));
                }
                da.queue_draw();
            }
        });
    }

    // Live update: isoline count slider
    {
        let st = state.clone();
        let da = drawing_area.clone();
        let lbl = iso_count_label.clone();
        let iso_entry = iso_entry.clone();
        adj_iso.connect_value_changed(move |adj| {
            let n = adj.value() as usize;
            lbl.set_text(&format!("Levels: {}", n));
            let mut s = st.borrow_mut();
            if s.cached_slice.is_some() {
                s.n_iso_levels = n;
                s.custom_thresholds = parse_thresholds(&iso_entry.text());
                let thresholds = if s.custom_thresholds.is_empty() {
                    auto_thresholds(
                        s.cached_slice.as_ref().unwrap(),
                        s.n_iso_levels,
                        s.threshold_mode,
                    )
                } else {
                    s.custom_thresholds.clone()
                };
                s.cached_isolines = extract_isolines(s.cached_slice.as_ref().unwrap(), &thresholds);
                da.queue_draw();
            }
        });
    }

    // Live update: atom tolerance slider
    {
        let st = state.clone();
        let da = drawing_area.clone();
        let lbl = tol_label.clone();
        tol_adj.connect_value_changed(move |adj| {
            lbl.set_text(&format!("Tolerance: {:.2} (frac)", adj.value()));
            let mut s = st.borrow_mut();
            s.atom_tolerance = adj.value();
            if s.chgcar_a.is_some() && s.cached_slice.is_some() {
                // Recompute atom projection only (slice stays cached)
                s.cached_atoms.clear();
                if s.show_atoms {
                    if let Some(ref chgcar) = s.chgcar_a {
                        if s.use_hkl {
                            let lat = &chgcar.lattice;
                            let avg_len =
                                (vec_len(&lat[0]) + vec_len(&lat[1]) + vec_len(&lat[2])) / 3.0;
                            let tol_ang = s.atom_tolerance * avg_len;
                            s.cached_atoms =
                                project_atoms_hkl(chgcar, s.hkl, s.hkl_offset, tol_ang);
                        } else {
                            s.cached_atoms = project_atoms_fractional(
                                chgcar,
                                s.plane,
                                s.slice_pos,
                                s.atom_tolerance,
                            );
                        }
                    }
                }
                da.queue_draw();
            }
        });
    }

    // Update Plot button — reads all controls
    {
        let st = state.clone();
        let da = drawing_area.clone();
        let status = status_label.clone();
        let plane_combo = plane_combo.clone();
        let pos_adj = pos_adj.clone();
        let spin_iso = spin_iso.clone();
        let iso_entry = iso_entry.clone();
        let cmap_combo = cmap_combo.clone();
        let check_diff = check_diff.clone();
        let radio_up = radio_up.clone();
        let radio_down = radio_down.clone();
        let radio_mag = radio_mag.clone();
        let radio_hkl = radio_hkl.clone();
        let spin_h = spin_h.clone();
        let spin_k = spin_k.clone();
        let spin_l = spin_l.clone();
        let off_adj = off_adj.clone();
        let radio_log = radio_log.clone();
        let check_normalize = check_normalize.clone();
        let check_atoms = check_atoms.clone();
        let tol_adj = tol_adj.clone();

        btn_update.connect_clicked(move |_| {
            let mut s = st.borrow_mut();
            s.use_hkl = radio_hkl.is_active();
            if s.use_hkl {
                s.hkl = [
                    spin_h.value_as_int(),
                    spin_k.value_as_int(),
                    spin_l.value_as_int(),
                ];
                s.hkl_offset = off_adj.value();
            } else {
                s.plane = match plane_combo.active() {
                    Some(0) => SlicePlane::XY,
                    Some(1) => SlicePlane::XZ,
                    Some(2) => SlicePlane::YZ,
                    _ => SlicePlane::XY,
                };
                s.slice_pos = pos_adj.value();
            }
            s.channel = if radio_up.is_active() {
                DensityChannel::SpinUp
            } else if radio_down.is_active() {
                DensityChannel::SpinDown
            } else if radio_mag.is_active() {
                DensityChannel::Magnetization
            } else {
                DensityChannel::Total
            };
            s.difference_mode = check_diff.is_active();
            s.colormap = match cmap_combo.active() {
                Some(0) => ColormapChoice::Viridis,
                Some(1) => ColormapChoice::Plasma,
                Some(2) => ColormapChoice::BlueWhiteRed,
                Some(3) => ColormapChoice::Grayscale,
                _ => ColormapChoice::Viridis,
            };
            s.n_iso_levels = spin_iso.value_as_int() as usize;
            s.custom_thresholds = parse_thresholds(&iso_entry.text());
            s.threshold_mode = if radio_log.is_active() {
                ThresholdMode::Logarithmic
            } else {
                ThresholdMode::Linear
            };
            s.normalize = check_normalize.is_active();
            s.show_atoms = check_atoms.is_active();
            s.atom_tolerance = tol_adj.value();
            s.recompute();

            if let Some(slice) = &s.cached_slice {
                let plane_str = if s.use_hkl {
                    format!("({} {} {})", s.hkl[0], s.hkl[1], s.hkl[2])
                } else {
                    match s.plane {
                        SlicePlane::XY => "XY".into(),
                        SlicePlane::XZ => "XZ".into(),
                        SlicePlane::YZ => "YZ".into(),
                    }
                };
                let unit_str = if s.normalize { "e/ų" } else { "e" };
                status.set_text(&format!(
                    "{} | [{:.3e} … {:.3e}] {} | {} isolines | {} atoms",
                    plane_str,
                    slice.data_min,
                    slice.data_max,
                    unit_str,
                    s.cached_isolines.len(),
                    s.cached_atoms.len(),
                ));
            } else {
                status.set_text("No data — load a CHGCAR first.");
            }
            da.queue_draw();
        });
    }

    // Export PNG / PDF
    {
        let st = state.clone();
        let app_st = app_state.clone();
        btn_export.connect_clicked(move |btn| {
            let s = st.borrow();
            if s.cached_slice.is_none() {
                return;
            }
            let slice = s.cached_slice.clone().unwrap();
            let isolines = s.cached_isolines.clone();
            let atoms = s.cached_atoms.clone();
            let colormap = s.colormap;
            let clip_cell = s.show_3d_cell;
            drop(s);

            // Read export settings live from app config (picks up preference changes)
            let export_cfg = app_st
                .as_ref()
                .map(|s| s.borrow().config.export_plot.clone())
                .unwrap_or_default();
            let export_scheme = app_st
                .as_ref()
                .map(|s| s.borrow().config.color_scheme)
                .unwrap_or_default();

            // Read export settings from app config (captured at build time)
            // export_cfg is already read live above

            let native = FileChooserNative::new(
                Some("Export PNG / PDF"),
                btn.root()
                    .and_then(|r| r.downcast::<gtk4::Window>().ok())
                    .as_ref(),
                FileChooserAction::Save,
                Some("Save"),
                Some("Cancel"),
            );
            let f_png = gtk4::FileFilter::new();
            f_png.set_name(Some("PNG Image (*.png)"));
            f_png.add_pattern("*.png");
            native.add_filter(&f_png);
            let f_pdf = gtk4::FileFilter::new();
            f_pdf.set_name(Some("PDF Document (*.pdf)"));
            f_pdf.add_pattern("*.pdf");
            native.add_filter(&f_pdf);
            native.set_current_name("charge_density.png");

            native.connect_response(move |d, resp| {
                if resp == ResponseType::Accept {
                    if let Some(path) = d.file().and_then(|f| f.path()) {
                        let w = 1600.0f64;
                        let h = 1200.0f64;
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();

                        if ext == "pdf" {
                            if let Ok(surf) = cairo::PdfSurface::new(w, h, &path) {
                                if let Ok(ctx) = cairo::Context::new(&surf) {
                                    ctx.set_source_rgb(1.0, 1.0, 1.0);
                                    ctx.rectangle(0.0, 0.0, w, h);
                                    let _ = ctx.fill();
                                    draw_scene(
                                        &ctx,
                                        w,
                                        h,
                                        &slice,
                                        &isolines,
                                        &atoms,
                                        colormap,
                                        true,
                                        Some(&export_cfg),
                                        clip_cell,
                                        export_scheme,
                                    );
                                }
                                surf.finish();
                                println!("Exported PDF: {}", path.display());
                            }
                        } else if let Ok(surf) =
                            cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32)
                        {
                            if let Ok(ctx) = cairo::Context::new(&surf) {
                                ctx.set_source_rgb(1.0, 1.0, 1.0);
                                ctx.rectangle(0.0, 0.0, w, h);
                                let _ = ctx.fill();
                                draw_scene(
                                    &ctx,
                                    w,
                                    h,
                                    &slice,
                                    &isolines,
                                    &atoms,
                                    colormap,
                                    true,
                                    Some(&export_cfg),
                                    clip_cell,
                                    export_scheme,
                                );
                            }
                            if let Ok(mut file) = std::fs::File::create(&path) {
                                let _ = surf.write_to_png(&mut file);
                                println!("Exported PNG: {}", path.display());
                            }
                        }
                    }
                }
            });
            native.show();
        });
    }

    root
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn chgcar_chooser(parent: Option<&gtk4::Window>) -> FileChooserNative {
    let native = FileChooserNative::new(
        Some("Open CHGCAR"),
        parent,
        FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );
    let filter = FileFilter::new();
    filter.set_name(Some("VASP CHGCAR / all files"));
    filter.add_pattern("CHGCAR");
    filter.add_pattern("CHGCAR*");
    filter.add_pattern("*.chgcar");
    filter.add_pattern("*");
    native.add_filter(&filter);
    native
}

fn parse_thresholds(text: &glib::GString) -> Vec<f64> {
    let s = text.as_str();
    if s.trim().is_empty() {
        return Vec::new();
    }
    s.split(',')
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .collect()
}
