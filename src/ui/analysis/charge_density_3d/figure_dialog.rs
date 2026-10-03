// src/ui/analysis/charge_density_3d/figure_dialog.rs
//
// "Export Figure…": a journal-ready image of the 3D charge-density view.
//
// The user picks a physical size (journal presets or custom mm), dpi,
// supersampling, background and which annotations to add. While the dialog
// is open the 3D view shows the figure's frame, so what is framed is what is
// exported. Export renders offscreen through `GlBackend::render_image`
// (tiled, supersampled, depth-peeled transparency) and writes PDF/SVG (raster
// picture + vector annotations) or PNG/TIFF (dpi in the file) through
// `rendering::figure`, plus a scene file next to it.

use super::{frame_rect, prepare_backend, redraw, scene_file, Lobes, Ui, View};
use crate::rendering::figure::{self, Annotations, ColorBar, Page, NATURE_MAX_HEIGHT_MM, PRESETS};
use crate::rendering::gl::ExportRequest;
use crate::utils::console;
use gtk4::prelude::*;
use gtk4::{glib, Align, Orientation};
use nalgebra::{Vector3, Vector4};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// The figure's settings; stored in scene files for exact re-export.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FigureSettings {
    pub preset: usize,
    pub width_mm: f64,
    pub height_mm: f64,
    pub dpi: f64,
    pub supersample: u32,
    pub transparent: bool,
    pub font_pt: f64,
    pub line_pt: f64,
    pub panel_letter: String,
    pub title: bool,
    pub key: bool,
    pub color_bar: bool,
    pub axes: bool,
    pub scale_bar: bool,
    pub atom_labels: bool,
}

impl Default for FigureSettings {
    fn default() -> Self {
        Self {
            preset: 1, // Nature, 2 columns
            width_mm: 183.0,
            height_mm: 120.0,
            dpi: 600.0,
            supersample: 3,
            transparent: false,
            font_pt: 7.0,
            line_pt: 0.5,
            panel_letter: "a".into(),
            title: true,
            key: true,
            color_bar: true,
            axes: true,
            scale_bar: true,
            atom_labels: false,
        }
    }
}

thread_local! {
    /// Last settings used, so the next figure in a session matches.
    static LAST: RefCell<FigureSettings> = RefCell::new(FigureSettings::default());
}

const DPIS: [f64; 4] = [300.0, 450.0, 600.0, 1200.0];
const SUPERSAMPLES: [u32; 3] = [2, 3, 4];

/// Annotations for `v` given the exported view-space window (Å).
fn annotations(v: &View, s: &FigureSettings, window: [f64; 4], page: &Page) -> Annotations {
    let Some(f) = v.field.as_ref() else { return Annotations::default() };
    let mut a = Annotations::default();
    let letter = s.panel_letter.trim();
    if !letter.is_empty() {
        a.panel_letter = Some(letter.to_string());
    }
    if s.title {
        a.title = Some(f.label.clone());
    }
    if s.key {
        let iso = figure::format_value(v.iso);
        let unit = "e/Å³";
        if v.lobes != Lobes::Negative && v.lobes_now[0].is_some() {
            a.key.push((v.color_pos.map(|c| c as f64), format!("ρ = +{iso} {unit}")));
        }
        if v.lobes != Lobes::Positive && v.lobes_now[1].is_some() {
            a.key.push((v.color_neg.map(|c| c as f64), format!("ρ = −{iso} {unit}")));
        }
    }
    if s.color_bar && v.section.show {
        let signed = f.signed();
        a.color_bar = Some(ColorBar {
            signed,
            lo: if signed { 0.0 } else { f.abs_max() * 1e-4 },
            hi: if signed { v.iso } else { f.abs_max() },
            label: if signed { "Δρ (e/Å³)".into() } else { "ρ (e/Å³, log scale)".into() },
        });
    }
    if s.axes {
        let r = v.camera.rotation;
        let l = f.lattice;
        let dirs: Vec<[f64; 2]> = (0..3)
            .map(|i| {
                let vec = Vector3::new(l[i][0], l[i][1], l[i][2]);
                let p = r * vec.normalize();
                [p.x, p.y] // main-view convention: x right, y down
            })
            .collect();
        a.axes = Some([dirs[0], dirs[1], dirs[2]]);
    }
    let width_a = window[1] - window[0];
    if s.scale_bar {
        let pt_per_a = page.width_pt() / width_a;
        a.scale_bar = Some((figure::nice_length(width_a * 0.2), pt_per_a));
    }
    if s.atom_labels {
        let view = v.camera.view().cast::<f64>();
        let spheres = super::atom_spheres(f);
        let proj: Vec<(String, Vector4<f64>, f64)> = spheres
            .iter()
            .map(|(el, c, r)| (el.clone(), view * Vector4::new(c.x, c.y, c.z, 1.0), *r))
            .collect();
        let [l0, r0, b0, t0] = window;
        for (i, (el, p, _)) in proj.iter().enumerate() {
            // Hidden behind a nearer atom (GL view: larger z is nearer).
            let hidden = proj.iter().enumerate().any(|(j, (_, q, rq))| {
                j != i && q.z > p.z && ((q.x - p.x).powi(2) + (q.y - p.y).powi(2)).sqrt() < *rq
            });
            if hidden || p.x < l0 || p.x > r0 || p.y < b0 || p.y > t0 {
                continue;
            }
            let x = (p.x - l0) / (r0 - l0) * page.width_pt();
            let y = (t0 - p.y) / (t0 - b0) * page.height_pt();
            a.labels.push((el.clone(), x, y));
        }
    }
    a
}

fn page_for(s: &FigureSettings) -> Page {
    Page {
        width_mm: s.width_mm,
        height_mm: s.height_mm,
        dpi: s.dpi,
        font_pt: s.font_pt,
        line_pt: s.line_pt,
        ink: [0.0, 0.0, 0.0],
    }
}

/// Size (pt) of the page area the 3D view fills, and the annotation bands
/// around it. The annotations' extent does not depend on the window, so a
/// placeholder window is enough to size them.
fn content_area(v: &View, s: &FigureSettings) -> ((f64, f64), figure::Bands) {
    let page = page_for(s);
    let probe = annotations(v, s, [-1.0, 1.0, -1.0, 1.0], &page);
    let b = figure::bands(&probe, &page);
    let w = (page.width_pt() - 2.0 * b.side).max(page.width_pt() * 0.3);
    let h = (page.height_pt() - b.top - b.bottom).max(page.height_pt() * 0.3);
    ((w, h), b)
}

/// Fit the structure into the figure frame on the current canvas.
fn fit(view: &Rc<RefCell<View>>, ui: &Ui, s: &FigureSettings) {
    let Some(area) = ui.area.upgrade() else { return };
    let sf = area.scale_factor() as f64;
    let (cw, ch) = (area.width() as f64 * sf, area.height() as f64 * sf);
    if cw < 1.0 || ch < 1.0 {
        return;
    }
    let mut v = view.borrow_mut();
    let ((w, h), _) = content_area(&v, s);
    super::fit_into_frame(&mut v, cw, ch, w / h);
    drop(v);
    redraw(ui);
}

/// Render and write the figure to `path`.
fn export(path: &Path, view: &Rc<RefCell<View>>, ui: &Ui, s: &FigureSettings) -> Result<String, String> {
    let area = ui.area.upgrade().ok_or("3D view is closed")?;
    area.make_current();
    if let Some(e) = area.error() {
        return Err(e.to_string());
    }
    let mut guard = view.borrow_mut();
    let v = &mut *guard;
    if v.field.is_none() || v.backend.is_none() {
        return Err("load a CHGCAR first".into());
    }
    prepare_backend(v);

    let page = page_for(s);
    let (content, bands) = content_area(v, s);
    // The frame drawn on the canvas maps onto the page's content area; the
    // window grows by the annotation bands at the same scale.
    let sf = area.scale_factor() as f64;
    let (cw, ch) = (area.width() as f64 * sf, area.height() as f64 * sf);
    let (_, hh) = v.camera.half_extents(cw, ch);
    let a_per_px = 2.0 * hh / ch;
    let (fw, fh) = frame_rect(cw, ch, content.0 / content.1);
    let a_per_pt = fw * a_per_px / content.0;
    let (cx, cy) = (fw / 2.0 * a_per_px, fh / 2.0 * a_per_px);
    let window = [
        -cx - bands.side * a_per_pt,
        cx + bands.side * a_per_pt,
        -cy - bands.bottom * a_per_pt,
        cy + bands.top * a_per_pt,
    ];

    let (wpx, hpx) = (figure::pixels_for(s.width_mm, s.dpi), figure::pixels_for(s.height_mm, s.dpi));
    let req = ExportRequest {
        window,
        width: wpx,
        height: hpx,
        supersample: s.supersample,
        background: (!s.transparent).then_some(super::BACKGROUND),
        line_width_px: (s.line_pt * s.dpi / 72.0) as f32,
        max_layers: 12,
    };
    let t = std::time::Instant::now();
    let camera = v.camera.clone();
    let img = v.backend.as_mut().expect("checked").render_image(&camera, &req, &mut |_| {})?;
    let ann = annotations(v, s, window, &page);
    figure::write(path, &img, &page, &ann)?;
    if let Some(scene) = scene_file::from_view(v, Some(s.clone())) {
        let scene_path = path.with_extension("cview.json");
        scene_file::save(&scene_path, &scene)?;
    }
    Ok(format!(
        "Figure saved to {} ({} × {} px, {:.0} × {:.0} mm at {:.0} dpi, {:.1} s); scene saved alongside",
        path.display(),
        wpx,
        hpx,
        s.width_mm,
        s.height_mm,
        s.dpi,
        t.elapsed().as_secs_f64()
    ))
}

fn labeled(label: &str, w: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let b = gtk4::Box::new(Orientation::Horizontal, 8);
    let l = gtk4::Label::new(Some(label));
    l.set_halign(Align::Start);
    l.set_hexpand(true);
    b.append(&l);
    b.append(w);
    b
}

pub(super) fn show(parent: &gtk4::Window, view: Rc<RefCell<View>>, ui: Ui) {
    let s0 = LAST.with(|l| l.borrow().clone());
    let win = gtk4::Window::builder()
        .title("Export Figure")
        .transient_for(parent)
        .default_width(380)
        .resizable(false)
        .build();
    let body = gtk4::Box::new(Orientation::Vertical, 8);
    body.set_margin_top(12);
    body.set_margin_bottom(12);
    body.set_margin_start(12);
    body.set_margin_end(12);

    let preset_names: Vec<&str> = PRESETS.iter().map(|p| p.0).collect();
    let preset = gtk4::DropDown::from_strings(&preset_names);
    preset.set_selected(s0.preset as u32);
    let width = gtk4::SpinButton::with_range(20.0, 500.0, 0.5);
    width.set_digits(1);
    width.set_value(s0.width_mm);
    let height = gtk4::SpinButton::with_range(10.0, 500.0, 0.5);
    height.set_digits(1);
    height.set_value(s0.height_mm);
    let dpi = gtk4::DropDown::from_strings(&["300 dpi", "450 dpi", "600 dpi (recommended)", "1200 dpi"]);
    dpi.set_selected(DPIS.iter().position(|d| *d == s0.dpi).unwrap_or(2) as u32);
    let ss = gtk4::DropDown::from_strings(&["2× (fast)", "3× (recommended)", "4× (finest)"]);
    ss.set_selected(SUPERSAMPLES.iter().position(|d| *d == s0.supersample).unwrap_or(1) as u32);
    let bg = gtk4::DropDown::from_strings(&["White", "Transparent"]);
    bg.set_selected(s0.transparent as u32);
    let font = gtk4::SpinButton::with_range(4.0, 14.0, 0.5);
    font.set_digits(1);
    font.set_value(s0.font_pt);
    let line = gtk4::SpinButton::with_range(0.1, 3.0, 0.05);
    line.set_digits(2);
    line.set_value(s0.line_pt);

    body.append(&labeled("Size", &preset));
    let size_row = gtk4::Box::new(Orientation::Horizontal, 6);
    size_row.append(&width);
    size_row.append(&gtk4::Label::new(Some("×")));
    size_row.append(&height);
    size_row.append(&gtk4::Label::new(Some("mm")));
    body.append(&labeled("Width × height", &size_row));
    body.append(&labeled("Resolution", &dpi));
    body.append(&labeled("Supersampling", &ss));
    body.append(&labeled("Background", &bg));
    body.append(&labeled("Font size (pt, at final size)", &font));
    body.append(&labeled("Line width (pt)", &line));

    body.append(&gtk4::Separator::new(Orientation::Horizontal));
    let check = |label: &str, on: bool| {
        let c = gtk4::CheckButton::with_label(label);
        c.set_active(on);
        c
    };
    let letter = gtk4::Entry::new();
    letter.set_text(&s0.panel_letter);
    letter.set_width_chars(3);
    letter.set_max_length(3);
    body.append(&labeled("Panel letter", &letter));
    let c_title = check("Title (density name)", s0.title);
    let c_key = check("Isovalue key", s0.key);
    let c_cbar = check("Colour bar (when a section is shown)", s0.color_bar);
    let c_axes = check("Axes (a, b, c)", s0.axes);
    let c_scale = check("Scale bar (Å)", s0.scale_bar);
    let c_labels = check("Atom labels", s0.atom_labels);
    for c in [&c_title, &c_key, &c_cbar, &c_axes, &c_scale, &c_labels] {
        body.append(c);
    }

    let info = gtk4::Label::new(None);
    info.set_wrap(true);
    info.set_xalign(0.0);
    info.add_css_class("dim-label");
    body.append(&info);
    let buttons = gtk4::Box::new(Orientation::Horizontal, 8);
    buttons.set_halign(Align::End);
    let fit_btn = gtk4::Button::with_label("Fit to Frame");
    fit_btn.set_tooltip_text(Some("Zoom so the cell and atoms fill the figure frame"));
    buttons.append(&fit_btn);
    let close = gtk4::Button::with_label("Close");
    let go = gtk4::Button::with_label("Export…");
    go.add_css_class("suggested-action");
    buttons.append(&close);
    buttons.append(&go);
    body.append(&buttons);
    win.set_child(Some(&body));

    // Current settings from the controls.
    let read: Rc<dyn Fn() -> FigureSettings> = {
        let (preset, width, height, dpi, ss, bg, font, line, letter) =
            (preset.clone(), width.clone(), height.clone(), dpi.clone(), ss.clone(), bg.clone(), font.clone(), line.clone(), letter.clone());
        let checks = [c_title.clone(), c_key.clone(), c_cbar.clone(), c_axes.clone(), c_scale.clone(), c_labels.clone()];
        Rc::new(move || FigureSettings {
            preset: preset.selected() as usize,
            width_mm: width.value(),
            height_mm: height.value(),
            dpi: DPIS[(dpi.selected() as usize).min(DPIS.len() - 1)],
            supersample: SUPERSAMPLES[(ss.selected() as usize).min(SUPERSAMPLES.len() - 1)],
            transparent: bg.selected() == 1,
            font_pt: font.value(),
            line_pt: line.value(),
            panel_letter: letter.text().to_string(),
            title: checks[0].is_active(),
            key: checks[1].is_active(),
            color_bar: checks[2].is_active(),
            axes: checks[3].is_active(),
            scale_bar: checks[4].is_active(),
            atom_labels: checks[5].is_active(),
        })
    };

    // Keep the frame on the canvas and the size readout current.
    let refresh: Rc<dyn Fn()> = {
        let (view, ui, info, read) = (view.clone(), ui.clone(), info.clone(), read.clone());
        Rc::new(move || {
            let s = read();
            let aspect = {
                let v = view.borrow();
                let ((w, h), _) = content_area(&v, &s);
                w / h
            };
            view.borrow_mut().export_frame = Some(aspect);
            let (w, h) = (figure::pixels_for(s.width_mm, s.dpi), figure::pixels_for(s.height_mm, s.dpi));
            let mut text = format!(
                "{w} × {h} px at {:.0} dpi, rendered at {}× ({} × {} px). The red frame on the 3D view is the picture area; annotations sit in bands above and below it.",
                s.dpi,
                s.supersample,
                w * s.supersample,
                h * s.supersample
            );
            let nature = PRESETS.get(s.preset).is_some_and(|p| p.0.starts_with("Nature"));
            if nature && s.height_mm > NATURE_MAX_HEIGHT_MM {
                text += &format!("\n⚠ Nature figures are at most {NATURE_MAX_HEIGHT_MM:.0} mm tall.");
            }
            if nature && !(5.0..=7.0).contains(&s.font_pt) {
                text += "\n⚠ Nature asks for 5–7 pt text at final size.";
            }
            info.set_text(&text);
            redraw(&ui);
        })
    };
    {
        let (width, refresh) = (width.clone(), refresh.clone());
        preset.connect_selected_notify(move |d| {
            if let Some((_, mm)) = PRESETS.get(d.selected() as usize) {
                if *mm > 0.0 {
                    width.set_value(*mm);
                }
            }
            refresh();
        });
    }
    for spin in [&width, &height, &font, &line] {
        let r = refresh.clone();
        spin.connect_value_changed(move |_| r());
    }
    {
        let preset = preset.clone();
        width.connect_value_changed(move |w| {
            // Typing a width that is not the preset's makes it custom.
            let sel = preset.selected() as usize;
            if PRESETS.get(sel).is_some_and(|p| p.1 > 0.0 && (p.1 - w.value()).abs() > 0.05) {
                preset.set_selected((PRESETS.len() - 1) as u32);
            }
        });
    }
    for dd in [&dpi, &ss, &bg] {
        let r = refresh.clone();
        dd.connect_selected_notify(move |_| r());
    }
    refresh();
    // Start with the structure filling the figure.
    fit(&view, &ui, &read());
    {
        let (view, ui, read) = (view.clone(), ui.clone(), read.clone());
        fit_btn.connect_clicked(move |_| fit(&view, &ui, &read()));
    }

    {
        let view = view.clone();
        let ui2 = ui.clone();
        let read2 = read.clone();
        win.connect_close_request(move |_| {
            view.borrow_mut().export_frame = None;
            LAST.with(|l| *l.borrow_mut() = read2());
            redraw(&ui2);
            glib::Propagation::Proceed
        });
    }
    {
        let w = win.downgrade();
        close.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.close();
            }
        });
    }
    {
        let (view, ui, read, info) = (view.clone(), ui.clone(), read.clone(), info.clone());
        let win_w = win.downgrade();
        go.connect_clicked(move |_| {
            let Some(win) = win_w.upgrade() else { return };
            let d = gtk4::FileChooserNative::new(
                Some("Export Figure"),
                Some(&win),
                gtk4::FileChooserAction::Save,
                Some("Export"),
                Some("Cancel"),
            );
            for (name, pat) in [
                ("PDF — vector annotations (*.pdf)", "*.pdf"),
                ("SVG — vector annotations (*.svg)", "*.svg"),
                ("PNG (*.png)", "*.png"),
                ("TIFF (*.tiff)", "*.tiff"),
            ] {
                let f = gtk4::FileFilter::new();
                f.set_name(Some(name));
                f.add_pattern(pat);
                d.add_filter(&f);
            }
            d.set_current_name("charge_density.pdf");
            let (view, ui, read, info) = (view.clone(), ui.clone(), read.clone(), info.clone());
            d.connect_response(move |d, r| {
                if r == gtk4::ResponseType::Accept {
                    if let Some(mut path) = d.file().and_then(|f| f.path()) {
                        if path.extension().is_none() {
                            path.set_extension("pdf");
                        }
                        let s = read();
                        LAST.with(|l| *l.borrow_mut() = s.clone());
                        info.set_text("Rendering…");
                        // Let the label paint before the (blocking) render.
                        let (view, ui, info) = (view.clone(), ui.clone(), info.clone());
                        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                            match export(&path, &view, &ui, &s) {
                                Ok(msg) => {
                                    console::log_info(&msg);
                                    info.set_text(&msg);
                                }
                                Err(e) => {
                                    console::log_error(&format!("Figure export failed: {e}"));
                                    info.set_text(&format!("Export failed: {e}"));
                                }
                            }
                            redraw(&ui);
                        });
                    }
                }
                d.destroy();
            });
            d.show();
        });
    }
    win.present();
}

/// Export without the dialog (scripting, tests): the structure is fitted
/// to the figure first, as there is no frame to arrange it in by hand.
pub(crate) fn export_with(path: &Path, view: &Rc<RefCell<View>>, ui: &Ui, s: &FigureSettings) -> Result<String, String> {
    fit(view, ui, s);
    export(path, view, ui, s)
}
